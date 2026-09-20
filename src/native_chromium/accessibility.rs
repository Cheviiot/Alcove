// SPDX-License-Identifier: GPL-3.0-only
use anyhow::{bail, Context, Result};
use glib::translate::*;
use gtk::{glib, prelude::*, subclass::prelude::*};
use std::{cell::RefCell, ffi::CString};

// Public, Linux-specific GTK API since 4.14. It is in gtk/a11y/gtkatspi.h
// rather than Gtk.gir, so gtk-rs does not generate this function.
unsafe extern "C" {
    fn gtk_at_spi_socket_new(
        bus_name: *const std::ffi::c_char,
        object_path: *const std::ffi::c_char,
        error: *mut *mut glib::ffi::GError,
    ) -> *mut gtk::ffi::GtkAccessible;
}

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Site {
        pub socket: RefCell<Option<gtk::Accessible>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Site {
        const NAME: &'static str = "BastleProbeSite";
        type Type = super::Site;
        type ParentType = gtk::Box;
        type Interfaces = (gtk::Accessible,);
    }
    impl ObjectImpl for Site {
        fn dispose(&self) {
            if let Some(socket) = self.socket.take() {
                socket.set_accessible_parent(None::<&gtk::Accessible>, None::<&gtk::Accessible>);
            }
        }
    }
    impl WidgetImpl for Site {}
    impl BoxImpl for Site {}
    impl AccessibleImpl for Site {
        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            self.socket
                .borrow()
                .clone()
                .or_else(|| self.parent_first_accessible_child())
        }
    }
}
glib::wrapper! {
    pub struct Site(ObjectSubclass<imp::Site>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}
impl Site {
    pub fn new(picture: &gtk::Picture) -> Self {
        let site: Self = glib::Object::builder()
            .property("accessible-role", gtk::AccessibleRole::Group)
            .build();
        site.append(picture);
        site
    }
    pub fn attach(&self, id: &str) -> Result<()> {
        let (bus, path) = id
            .rsplit_once(':')
            .context("invalid AT-SPI plug identifier")?;
        if !bus.starts_with(':') || !path.starts_with("/org/a11y/atspi/") {
            bail!("unexpected AT-SPI plug address");
        }
        let bus = CString::new(bus)?;
        let path = CString::new(path)?;
        let mut error = std::ptr::null_mut();
        // SAFETY: NUL-terminated strings live through the call. GTK returns an
        // owned accessible or an owned GError; both are transferred into Rust.
        let socket: gtk::Accessible = unsafe {
            let value = gtk_at_spi_socket_new(bus.as_ptr(), path.as_ptr(), &mut error);
            if !error.is_null() {
                return Err(from_glib_full::<_, glib::Error>(error).into());
            }
            if value.is_null() {
                bail!("GTK returned no AT-SPI socket");
            }
            from_glib_full(value)
        };
        if let Some(previous) = self.imp().socket.replace(Some(socket.clone())) {
            previous.set_accessible_parent(None::<&gtk::Accessible>, None::<&gtk::Accessible>);
        }
        socket.set_accessible_parent(Some(self), None::<&gtk::Accessible>);
        Ok(())
    }
}
