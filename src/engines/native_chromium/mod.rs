// SPDX-License-Identifier: GPL-3.0-only
mod accessibility;
mod background;
mod file_portal;
mod gpu;
mod policy;
mod popup_surface;
mod protocol;
mod site_requests;
use crate::ui::shell::web_app_shell::{
    adjusted_zoom_level, WebAppShell, DEFAULT_ZOOM_LEVEL, ZOOM_STEP,
};
pub use policy::PolicyStore;
pub const WORKER_PROTOCOL: u32 = protocol::VERSION as u32;

use adw::prelude::*;
use anyhow::{bail, Context, Result};
use gettextrs::gettext;
use gtk::{gdk, gio, glib};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::PermissionsExt,
    os::{
        fd::AsRawFd,
        unix::{net::UnixDatagram, process::CommandExt},
    },
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Options {
    worker: PathBuf,
    cef: PathBuf,
    output: PathBuf,
    url: String,
    seconds: u64,
    gpu: bool,
    layout: bool,
    native_accessibility: bool,
    fake_media: bool,
    real_site: bool,
    diagnostics: bool,
    profile: Option<PathBuf>,
    title: String,
    width: i32,
    height: i32,
    maximized: bool,
    user_agent: Option<String>,
    keep_alive: Option<Rc<dyn std::any::Any>>,
    policy: Rc<policy::Policy>,
    app_id: Option<crate::domain::model::AppId>,
    start_in_background: bool,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mut values = std::collections::HashMap::new();
        while let Some(key) = args.next() {
            let value = args.next().context("each option needs a value")?;
            values.insert(key, value);
        }
        let mut take = |name: &str| {
            values
                .remove(name)
                .with_context(|| format!("missing {name}"))
        };
        let result = Self {
            diagnostics: true,
            profile: None,
            title: "Alcove — Chromium Prototype".into(),
            width: 800,
            height: 640,
            maximized: false,
            user_agent: None,
            keep_alive: None,
            app_id: None,
            start_in_background: false,
            policy: policy::Policy::new(crate::domain::policy::AppPolicyV2::default(), None),
            worker: take("--worker")?.into(),
            cef: take("--cef-root")?.into(),
            output: take("--output")?.into(),
            url: take("--url")?,
            seconds: values
                .remove("--seconds")
                .unwrap_or_else(|| "0".into())
                .parse()?,
            gpu: values.remove("--gpu").as_deref() == Some("true"),
            layout: values.remove("--layout").as_deref() == Some("true"),
            native_accessibility: values.remove("--native-accessibility").as_deref()
                == Some("true"),
            fake_media: values.remove("--fake-media").as_deref() == Some("true"),
            real_site: values.remove("--real-site").as_deref() == Some("true"),
        };
        if !values.is_empty() {
            bail!("unknown options: {:?}", values.keys());
        }
        protocol::validate_url(&result.url)?;
        fs::create_dir(&result.output).context("output directory must not already exist")?;
        fs::set_permissions(&result.output, fs::Permissions::from_mode(0o700))?;
        Ok(result)
    }
}

/// Inputs supplied by Alcove after validating the app configuration and policy.
/// `lifetime` retains the repository profile lock until all CEF windows close.
pub struct Launch {
    pub app_id: crate::domain::model::AppId,
    pub start_in_background: bool,
    pub worker: PathBuf,
    pub cef: PathBuf,
    pub profile: PathBuf,
    pub url: String,
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    pub user_agent: Option<String>,
    pub lifetime: Rc<dyn std::any::Any>,
    pub policy: crate::domain::policy::AppPolicyV2,
    pub policy_store: Rc<dyn PolicyStore>,
}

pub fn open(app: &adw::Application, launch: Launch) -> Result<adw::ApplicationWindow> {
    protocol::validate_url(&launch.url)?;
    launch.policy.validate()?;
    let directory = tempfile::Builder::new()
        .prefix("alcove-cef-")
        .tempdir_in(glib::user_runtime_dir())
        .context("create private CEF runtime directory")?;
    fs::create_dir_all(&launch.profile)?;
    fs::set_permissions(&launch.profile, fs::Permissions::from_mode(0o700))?;
    let mut options = Options {
        app_id: Some(launch.app_id),
        start_in_background: launch.start_in_background,
        worker: launch.worker,
        cef: launch.cef,
        output: directory.path().to_path_buf(),
        profile: Some(launch.profile),
        url: launch.url,
        title: launch.title,
        width: launch.width,
        height: launch.height,
        maximized: launch.maximized,
        user_agent: launch.user_agent,
        keep_alive: Some(Rc::new((directory, launch.lifetime))),
        policy: policy::Policy::new(launch.policy, Some(launch.policy_store)),
        seconds: 0,
        gpu: true,
        native_accessibility: true,
        diagnostics: false,
        layout: false,
        fake_media: false,
        real_site: false,
    };
    let worker = Worker::start(&options)?;
    // Only the worker owns the profile lock and IPC directory. View options
    // must not extend their lifetime through GTK signal closures.
    options.keep_alive = None;
    build_view(app, options, View::new(worker, 1), None)
}

struct Worker {
    directory: PathBuf,
    keep_alive: RefCell<Option<Rc<dyn std::any::Any>>>,
    child: RefCell<Child>,
    input: RefCell<ChildStdin>,
    receiver: mpsc::Receiver<Value>,
    gpu_socket: UnixDatagram,
    inboxes: RefCell<std::collections::HashMap<u64, Vec<Value>>>,
    textures: RefCell<std::collections::HashMap<(u64, gpu::Surface), gpu::Texture>>,
    created: RefCell<Vec<Value>>,
    windows: RefCell<std::collections::HashMap<u64, glib::WeakRef<adw::ApplicationWindow>>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        let child = self.child.get_mut();
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Worker {
    fn start(options: &Options) -> Result<Rc<Self>> {
        let backend =
            if gdk::Display::default().is_some_and(|d| d.type_().name().contains("Wayland")) {
                "wayland"
            } else {
                "x11"
            };
        let (gpu_socket, child_socket) = UnixDatagram::pair()?;
        let mut command = Command::new(&options.worker);
        command
            .arg(format!("--probe-directory={}", options.output.display()))
            .arg(format!("--cef-root={}", options.cef.display()))
            .arg(format!("--url={}", options.url))
            .arg(format!("--ozone-platform={backend}"))
            .arg("--gtk-version=4")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg(format!("--gpu-socket={}", child_socket.as_raw_fd()))
            .args(if options.gpu {
                vec!["--probe-gpu"]
            } else {
                vec![]
            })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if options.diagnostics {
                Stdio::from(File::create(options.output.join("worker-stderr.log"))?)
            } else {
                Stdio::null()
            });
        if options.diagnostics {
            command.arg("--alcove-diagnostics");
        }
        if let Some(profile) = &options.profile {
            command.arg(format!("--alcove-profile-root={}", profile.display()));
        }
        if let Some(agent) = &options.user_agent {
            command.arg(format!("--user-agent={agent}"));
        }
        let policy_path = options.output.join("policy.json");
        fs::write(
            &policy_path,
            serde_json::to_vec(&*options.policy.current.borrow())?,
        )?;
        command.arg(format!("--alcove-policy={}", policy_path.display()));
        command.args(policy::proxy_arguments(
            &options.policy.current.borrow().proxy,
        )?);
        if backend == "wayland" {
            // Chromium 152's per-surface scale override falls back to 1 for
            // OSR's absent native window. GTK owns the actual Wayland surface;
            // let GetScreenInfo carry its scale instead. GTK fractional scaling
            // is unaffected. Revalidate this workaround on each CEF upgrade.
            command.arg("--disable-features=WaylandFractionalScaleV1");
        }
        if options.native_accessibility {
            command
                .args([
                    "--probe-native-accessibility",
                    "--force-renderer-accessibility=complete",
                ])
                .env("ACCESSIBILITY_ENABLED", "1")
                .env("GNOME_ACCESSIBILITY", "1");
        }
        if options.fake_media {
            // Synthetic devices exercise the real permission callbacks without
            // recording from the user's microphone or camera. No auto-grant.
            command.arg("--use-fake-device-for-media-stream");
        }
        // SAFETY: this child-only operation uses only an async-signal-safe fcntl
        // on an already-created descriptor. The parent keeps CLOEXEC set.
        unsafe {
            command.pre_exec(move || {
                rustix::io::fcntl_setfd(&child_socket, rustix::io::FdFlags::empty())?;
                Ok(())
            });
        }
        let mut child = command.spawn().context("start CEF worker")?;
        let input = child.stdin.take().context("worker input")?;
        let output = child.stdout.take().context("worker output")?;
        let log = if options.diagnostics {
            Some(File::create(options.output.join("events.jsonl"))?)
        } else {
            None
        };
        let (sender, receiver) = mpsc::sync_channel(32);
        std::thread::spawn(move || {
            let mut log = log;
            let mut reader = BufReader::new(output);
            loop {
                let mut line = String::new();
                // Accessibility trees may be larger than ordinary state messages.
                match reader.by_ref().take(4 * 1024 * 1024).read_line(&mut line) {
                    Ok(0) => break,
                    Err(error) => {
                        let _ = sender.send(json!({"event":"protocol-error", "message":format!("worker event read failed: {error}")}));
                        break;
                    }
                    Ok(_) if !line.ends_with('\n') => {
                        let _ = sender.send(json!({"event":"protocol-error", "message":"worker event exceeded 4 MiB or was truncated"}));
                        break;
                    }
                    Ok(_) => {}
                }
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if let Some(log) = &mut log {
                    let _ = log.write_all(line.as_bytes());
                }
                if sender.send(value).is_err() {
                    break;
                }
            }
        });
        Ok(Rc::new(Self {
            directory: options.output.clone(),
            keep_alive: RefCell::new(options.keep_alive.clone()),
            child: RefCell::new(child),
            input: RefCell::new(input),
            receiver,
            gpu_socket,
            inboxes: Default::default(),
            textures: Default::default(),
            created: Default::default(),
            windows: Default::default(),
        }))
    }
    fn shutdown(self: &Rc<Self>, app: &adw::Application) {
        self.send("quit", json!({}));
        let hold = app.hold();
        let worker = self.clone();
        let deadline = Instant::now() + Duration::from_secs(3);
        glib::timeout_add_local(Duration::from_millis(20), move || {
            let _ = &hold;
            let exited = worker
                .child
                .borrow_mut()
                .try_wait()
                .ok()
                .flatten()
                .is_some();
            if exited || Instant::now() >= deadline {
                if !exited {
                    let mut child = worker.child.borrow_mut();
                    let _ = child.kill();
                    let _ = child.wait();
                }
                worker.keep_alive.borrow_mut().take();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
    fn send(&self, name: &str, parameters: Value) {
        let command = protocol::command(name, parameters);
        let _ = writeln!(self.input.borrow_mut(), "{command}");
    }
    fn pump(&self) {
        for event in self.receiver.try_iter().take(128) {
            let view = event["view"].as_u64().unwrap_or(1);
            if event["event"] == "view-created" {
                self.created.borrow_mut().push(event.clone());
                self.inboxes.borrow_mut().entry(view).or_default();
            }
            if let Some(inbox) = self.inboxes.borrow_mut().get_mut(&view) {
                // Inboxes are drained every frame. Bound a stalled/closed view.
                if inbox.len() < 256 {
                    inbox.push(event);
                }
            }
        }
        for _ in 0..16 {
            match gpu::receive(&self.gpu_socket) {
                Ok(Some(texture)) => {
                    if self.inboxes.borrow().contains_key(&texture.view) {
                        self.textures
                            .borrow_mut()
                            .insert((texture.view, texture.surface), texture);
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    if let Some(inbox) = self.inboxes.borrow_mut().get_mut(&1) {
                        inbox.push(json!({"event":"gpu-error", "message":error.to_string()}));
                    }
                    break;
                }
            }
        }
    }
    fn stop(&self) {
        self.send("quit", json!({}));
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.child.borrow_mut().try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.borrow_mut().kill();
        let _ = self.child.borrow_mut().wait();
    }
}

// A native window owns a view endpoint, while all endpoints share one worker.
// Routing is mandatory for commands, requests, accessibility and GPU frames.
struct View {
    engine: Rc<Worker>,
    id: u64,
}
impl std::ops::Deref for View {
    type Target = Worker;
    fn deref(&self) -> &Worker {
        &self.engine
    }
}
impl View {
    fn new(engine: Rc<Worker>, id: u64) -> Rc<Self> {
        engine.inboxes.borrow_mut().entry(id).or_default();
        Rc::new(Self { engine, id })
    }
    fn send(&self, name: &str, mut parameters: Value) {
        parameters["view"] = json!(self.id);
        self.engine.send(name, parameters);
    }
    fn events(&self) -> Vec<Value> {
        self.engine.pump();
        self.engine
            .inboxes
            .borrow_mut()
            .get_mut(&self.id)
            .map(std::mem::take)
            .unwrap_or_default()
    }
    fn texture(&self, surface: gpu::Surface) -> Option<gpu::Texture> {
        self.engine
            .textures
            .borrow_mut()
            .remove(&(self.id, surface))
    }
    fn retire(&self) {
        self.engine.inboxes.borrow_mut().remove(&self.id);
        self.engine
            .textures
            .borrow_mut()
            .retain(|(view, _), _| *view != self.id);
        self.engine.windows.borrow_mut().remove(&self.id);
    }
}

// Probe observations wrap the same shell used by the normal WebKit window.
struct Chrome {
    shell: Rc<WebAppShell>,
    dialogs: Cell<u32>,
    dialog_since: Cell<Option<Instant>>,
    dialog_hold_observed: Cell<bool>,
    dialog_hold_failed: Cell<bool>,
}
impl std::ops::Deref for Chrome {
    type Target = WebAppShell;
    fn deref(&self) -> &Self::Target {
        &self.shell
    }
}
impl Chrome {
    fn begin_dialog(&self) {
        if self.dialogs.get() == 0 {
            self.dialog_since.set(Some(Instant::now()));
        }
        self.dialogs.set(self.dialogs.get() + 1);
        self.shell.begin_dialog();
    }
    fn end_dialog(&self) {
        self.dialogs.set(self.dialogs.get().saturating_sub(1));
        if self.dialogs.get() == 0 {
            self.dialog_since.set(None);
        }
        self.shell.end_dialog();
    }
    fn tick(&self) {
        if self
            .dialog_since
            .get()
            .is_some_and(|t| t.elapsed() > Duration::from_millis(1500))
        {
            self.dialog_hold_observed.set(true);
            if !self.widget().reveals_top_bars() {
                self.dialog_hold_failed.set(true);
            }
        }
    }
}

fn capture(window: &adw::ApplicationWindow, path: PathBuf) -> Result<()> {
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(window)).snapshot(
        &snapshot,
        window.width() as f64,
        window.height() as f64,
    );
    let node = snapshot.to_node().context("no rendered window")?;
    window
        .renderer()
        .context("no renderer")?
        .render_texture(&node, None)
        .save_to_png(path)?;
    Ok(())
}

fn cpu_texture(path: &std::path::Path) -> Result<gdk::Texture> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(protocol::MAX_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let (w, h) = protocol::frame_dimensions(&bytes)?;
    let data = glib::Bytes::from_owned(bytes[16..].to_vec());
    Ok(gdk::MemoryTexture::new(
        w,
        h,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &data,
        w as usize * 4,
    )
    .upcast())
}

fn modifiers(state: gdk::ModifierType) -> u32 {
    let mut result = 0;
    for (gdk, cef) in [
        (gdk::ModifierType::SHIFT_MASK, 1 << 1),
        (gdk::ModifierType::CONTROL_MASK, 1 << 2),
        (gdk::ModifierType::ALT_MASK, 1 << 3),
        (gdk::ModifierType::BUTTON1_MASK, 1 << 4),
        (gdk::ModifierType::BUTTON2_MASK, 1 << 5),
        (gdk::ModifierType::BUTTON3_MASK, 1 << 6),
    ] {
        if state.contains(gdk) {
            result |= cef;
        }
    }
    result
}

fn virtual_key(key: gdk::Key) -> i32 {
    match key {
        gdk::Key::BackSpace => 8,
        gdk::Key::Tab | gdk::Key::ISO_Left_Tab => 9,
        gdk::Key::Return | gdk::Key::KP_Enter => 13,
        gdk::Key::Escape => 27,
        gdk::Key::Shift_L | gdk::Key::Shift_R => 16,
        gdk::Key::Control_L | gdk::Key::Control_R => 17,
        gdk::Key::Alt_L | gdk::Key::Alt_R => 18,
        gdk::Key::Super_L => 91,
        gdk::Key::Super_R => 92,
        gdk::Key::Left => 37,
        gdk::Key::Up => 38,
        gdk::Key::Right => 39,
        gdk::Key::Down => 40,
        gdk::Key::Delete => 46,
        gdk::Key::Home => 36,
        gdk::Key::End => 35,
        gdk::Key::Page_Up => 33,
        gdk::Key::Page_Down => 34,
        _ => key
            .to_unicode()
            .map(|c| c.to_ascii_uppercase() as i32)
            .unwrap_or(0),
    }
}

// Keep translatable messages outside glib::clone! macro input so xgettext
// can discover them when generating the application catalog.
fn show_load_failure(page: &adw::StatusPage, renderer_crashed: bool) {
    if renderer_crashed {
        page.set_title(&gettext("Website Stopped Responding"));
        page.set_description(Some(&gettext("Reload the website to continue.")));
    } else {
        page.set_title(&gettext("Could Not Load the Website"));
        page.set_description(Some(&gettext("Check your connection and try again.")));
    }
}

fn show_worker_failure(page: &adw::StatusPage) {
    page.set_title(&gettext("Chromium Stopped"));
    page.set_description(Some(&gettext(
        "Close and reopen the application to try again.",
    )));
}

fn build_window(app: &adw::Application, options: Options) -> Result<()> {
    let worker = Worker::start(&options)?;
    build_view(app, options, View::new(worker, 1), None).map(|_| ())
}

fn build_view(
    app: &adw::Application,
    options: Options,
    worker: Rc<View>,
    popup: Option<Value>,
) -> Result<adw::ApplicationWindow> {
    let primary = worker.id == 1;
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(&options.title)
        .default_width(options.width)
        .default_height(options.height)
        .build();
    if primary && options.maximized {
        window.maximize();
    }
    if let Some(event) = popup.as_ref() {
        window.set_default_size(
            event["width"].as_i64().unwrap_or(640) as i32,
            event["height"].as_i64().unwrap_or(640) as i32,
        );
        if let Some(parent) = event["opener"].as_u64().and_then(|id| {
            worker
                .windows
                .borrow()
                .get(&id)
                .and_then(glib::WeakRef::upgrade)
        }) {
            window.set_transient_for(Some(&parent));
        }
    }
    worker
        .windows
        .borrow_mut()
        .insert(worker.id, window.downgrade());
    let model = gio::Menu::new();
    model.append(Some(&gettext("Downloads")), Some("win.downloads"));
    let background_enabled = primary && options.policy.current.borrow().background.enabled;
    let background =
        background::Background::new(&window, options.app_id.clone(), background_enabled);
    window.connect_destroy(glib::clone!(
        #[strong]
        background,
        move |_| background.finish()
    ));
    if background_enabled {
        model.append(
            Some(&gettext("Stop Background Activity")),
            Some("win.stop-background"),
        );
    }
    for name in ["show-background", "stop-background"] {
        let action = gio::SimpleAction::new(name, None);
        let background = background.clone();
        action.connect_activate(move |_, _| {
            if name == "show-background" {
                background.show();
            } else {
                background.stop();
            }
        });
        window.add_action(&action);
    }
    if options.diagnostics {
        model.append(Some("О прототипе"), Some("win.about"));
    }
    let shell = WebAppShell::new(&window, &model);
    shell.set_title(&options.title);
    let zoom = Rc::new(Cell::new(DEFAULT_ZOOM_LEVEL));
    for name in [
        "back",
        "forward",
        "reload",
        "reload-bypass-cache",
        "stop",
        "home",
        "zoom-in",
        "zoom-out",
        "zoom-reset",
        "toggle-fullscreen",
    ] {
        let action = gio::SimpleAction::new(name, None);
        let worker = worker.clone();
        let zoom = zoom.clone();
        let home = options.url.clone();
        let weak_window = window.downgrade();
        action.connect_activate(move |_, _| {
            match name {
                "toggle-fullscreen" => {
                    if let Some(w) = weak_window.upgrade() {
                        if w.is_fullscreen() {
                            w.unfullscreen();
                        } else {
                            w.fullscreen();
                        }
                    }
                }
                "home" => worker.send("load", json!({"url":home})),
                "zoom-in" | "zoom-out" | "zoom-reset" => {
                    let factor = match name {
                        "zoom-in" => adjusted_zoom_level(zoom.get(), ZOOM_STEP),
                        "zoom-out" => adjusted_zoom_level(zoom.get(), -ZOOM_STEP),
                        _ => DEFAULT_ZOOM_LEVEL,
                    };
                    zoom.set(factor);
                    // WebKit uses a linear factor; Chromium uses powers of 1.2.
                    worker.send("zoom", json!({"level":factor.ln() / 1.2_f64.ln()}));
                }
                _ => worker.send(name, json!({})),
            }
        });
        window.add_action(&action);
    }
    let picture = gtk::Picture::builder()
        .accessible_role(if options.native_accessibility {
            gtk::AccessibleRole::Presentation
        } else {
            gtk::AccessibleRole::Img
        })
        .build();
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Fill);
    picture.set_focusable(false);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Website content",
    ))]);
    let site = accessibility::Site::new(&picture);
    site.set_focusable(true);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&site));
    let mut popup_surface = popup_surface::PopupSurface::new();
    overlay.add_overlay(&popup_surface.layer);
    overlay.set_measure_overlay(&popup_surface.layer, false);
    let retry = gtk::Button::builder()
        .label(gettext("Reload"))
        .action_name("win.reload")
        .halign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();
    let error_page = adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .child(&retry)
        .build();
    let content = gtk::Stack::new();
    content.add_named(&overlay, Some("site"));
    content.add_named(&error_page, Some("error"));
    shell.set_content(Some(&content));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(shell.widget()));
    window.set_content(Some(&toasts));
    let chrome = Rc::new(Chrome {
        shell,
        dialogs: Cell::new(0),
        dialog_since: Cell::new(None),
        dialog_hold_observed: Cell::new(false),
        dialog_hold_failed: Cell::new(false),
    });
    let requests = site_requests::Requests::new(
        &window,
        worker.clone(),
        chrome.clone(),
        &toasts,
        options.policy.clone(),
    );
    let downloads = gio::SimpleAction::new("downloads", None);
    downloads.connect_activate(glib::clone!(
        #[strong]
        requests,
        move |_, _| requests.show_downloads()
    ));
    window.add_action(&downloads);
    let about = gio::SimpleAction::new("about", None);
    about.connect_activate(glib::clone!(
        #[weak]
        window,
        #[strong]
        chrome,
        move |_, _| {
            let dialog = adw::AlertDialog::new(
                Some("Экспериментальная сборка"),
                Some("Прототип проверяет работу Chromium внутри нативного окна Alcove."),
            );
            dialog.add_response("close", "Закрыть");
            dialog.set_close_response("close");
            chrome.begin_dialog();
            dialog.connect_closed(glib::clone!(
                #[strong]
                chrome,
                move |_| chrome.end_dialog()
            ));
            dialog.present(Some(&window));
        }
    ));
    window.add_action(&about);
    let style = adw::StyleManager::default();
    let style_handler = style.connect_dark_notify(glib::clone!(
        #[strong]
        worker,
        move |style| {
            worker.send("theme", json!({"dark":style.is_dark()}));
        }
    ));
    let style_handler = Cell::new(Some(style_handler));
    window.connect_destroy(move |_| {
        if let Some(handler) = style_handler.take() {
            style.disconnect(handler);
        }
    });
    let pointer = Rc::new(Cell::new((0.0, 0.0)));
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(glib::clone!(
        #[strong]
        worker,
        #[strong]
        pointer,
        move |c, x, y| {
            pointer.set((x, y));
            worker.send(
                "mouse",
                json!({"x":x as i32,"y":y as i32,"modifiers":modifiers(c.current_event_state())}),
            );
        }
    ));
    picture.add_controller(motion);
    let clicks = gtk::GestureClick::new();
    clicks.set_button(0);
    clicks.connect_pressed(glib::clone!(#[strong] worker, #[weak] site, move |g,n,x,y| {
        site.grab_focus(); worker.send("click",json!({"x":x as i32,"y":y as i32,"button":g.current_button(),"count":n,"up":false,"modifiers":modifiers(g.current_event_state())}));
    }));
    clicks.connect_released(glib::clone!(#[strong] worker, move |g,n,x,y| {
        worker.send("click",json!({"x":x as i32,"y":y as i32,"button":g.current_button(),"count":n,"up":true,"modifiers":modifiers(g.current_event_state())}));
    }));
    picture.add_controller(clicks);
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    scroll.connect_scroll(glib::clone!(#[strong] worker, #[strong] pointer, move |c,dx,dy| {
        let (x,y) = pointer.get(); worker.send("scroll",json!({"x":x as i32,"y":y as i32,"dx":(-dx*40.0) as i32,"dy":(-dy*40.0) as i32,"modifiers":modifiers(c.current_event_state())}));
        glib::Propagation::Stop
    }));
    picture.add_controller(scroll);
    let ime = gtk::IMMulticontext::new();
    ime.set_client_widget(Some(&site));
    ime.connect_commit(glib::clone!(
        #[strong]
        worker,
        move |_, text| worker.send("text", json!({"text":text}))
    ));
    ime.connect_preedit_changed(glib::clone!(
        #[strong]
        worker,
        move |context| {
            worker.send(
                "composition",
                json!({"text":context.preedit_string().0.as_str()}),
            );
        }
    ));
    let input = gtk::EventControllerKey::new();
    input.connect_key_pressed(glib::clone!(#[strong] worker, #[strong] ime, move |c,key,code,state| {
        if c.current_event().is_some_and(|e| ime.filter_keypress(&e)) { return glib::Propagation::Stop; }
        worker.send("key",json!({"key":virtual_key(key),"native":code,"up":false,"modifiers":modifiers(state)}));
        glib::Propagation::Stop
    }));
    input.connect_key_released(glib::clone!(#[strong] worker, #[strong] ime, move |c,key,code,state| {
        if c.current_event().is_some_and(|e| ime.filter_keypress(&e)) { return; }
        worker.send("key",json!({"key":virtual_key(key),"native":code,"up":true,"modifiers":modifiers(state)}));
    }));
    site.add_controller(input);
    let input_focus = gtk::EventControllerFocus::new();
    input_focus.connect_enter(glib::clone!(
        #[strong]
        worker,
        #[strong]
        ime,
        move |_| {
            ime.focus_in();
            worker.send("focus", json!({"focused":true}));
        }
    ));
    input_focus.connect_leave(glib::clone!(
        #[strong]
        worker,
        #[strong]
        ime,
        move |_| {
            ime.focus_out();
            worker.send("focus", json!({"focused":false}));
        }
    ));
    site.add_controller(input_focus);
    if !primary || !options.start_in_background || !background.enter() {
        window.present();
        site.grab_focus();
    }
    chrome.reveal();
    let start = Instant::now();
    // Snapshot after GTK painted, not immediately after replacing the texture:
    // a newly invalidated GtkPicture has no stable render node yet.
    let captures = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
    let capture_errors = Rc::new(RefCell::new(Vec::<String>::new()));
    if options.diagnostics {
        window
            .frame_clock()
            .context("window frame clock")?
            .connect_after_paint(glib::clone!(
                #[weak]
                window,
                #[strong]
                captures,
                #[strong]
                capture_errors,
                move |_| {
                    for path in captures.borrow_mut().drain(..) {
                        if let Err(error) = capture(&window, path) {
                            capture_errors.borrow_mut().push(error.to_string());
                        }
                    }
                }
            ));
    }
    let mut size = (0, 0, 0);
    let mut frames = 0u64;
    let mut gpu_frames = 0u64;
    let mut gpu_presented = 0u64;
    let mut frame_size = (0, 0);
    let mut ax = 0u64;
    let mut ready = false;
    let mut last_site_focus = None;
    let mut last_visible = None;
    let mut ready_at = None;
    let mut text_sent = false;
    let mut inspected = false;
    let mut frame_seen = false;
    let mut captured = false;
    let mut revealed = false;
    let mut hidden_size = (0, 0);
    let mut layout_stage = 0;
    let mut layout_results = Vec::<Value>::new();
    let mut webgl = false;
    let mut resources = Vec::<Value>::new();
    let mut last_resource_second = 0;
    let mut last_site_sample = 0;
    let mut final_capture = false;
    let mut hidden_seen = false;
    let mut failures = Vec::<String>::new();
    let mut closed = false;
    let mut text_confirmed = false;
    let mut dialog_capture_at = None;
    let mut dialog_captured = false;
    let mut accessibility_focus_requests = Vec::<(u64, Instant)>::new();
    let closing = Rc::new(Cell::new(false));
    let engine_closed = Rc::new(Cell::new(false));
    window.connect_close_request(glib::clone!(
        #[strong]
        closing,
        #[strong]
        worker,
        #[strong]
        engine_closed,
        #[strong]
        background,
        move |_| {
            if !engine_closed.get()
                && worker
                    .child
                    .borrow_mut()
                    .try_wait()
                    .ok()
                    .flatten()
                    .is_none()
            {
                if background.enter() {
                    return glib::Propagation::Stop;
                }
                worker.send("close-view", json!({}));
                return glib::Propagation::Stop;
            }
            if closing.replace(true) {
                glib::Propagation::Proceed
            } else {
                // Keep the window alive for the final frame/report and worker
                // shutdown. The second close below completes the request.
                glib::Propagation::Stop
            }
        }
    ));
    glib::timeout_add_local(
        Duration::from_millis(16),
        glib::clone!(
            #[weak]
            window,
            #[weak]
            app,
            #[strong]
            worker,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                let current_size = (
                    if content.width() > 1 {
                        content.width()
                    } else {
                        options.width
                    },
                    if content.height() > 1 {
                        content.height()
                    } else {
                        options.height
                    },
                    window.scale_factor(),
                );
                let visible = window.is_visible();
                if ready && last_visible != Some(visible) {
                    worker.send("visibility", json!({"hidden":!visible}));
                    last_visible = Some(visible);
                }
                let site_focused = site.has_focus() && window.is_active();
                if ready && last_site_focus != Some(site_focused) {
                    worker.send("focus", json!({"focused":site_focused}));
                    last_site_focus = Some(site_focused);
                }
                if ready && current_size != size {
                    worker.send(
                        "resize",
                        json!({"width":current_size.0,"height":current_size.1,"scale":current_size.2}),
                    );
                    size = current_size;
                }
                for event in worker.events() {
                    requests.handle(&event);
                    if options.seconds > 0
                        && !dialog_captured
                        && event["event"] == "site-request"
                        && event["kind"] == "js-dialog"
                    {
                        dialog_capture_at = Some(Instant::now() + Duration::from_millis(500));
                        dialog_captured = true;
                    }
                    match event["event"].as_str().unwrap_or("") {
                        "ready" => {
                            if event["window_lifecycle"].as_u64() != Some(1) {
                                failures.push("worker lacks window lifecycle v1 capability".into());
                                worker.stop();
                                continue;
                            }
                            if event["policy_ui"].as_u64() != Some(1) {
                                failures.push("worker lacks policy UI v1 capability".into());
                                worker.stop();
                                continue;
                            }
                            if !options.diagnostics && event["app_launch"].as_u64() != Some(1) {
                                failures.push("worker lacks app launch v1 capability".into());
                                worker.stop();
                                continue;
                            }
                            if event["shell_actions"].as_u64() != Some(1) {
                                failures
                                    .push("worker lacks shared shell actions v1 capability".into());
                                worker.stop();
                                continue;
                            }
                            if event["surfaces"].as_u64() != Some(1) {
                                failures.push("worker lacks popup surfaces v1 capability".into());
                                worker.stop();
                                continue;
                            }
                            if event["multi_view"].as_u64() != Some(1) {
                                failures.push("worker lacks multi-view v1 capability".into());
                                worker.stop();
                                continue;
                            }
                            if event["protocol"].as_u64() != Some(protocol::VERSION) {
                                failures.push("worker protocol mismatch".into());
                                worker.stop();
                                continue;
                            }
                            if options.native_accessibility
                                && event["native_atspi"].as_u64() != Some(3)
                            {
                                failures.push("worker lacks native AT-SPI v3 capability".into());
                                worker.stop();
                                continue;
                            }
                            ready = true;
                            if event["site_requests"].as_u64() != Some(1) {
                                failures
                                    .push("worker lacks native site requests v1 capability".into());
                                worker.stop();
                                continue;
                            }
                            ready_at = Some(Instant::now());
                            worker.send(
                                "theme",
                                json!({"dark":adw::StyleManager::default().is_dark()}),
                            );
                        }
                        "view-closed" => {
                            engine_closed.set(true);
                            closing.set(true);
                        }
                        "popup-blocked" => {
                            toasts.add_toast(adw::Toast::new(
                                "Не удалось открыть дочернее окно: достигнут предел окон или движок завершается",
                            ));
                        }
                        "frame" => {
                            if event["surface"] == "popup" {
                                popup_surface.queue_cpu(event["generation"].as_u64().unwrap_or(0));
                            } else {
                                frames += 1;
                                frame_seen = true;
                            }
                        }
                        "gpu-frame" => {
                            if event["surface"] == "view" {
                                gpu_frames += 1;
                            }
                        }
                        "popup-surface" => {
                            if let Err(error) = popup_surface.update(&event) {
                                failures.push(error.to_string());
                            }
                        }
                        "accessibility" => ax += 1,
                        "native-accessibility-ready" => {
                            if let Some(id) = event["plug"].as_str() {
                                if let Err(error) = site.attach(id) {
                                    failures.push(format!("AT-SPI attachment: {error}"));
                                }
                            }
                        }
                        "native-accessibility-focus-request" => {
                            if let Some(id) = event["id"].as_u64() {
                                accessibility_focus_requests.push((id, Instant::now()));
                            }
                        }
                        "title" => {
                            let page_title = event["title"].as_str().unwrap_or("Chromium");
                            chrome.set_title(page_title);
                            if options.diagnostics {
                                window.set_title(Some(page_title));
                            }
                        }
                        "load-progress" => {
                            let progress = event["progress"].as_f64().unwrap_or(0.0);
                            chrome.set_loading(progress < 1.0, progress);
                        }
                        "navigation" => {
                            chrome.set_navigation(
                                event["back"].as_bool().unwrap_or(false),
                                event["forward"].as_bool().unwrap_or(false),
                            );
                            let loading = event["loading"].as_bool().unwrap_or(false);
                            chrome.set_loading(loading, 0.0);
                            if loading && content.visible_child_name().as_deref() == Some("error") {
                                content.set_visible_child_name("site");
                                site.grab_focus();
                            }
                        }
                        "load-error" | "renderer-crashed" => {
                            if !window.is_visible() {
                                background.show();
                            }
                            failures.push(event.to_string());
                            show_load_failure(&error_page, event["event"] == "renderer-crashed");
                            retry.set_visible(true);
                            content.set_visible_child_name("error");
                            retry.grab_focus();
                            chrome.set_loading(false, 0.0);
                            chrome.reveal();
                        }
                        "protocol-error" | "gpu-error" | "native-accessibility-binding-error" => {
                            failures.push(event.to_string());
                            if !options.diagnostics {
                                worker.stop();
                            }
                        }
                        "console"
                            if event["message"]
                                .as_str()
                                .is_some_and(|s| s.contains("ALCOVE_INPUT:Привет")) =>
                        {
                            text_confirmed = true;
                        }
                        "console" if event["message"].as_str() == Some("ALCOVE_WEBGL:true") => {
                            webgl = true;
                        }
                        _ => {}
                    }
                }
                accessibility_focus_requests.retain(|(id, requested)| {
                    // Complete actions only after GTK actually gives the site
                    // focus. A native modal surface cannot be bypassed through
                    // its embedded website's AT-SPI actions. Allow close animations.
                    let modal = chrome.is_modal();
                    let focused = !modal && window.is_active() && site.grab_focus();
                    if focused || modal || requested.elapsed() > Duration::from_millis(500) {
                        worker.send(
                            "native-accessibility-focus-response",
                            json!({"id":id,"focused":focused}),
                        );
                        false
                    } else {
                        true
                    }
                });
                if let Some(frame) = worker.texture(gpu::Surface::View) {
                    let texture = frame.texture;
                    frame_size = (texture.width(), texture.height());
                    picture.set_paintable(Some(&texture));
                    gpu_presented += 1;
                }
                if let Some(frame) = worker.texture(gpu::Surface::Popup) {
                    popup_surface.queue_gpu(frame);
                }
                match popup_surface.present(&options.output, window.scale_factor()) {
                    Ok(Some(generation)) => {
                        if options.seconds > 0 {
                            captures
                                .borrow_mut()
                                .push(options.output.join(format!("dropdown-{generation}.png")));
                            window.queue_draw();
                        }
                    }
                    Err(error) => failures.push(error.to_string()),
                    _ => {}
                }
                let created = std::mem::take(&mut *worker.created.borrow_mut());
                for event in created {
                    let Some(id) = event["view"].as_u64() else {
                        continue;
                    };
                    let mut child_options = options.clone();
                    child_options.seconds = 0;
                    child_options.layout = false;
                    // The engine already owns the isolated profile; a popup uses
                    // this subdirectory only for frames and diagnostics.
                    child_options.output =
                        worker.directory.clone().join("views").join(id.to_string());
                    if let Err(error) = build_view(
                        &app,
                        child_options,
                        View::new(worker.engine.clone(), id),
                        Some(event),
                    ) {
                        failures.push(format!("native popup: {error}"));
                        worker.engine.send("close-view", json!({"view":id}));
                    }
                }
                if frame_seen {
                    frame_seen = false;
                    let load = (|| -> Result<()> {
                        let texture = cpu_texture(&options.output.join("frame.bin"))?;
                        frame_size = (texture.width(), texture.height());
                        picture.set_paintable(Some(&texture));
                        Ok(())
                    })();
                    if let Err(e) = load {
                        failures.push(e.to_string());
                    }
                }
                chrome.tick();
                if dialog_capture_at.is_some_and(|deadline| Instant::now() >= deadline) {
                    dialog_capture_at = None;
                    captures
                        .borrow_mut()
                        .push(options.output.join("native-dialog.png"));
                    window.queue_draw();
                }
                let second = start.elapsed().as_secs();
                if options.diagnostics && second >= last_resource_second + 2 && resources.len() < 60
                {
                    let worker_pid = worker.child.borrow().id();
                    resources.push(json!({"second":second,
                        "host_fds":fs::read_dir("/proc/self/fd").map(|it|it.count()).ok(),
                        "worker_fds":fs::read_dir(format!("/proc/{worker_pid}/fd")).map(|it|it.count()).ok()}));
                    last_resource_second = second;
                }
                hidden_seen |= !chrome.widget().reveals_top_bars();
                if options.layout
                    && start.elapsed().as_secs() >= 5 + layout_stage
                    && layout_stage < 7
                {
                    layout_results.push(
                        json!({"stage":layout_stage,"size":[picture.width(),picture.height()],
                        "fullscreen":window.is_fullscreen(),"maximized":window.is_maximized()}),
                    );
                    match layout_stage {
                        0 => window.set_default_size(360, 640),
                        1 => window.set_default_size(800, 640),
                        2 => window.maximize(),
                        3 => window.unmaximize(),
                        4 => window.fullscreen(),
                        5 => window.unfullscreen(),
                        _ => (),
                    }
                    layout_stage += 1;
                }
                if options.real_site && ready && start.elapsed().as_secs() / 2 > last_site_sample {
                    last_site_sample = start.elapsed().as_secs() / 2;
                    // Read-only observations. Interactions come from the external
                    // AT-SPI client and the private compositor's actual input.
                    worker.send(
                        "evaluate",
                        json!({"script":include_str!("../../../tests/engine/real-site-observe.js")}),
                    );
                    if last_site_sample.is_multiple_of(3) && last_site_sample <= 36 {
                        captures.borrow_mut().push(
                            options
                                .output
                                .join(format!("site-{}.png", last_site_sample * 2)),
                        );
                        window.queue_draw();
                    }
                }
                if options.seconds > 0
                    && !options.real_site
                    && ready_at.is_some_and(|t| t.elapsed() > Duration::from_secs(3))
                    && !text_sent
                    && frames + gpu_presented > 0
                {
                    worker.send(
                        "evaluate",
                        json!({"script":"document.querySelector('#input')?.focus();"}),
                    );
                    text_sent = true;
                }
                if options.seconds > 0
                    && text_sent
                    && !inspected
                    && ready_at.is_some_and(|t| t.elapsed() > Duration::from_secs(4))
                {
                    ime.emit_by_name::<()>("commit", &[&"Привет"]);
                    worker.send("native-accessibility-inspect", json!({}));
                    worker.send("evaluate",json!({"script":"console.log('ALCOVE_INPUT:'+document.querySelector('#input')?.value);"}));
                    inspected = true;
                }
                if primary && !ready && !closed && start.elapsed() > Duration::from_secs(20) {
                    let _ = worker.child.borrow_mut().kill();
                }
                if !closed {
                    if let Ok(Some(status)) = worker.child.borrow_mut().try_wait() {
                        closed = true;
                        requests.close();
                        if !engine_closed.get() {
                            background.show();
                            failures.push(format!("CEF worker exited: {status}"));
                            show_worker_failure(&error_page);
                            retry.set_visible(false);
                            content.set_visible_child_name("error");
                            chrome.set_loading(false, 0.0);
                            chrome.reveal();
                        }
                    }
                }
                if !options.diagnostics {
                    // Ordinary app sessions can last indefinitely. Diagnostic
                    // failures are not a production browsing-history log.
                    failures.clear();
                }
                if options.seconds > 0
                    && !captured
                    && start.elapsed() > Duration::from_secs(options.seconds.saturating_sub(3))
                {
                    captures
                        .borrow_mut()
                        .push(options.output.join("window-hidden.png"));
                    hidden_size = (picture.width(), picture.height());
                    captured = true;
                }
                if options.seconds > 0
                    && !revealed
                    && start.elapsed() > Duration::from_secs(options.seconds.saturating_sub(2))
                {
                    // Exercise the same capture-phase F10 handler as a key event.
                    let controllers = window.observe_controllers();
                    for index in 0..controllers.n_items() {
                        if let Some(keys) = controllers
                            .item(index)
                            .and_downcast::<gtk::EventControllerKey>()
                        {
                            keys.emit_by_name::<bool>(
                                "key-pressed",
                                &[&gdk::Key::F10, &0u32, &gdk::ModifierType::empty()],
                            );
                        }
                    }
                    revealed = true;
                }
                if closing.get() && !options.diagnostics {
                    background.finish();
                    requests.close();
                    if primary {
                        worker.engine.shutdown(&app);
                    }
                    worker.retire();
                    window.close();
                    return glib::ControlFlow::Break;
                }
                if closing.get() && !primary {
                    requests.close();
                    let _ = capture(&window, options.output.join("window.png"));
                    let _ = fs::write(
                        options.output.join("report.json"),
                        json!({
                            "view":worker.id,"gpu_presented":gpu_presented,"cpu_frames":frames,
                            "frame_size":[frame_size.0,frame_size.1],"size":[size.0,size.1],
                            "closed_by_engine":engine_closed.get(),"errors":failures,"popup_surface":popup_surface.report()
                        })
                        .to_string(),
                    );
                    worker.retire();
                    window.close();
                    return glib::ControlFlow::Break;
                }
                if closing.get()
                    || (options.seconds > 0
                        && start.elapsed() > Duration::from_secs(options.seconds))
                {
                    if !final_capture {
                        requests.close();
                        captures
                            .borrow_mut()
                            .push(options.output.join("window.png"));
                        window.queue_draw();
                        final_capture = true;
                        return glib::ControlFlow::Continue;
                    }
                    if !captures.borrow().is_empty()
                        && start.elapsed() < Duration::from_secs(options.seconds + 3)
                    {
                        return glib::ControlFlow::Continue;
                    }
                    failures.extend(capture_errors.borrow_mut().drain(..));
                    if !options.output.join("window.png").exists() {
                        failures.push("GTK screenshot missing".into());
                    }
                    if !ready {
                        failures.push("worker handshake timeout".into());
                    }
                    if frames + gpu_presented == 0 {
                        failures.push("no frames presented".into());
                    }
                    let report = json!({"protocol":protocol::VERSION,"backend":gdk::Display::default().map(|d|d.type_().name().to_string()),
                "transport":if options.gpu {"gpu-dmabuf"} else {"cpu-diagnostic"},"cpu_frames":frames,"gpu_callbacks":gpu_frames,"gpu_presented":gpu_presented,"accessibility_updates":ax,
                "unicode_commit_confirmed":text_confirmed,"autohide_observed":hidden_seen,"size":[size.0,size.1],
                "header_visible_at_capture":chrome.widget().reveals_top_bars(),
                "dialog_hold_observed":chrome.dialog_hold_observed.get(),"dialog_hold_failed":chrome.dialog_hold_failed.get(),
                "webgl":webgl,"scale_factor":size.2,"layout_checks":layout_results,
                "frame_size":[frame_size.0,frame_size.1],
                "popup_surface":popup_surface.report(),
                "resource_samples":resources,
                "overlay_preserved_viewport":hidden_size == (picture.width(),picture.height()),
                "elapsed_seconds":start.elapsed().as_secs_f64(),"errors":failures,
                "native_accessibility":options.native_accessibility,
                "production_ready":false,"acceptance_gaps":["extended screenreader interaction and accessibility interfaces", "notification delivery and screen capture portal", "crash recovery and Flatpak sandbox verification", "real keyboard/IME and performance acceptance"]});
                    let _ = fs::write(
                        options.output.join("report.json"),
                        serde_json::to_vec_pretty(&report).unwrap(),
                    );
                    worker.stop();
                    closing.set(true);
                    window.close();
                    app.quit();
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            }
        ),
    );
    Ok(window)
}

pub fn run_probe() -> glib::ExitCode {
    // SAFETY: process startup, before GTK or any worker threads are initialized.
    unsafe {
        gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "");
    }
    let locale =
        std::env::var("ALCOVE_PROBE_LOCALEDIR").unwrap_or_else(|_| "/usr/share/locale".into());
    if let Err(error) = gettextrs::bindtextdomain("alcove", locale)
        .and_then(|_| gettextrs::bind_textdomain_codeset("alcove", "UTF-8"))
        .and_then(|_| gettextrs::textdomain("alcove"))
    {
        eprintln!("Failed to initialize translations: {error}");
        return glib::ExitCode::FAILURE;
    }
    let options = match Options::parse() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e:#}");
            return glib::ExitCode::FAILURE;
        }
    };
    let app = adw::Application::builder()
        .application_id("io.github.cheviiot.alcove.NativeProbe")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let startup_failed = Rc::new(Cell::new(false));
    app.connect_activate(glib::clone!(
        #[strong]
        startup_failed,
        move |app| {
            if let Err(e) = build_window(app, options.clone()) {
                eprintln!("{e:#}");
                startup_failed.set(true);
                app.quit();
            }
        }
    ));
    let result = app.run_with_args::<&str>(&[]);
    if startup_failed.get() {
        glib::ExitCode::FAILURE
    } else {
        result
    }
}
