# Changelog

All notable Alcove changes are recorded here. The format follows Keep a
Changelog and versions follow Semantic Versioning.

## [Unreleased]

## [0.1.0] - 2026-09-20

### Added

- Turn any valid HTTP(S) address into a standalone desktop application with its
  own profile, cookies, cache and storage.
- Provide a native GNOME manager: an adaptive library, a two-step creation
  flow, per-application settings that save immediately, and native preferences
  dialogs for permissions, privacy, backups, add-ons and diagnostics.
- Share one GTK4/libadwaita runtime window between engines, with symbolic
  navigation, grouped menus and an auto-hiding header.
- Keep WebKitGTK as the default engine and offer Chromium as an optional
  Flatpak add-on that is never installed implicitly and never switched without
  confirmation.
- Integrate with the desktop through XDG portals for launchers, background
  permission, file access, downloads and notifications.
- Manage per-application policy: permissions, navigation allowlists, proxies,
  background behavior and content filters.
- Create portable backups and encrypt archives that contain website data.
- Render the optional Chromium engine with CEF inside the application's own
  GTK window, keeping the Chromium sandbox through zypak, and ship it as a
  separately installed Flatpak add-on for x86_64.
- Add reproducible interface and engine audit harnesses under `tests/`.

[Unreleased]: https://github.com/Cheviiot/Alcove/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Cheviiot/Alcove/releases/tag/v0.1.0
