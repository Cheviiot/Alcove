// SPDX-License-Identifier: GPL-3.0-only
//! Translating GDK input into the events the CEF worker understands.

use gtk::gdk;

pub fn modifiers(state: gdk::ModifierType) -> u32 {
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

pub fn virtual_key(key: gdk::Key) -> i32 {
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
