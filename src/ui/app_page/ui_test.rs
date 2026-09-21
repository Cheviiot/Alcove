// SPDX-License-Identifier: GPL-3.0-only
//! Driving the application settings page under the `ui-tests` feature.

use adw::prelude::*;
use adw::subclass::prelude::*;

use super::page::AppPage;
use crate::domain::model::AppConfigV3;
use crate::engines::chromium::EngineAvailability;

impl AppPage {
    pub(crate) fn expand_advanced(&self) {
        self.imp().advanced_expander.set_expanded(true);
        self.imp().icon_expander.set_expanded(true);
        crate::ui::test_support::settle();
        let adjustment = self.imp().settings_scroll.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
    }
}

pub(crate) fn run_ui_smoke_test() -> anyhow::Result<()> {
    use anyhow::ensure;
    let config = AppConfigV3::new("UI smoke test", "https://example.org", 0)?;
    let page = AppPage::new(config, EngineAvailability::Missing);
    ensure!(!page.has_pending_changes(), "opening settings changed data");
    page.imp().title_entry.set_text("");
    ensure!(
        page.has_pending_changes() && !page.can_pop(),
        "invalid draft can disappear on navigation"
    );
    ensure!(
        page.imp().title_error.is_visible(),
        "name validation is not inline"
    );
    page.imp().url_entry.set_text("file:///tmp/test");
    ensure!(
        page.imp().url_error.is_visible(),
        "invalid address has no inline error"
    );
    let config = page.imp().config.borrow().clone().unwrap();
    page.populate(&config);
    ensure!(
        !page.has_pending_changes(),
        "restoring the saved values stayed dirty"
    );
    ensure!(
        page.imp().engine_row.model().unwrap().n_items() == 1,
        "missing engine offered for selection"
    );
    Ok(())
}
