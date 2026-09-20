// SPDX-License-Identifier: GPL-3.0-only

use adw::prelude::*;
use anyhow::{ensure, Context, Result};
use gtk::{gdk, glib};

pub fn settle() {
    let context = glib::MainContext::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
    while std::time::Instant::now() < deadline {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Render actual GTK widgets under Xvfb, and catch minimum-width regressions.
pub fn capture(window: &impl IsA<gtk::Window>, name: &str, width: i32, height: i32) -> Result<()> {
    let window = window.as_ref();
    window.set_default_size(width, height);
    window.present();
    settle();
    let dialog_width = window
        .downcast_ref::<adw::ApplicationWindow>()
        .and_then(|window| window.visible_dialog())
        .map(|dialog| {
            dialog.width().max(
                dialog
                    .child()
                    .map(|child| child.measure(gtk::Orientation::Horizontal, -1).0)
                    .unwrap_or(0),
            )
        })
        .unwrap_or(0);
    if window.width() > width || dialog_width > width {
        fn oversized(widget: &gtk::Widget, width: i32) {
            let (minimum, natural, _, _) = widget.measure(gtk::Orientation::Horizontal, -1);
            if minimum > width {
                eprintln!(
                    "Oversized {}: min={minimum} natural={natural}",
                    widget.type_().name()
                );
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                oversized(&current, width);
                child = current.next_sibling();
            }
        }
        oversized(window.upcast_ref(), width);
    }
    ensure!(
        window.width() <= width,
        "{name}: minimum window width {} exceeds {width}",
        window.width()
    );
    ensure!(
        dialog_width <= width,
        "{name}: dialog width {dialog_width} exceeds {width}"
    );
    let Some(directory) = std::env::var_os("BASTLE_UI_SCREENSHOTS") else {
        return Ok(());
    };
    std::fs::create_dir_all(&directory)?;
    let snapshot = gtk::Snapshot::new();
    let paintable = gtk::WidgetPaintable::new(Some(window));
    paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
    let node = snapshot.to_node().context("window has no render node")?;
    let texture = window
        .renderer()
        .context("window has no renderer")?
        .render_texture(&node, None);
    texture.save_to_png(std::path::Path::new(&directory).join(format!("{name}.png")))?;
    Ok(())
}

pub fn prepare() {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    if std::env::var("ADW_DEBUG_HIGH_CONTRAST").as_deref() == Ok("1") {
        assert!(
            adw::StyleManager::default().is_high_contrast(),
            "high contrast was not enabled"
        );
    }
    println!(
        "UI appearance: high_contrast={}, font={:?}",
        adw::StyleManager::default().is_high_contrast(),
        gtk::Settings::default().map(|settings| settings.gtk_font_name())
    );
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display)
            .add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
    }
}
