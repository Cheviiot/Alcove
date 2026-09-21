// SPDX-License-Identifier: GPL-3.0-only
//! The diagnostic harness behind `alcove-native-chromium`: the hidden
//! command line and the window it builds for the audits under `tests/engine`.

use adw::prelude::*;
use anyhow::{bail, Context, Result};
use gtk::{gio, glib};
use std::{
    cell::Cell,
    fs::{self},
    os::unix::fs::PermissionsExt,
    rc::Rc,
};

use crate::engines::chromium::protocol;

use crate::ui::shell::chromium::{
    options::Options,
    site::store,
    window::build_view,
    worker::process::{View, Worker},
};

impl Options {
    /// The hidden `--alcove-*` command line the audit harness drives.
    pub(super) fn parse() -> Result<Self> {
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
            policy: store::Policy::new(crate::domain::policy::AppPolicyV2::default(), None),
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

pub(super) fn build_window(app: &adw::Application, options: Options) -> Result<()> {
    let worker = Worker::start(&options)?;
    build_view(app, options, View::new(worker, 1), None).map(|_| ())
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
