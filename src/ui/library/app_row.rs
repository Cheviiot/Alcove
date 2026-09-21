// SPDX-License-Identifier: GPL-3.0-only

use std::cell::Cell;

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;

use crate::{
    domain::model::AppConfigV3, engines::chromium::EngineAvailability, system::service::AppService,
    ui::icons,
};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/alcove/app_row.ui")]
    pub struct AppRow {
        pub generation: Cell<u64>,
        #[template_child]
        pub icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub subtitle: TemplateChild<gtk::Label>,
        #[template_child]
        pub engine_status: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppRow {
        const NAME: &'static str = "AlcoveAppRow";
        type Type = super::AppRow;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AppRow {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().set_accessible_role(gtk::AccessibleRole::Group);
        }
    }
    impl WidgetImpl for AppRow {}
    impl BoxImpl for AppRow {}
}

glib::wrapper! {
    pub struct AppRow(ObjectSubclass<imp::AppRow>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl AppRow {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_config(&self, config: AppConfigV3, availability: EngineAvailability) {
        let imp = self.imp();
        let generation = imp.generation.get().wrapping_add(1);
        imp.generation.set(generation);
        imp.title.set_label(&config.title);
        let host = url::Url::parse(&config.start_url)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| config.start_url.clone());
        imp.subtitle.set_label(&host);
        let missing =
            config.engine == crate::domain::model::Engine::Chromium && !availability.is_available();
        imp.engine_status
            .set_label(&gettext("Chromium · Add-on Required"));
        imp.engine_status.set_visible(missing);
        self.set_tooltip_text(Some(&format!("{}\n{}", config.title, host)));
        imp.icon.set_icon_name(Some("io.github.cheviiot.alcove"));

        let id = config.id.clone();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = row)]
            self,
            async move {
                let Ok(bytes) = AppService::portal().read_icon(&id) else {
                    return;
                };
                let Ok(texture) = icons::load_texture(bytes).await else {
                    return;
                };
                if row.imp().generation.get() == generation {
                    row.imp().icon.set_paintable(Some(&texture));
                }
            }
        ));
    }

    pub fn clear(&self) {
        let imp = self.imp();
        // A recycled row must ignore an icon decoded for its previous item.
        imp.generation.set(imp.generation.get().wrapping_add(1));
        imp.icon.set_icon_name(Some("io.github.cheviiot.alcove"));
        self.set_tooltip_text(None);
    }
}

impl Default for AppRow {
    fn default() -> Self {
        Self::new()
    }
}
