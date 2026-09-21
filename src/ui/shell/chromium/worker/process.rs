// SPDX-License-Identifier: GPL-3.0-only
//! The CEF child process and the per-window endpoints multiplexed onto it.

use adw::prelude::*;
use anyhow::{Context, Result};
use gtk::{gdk, glib};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
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

use crate::engines::chromium::protocol;

use crate::ui::shell::chromium::options::Options;
use crate::ui::shell::chromium::site::store;
use crate::ui::shell::chromium::worker::gpu;

pub struct Worker {
    pub directory: PathBuf,
    pub keep_alive: RefCell<Option<Rc<dyn std::any::Any>>>,
    pub child: RefCell<Child>,
    pub input: RefCell<ChildStdin>,
    pub receiver: mpsc::Receiver<Value>,
    pub gpu_socket: UnixDatagram,
    pub inboxes: RefCell<std::collections::HashMap<u64, Vec<Value>>>,
    pub textures: RefCell<std::collections::HashMap<(u64, gpu::Surface), gpu::Texture>>,
    pub created: RefCell<Vec<Value>>,
    pub windows: RefCell<std::collections::HashMap<u64, glib::WeakRef<adw::ApplicationWindow>>>,
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
    pub fn start(options: &Options) -> Result<Rc<Self>> {
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
        command.args(store::proxy_arguments(
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
    pub fn shutdown(self: &Rc<Self>, app: &adw::Application) {
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
    pub fn send(&self, name: &str, parameters: Value) {
        let command = protocol::command(name, parameters);
        let _ = writeln!(self.input.borrow_mut(), "{command}");
    }
    pub fn pump(&self) {
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
    pub fn stop(&self) {
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
pub struct View {
    pub engine: Rc<Worker>,
    pub id: u64,
}
impl std::ops::Deref for View {
    type Target = Worker;
    fn deref(&self) -> &Worker {
        &self.engine
    }
}
impl View {
    pub fn new(engine: Rc<Worker>, id: u64) -> Rc<Self> {
        engine.inboxes.borrow_mut().entry(id).or_default();
        Rc::new(Self { engine, id })
    }
    pub fn send(&self, name: &str, mut parameters: Value) {
        parameters["view"] = json!(self.id);
        self.engine.send(name, parameters);
    }
    pub fn events(&self) -> Vec<Value> {
        self.engine.pump();
        self.engine
            .inboxes
            .borrow_mut()
            .get_mut(&self.id)
            .map(std::mem::take)
            .unwrap_or_default()
    }
    pub fn texture(&self, surface: gpu::Surface) -> Option<gpu::Texture> {
        self.engine
            .textures
            .borrow_mut()
            .remove(&(self.id, surface))
    }
    pub fn retire(&self) {
        self.engine.inboxes.borrow_mut().remove(&self.id);
        self.engine
            .textures
            .borrow_mut()
            .retain(|(view, _), _| *view != self.id);
        self.engine.windows.borrow_mut().remove(&self.id);
    }
}

// Probe observations wrap the same shell used by the normal WebKit window.
