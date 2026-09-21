<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Interface

Alcove is a website-as-an-application launcher: a shared native shell hosts
either engine, and the engine displays the website. The interface uses GTK 4.22
and libadwaita 1.9, follows the [GNOME HIG](https://developer.gnome.org/hig/),
and takes the platform's spacing, typography, colors, focus indicators, dialogs
and symbolic icons. It ships no custom GTK theme. The engine itself is
described in [the Chromium engine](chromium-engine.md).

## Screen structure

- **Library:** a single `AdwNavigationView`, header actions for adding and
  searching, and a primary menu containing sorting and application utilities.
  A `GtkSearchBar` provides the same search flow at every width. The library is
  a virtualized `GtkListView` inside `AdwClampScrollable`, with the supported
  `navigation-sidebar` style. Rows contain an icon, name, domain and chevron;
  a missing Chromium add-on is indicated when needed. Activating a row opens
  its settings. Applications are launched through GNOME; the manager has no
  launch controls.
  Empty libraries and empty searches have distinct `AdwStatusPage` states.
- **Application:** compact icon/domain identity, immediately available name and
  address, appearance, permissions/behavior links and an Advanced section.
  There is no separate edit mode. Switches and engine selection save immediately;
  text saves on Enter or leaving the field. Edits are serialized and applied to
  the latest stored record, retaining unrelated settings and runtime window state.
  Runtime changes show a persistent next-launch notice. Validation and storage
  errors stay beside their fields; invalid text does not prevent another setting
  from saving. Back and window close protect any remaining unsaved draft.
  Only name/icon changes update the launcher through the system's required portal
  confirmation; ordinary preferences do not reinstall it.
- **Creation:** address → review → create, using an `AdwNavigationView` inside
  an adaptive dialog. Next fetches the site's own name/icon with a 15-second
  total deadline; Cancel and Enter Manually interrupt the lookup. An unreachable
  site has an editable hostname fallback. Returning to the same address
  preserves edited details. Engine choice is under Advanced; only installed engines are
  selectable. Invalid fields and installation errors stay inline. Portal
  cancellation keeps the draft. Successful creation returns to the library,
  clears its filter and focuses the new row after the modal dialog closes.
- **Permissions and privacy:** native `AdwPreferencesDialog` windows with
  immediate permission choices, switches, resets and filter changes. Text saves
  on Enter, focus leave or close. Failed writes restore the saved switch/choice
  and show an inline error; invalid drafts are protected on close. Permission
  reset preserves navigation, network, background and filter settings. Each edit
  merges into the current policy, retaining changes made by another window.
  Filter import uses the real file portal and WebKit validation; background
  authorization stays with the desktop portal. Changes show a next-launch notice.
- **Backup and restore:** scrollable action dialogs. Website data enables the
  encryption fields; mismatches appear inline and continuing is disabled until
  both passphrases match. A labelled cancel icon keeps the header usable with
  enlarged text at narrow widths. Native progress dialogs cover archive work.
  Incorrect decryption passwords can be retried without selecting the file again.
  Restore rows have full-row checkbox activation and accessible application names;
  identical entries are disabled and conflicts explain that a separate copy will
  be created. The restore action is disabled when nothing is selected.
- **Downloads:** a native empty state and rows with progress below the filename,
  leaving space for long names and narrow windows.
- **Utilities:** native preferences dialogs for add-ons and system capabilities;
  add-on actions use full-width button rows. Capabilities appear immediately and
  refresh in place with a busy indicator. Closing during a probe never reopens the
  dialog. `AdwShortcutsDialog` shows the shortcuts relevant to its parent window.
- **Runtime window:** symbolic navigation icons with tooltips and grouped menus,
  identical for both engines. Website content and engine isolation are unchanged.

The library's list-item factory binds recycled widgets to the current model
entry. Unbinding invalidates outstanding icon decoding, so an old icon cannot
appear on another application. A 10,000-entry test verifies bounded row
allocation and scrolling to the final item. Search and sorting replace the
visible model in one splice. List items expose their title and domain to AT-SPI
and support native Tab/arrow/Enter navigation.

## Checking it

The `ui-tests` feature builds the interface checks; run them through the gate
in [CONTRIBUTING.md](CONTRIBUTING.md) with `-Dui_tests=true`. What each screen
is driven through, at which widths, themes and text sizes, and what the checks
deliberately do not establish, is recorded in
[the UI audit instructions](../tests/ui/README.md).

Native widgets provide platform accessibility semantics; manual screen-reader
and real touchscreen evaluation are still required before claiming complete
accessibility conformance. Xvfb verifies the interface, not portal
availability — that needs a real desktop session.

## References

- [Navigation](https://developer.gnome.org/hig/guidelines/navigation.html)
- [Scaling and adaptiveness](https://developer.gnome.org/hig/guidelines/adaptive.html)
- [Header bars](https://developer.gnome.org/hig/patterns/containers/header-bars.html)
- [Dialogs](https://developer.gnome.org/hig/patterns/feedback/dialogs.html)
- [Libadwaita boxed lists](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/boxed-lists.html)
- [Libadwaita shortcuts dialog](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.ShortcutsDialog.html)
