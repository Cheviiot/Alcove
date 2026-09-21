// SPDX-License-Identifier: GPL-3.0-only

use adw::prelude::*;
use gettextrs::gettext;

use crate::engines::chromium::{self, EngineAvailability};
use crate::ui::common;

pub fn present(parent: &gtk::Window, availability: &EngineAvailability) {
    let dialog = adw::PreferencesDialog::builder()
        .title(gettext("Add-ons"))
        .content_width(560)
        .content_height(480)
        .build();
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Browser Engines"))
        .description(gettext(
            "Optional engines are installed separately and become available after Alcove is restarted.",
        ))
        .build();
    let default_engine = adw::ActionRow::builder()
        .use_markup(false)
        .title("WebKitGTK")
        .subtitle(gettext("Built-in engine for GNOME"))
        .build();
    default_engine.add_prefix(&gtk::Image::from_icon_name("web-browser-symbolic"));
    default_engine.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
    group.add(&default_engine);
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(gettext("Chromium Engine"))
        .subtitle(availability_subtitle(availability))
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("web-browser-symbolic"));
    group.add(&row);
    let button = adw::ButtonRow::builder()
        .title(availability_action(availability))
        .start_icon_name("folder-download-symbolic")
        .build();
    if !availability.is_available() {
        button.add_css_class("suggested-action");
    }
    group.add(&button);

    let restart = adw::ActionRow::builder()
        .use_markup(false)
        .title(gettext("Restart Required"))
        .subtitle(gettext(
            "Close and reopen Alcove after installing or removing this add-on.",
        ))
        .build();
    restart.add_prefix(&gtk::Image::from_icon_name("view-refresh-symbolic"));
    group.add(&restart);
    page.add(&group);
    dialog.add(&page);

    let alert_parent = parent.clone();
    button.connect_activated(move |_| {
        common::open_uri(
            &alert_parent,
            chromium::ADDON_REF_URL,
            &gettext("Could Not Open the Add-on Installer"),
        );
    });

    dialog.present(Some(parent));
}

fn availability_subtitle(availability: &EngineAvailability) -> String {
    match availability {
        EngineAvailability::Missing => gettext("Not installed — WebKitGTK remains the default"),
        EngineAvailability::Available => gettext("Installed and ready"),
        EngineAvailability::Incompatible(message) => {
            format!("{}: {message}", gettext("Update required"))
        }
        EngineAvailability::Broken(message) => {
            format!("{}: {message}", gettext("Installed but unavailable"))
        }
    }
}

fn availability_action(availability: &EngineAvailability) -> String {
    match availability {
        EngineAvailability::Missing => gettext("Install"),
        EngineAvailability::Available => gettext("Manage"),
        EngineAvailability::Incompatible(_) | EngineAvailability::Broken(_) => gettext("Reinstall"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_addon_is_presented_as_optional() {
        assert!(availability_subtitle(&EngineAvailability::Missing).contains("WebKitGTK"));
        assert_eq!(availability_action(&EngineAvailability::Missing), "Install");
    }
}
