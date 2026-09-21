<!-- SPDX-License-Identifier: GPL-3.0-only -->

<div align="center">
  <img src="../data/icons/hicolor/scalable/apps/io.github.cheviiot.alcove.svg" width="128" alt="Alcove icon">
  <h1>Alcove</h1>
  <p><strong>Every website, its own application.</strong></p>
  <p>
    <a href="https://github.com/Cheviiot/Alcove/actions/workflows/ci.yml"><img alt="Build" src="https://img.shields.io/github/actions/workflow/status/Cheviiot/Alcove/ci.yml?branch=main&amp;style=flat-square&amp;label=build"></a>
    <a href="COPYING"><img alt="GPL-3.0-only license" src="https://img.shields.io/badge/license-GPL--3.0--only-6f7782?style=flat-square"></a>
    <img alt="GTK 4 and libadwaita" src="https://img.shields.io/badge/GTK%204-libadwaita-4a86cf?style=flat-square">
    <img alt="Flatpak" src="https://img.shields.io/badge/package-Flatpak-1c1d22?style=flat-square">
  </p>
  <p><a href="README.md">Русский</a> · <strong>English</strong></p>
</div>

Alcove turns a website into a GNOME application: its own window, profile,
cookies, cache and permissions. Two accounts on one service run side by side
and know nothing about each other. It launches from the menu and needs no
browser.

## Features

- Two accounts on one service at once — no private mode, no second browser.
- Its own icon in the menu and its own slot in the window switcher.
- Camera, microphone and notifications are granted to a site, not a browser.
- Navigation, proxy, background, content filters and user agent, per site.
- A profile exports to an encrypted archive and moves to another machine.
- System access through XDG portals only, with no broad Flatpak permissions.

## Two engines

**WebKitGTK** is the default. **Chromium** is a Flatpak add-on for sites
WebKitGTK cannot serve: installed separately, never switched to without
confirmation, and never sharing profiles or sessions with the other engine.

DRM and Widevine, browser extensions and anti-bot circumvention are not
supported.

## Installation

```sh
flatpak install --user https://cheviiot.github.io/Alcove/alcove.flatpakref
```

There are no releases yet — until the first one, build from source as described
in [CONTRIBUTING.md](CONTRIBUTING.md).

## Privacy

No analytics, no third-party calls, no reporting of the addresses you open.
The only thing Alcove reaches out to is the websites you opened yourself.

## Contributing

[Issues](https://github.com/Cheviiot/Alcove/issues) ·
[CONTRIBUTING.md](CONTRIBUTING.md) ·
[Interface](gnome-hig.md) ·
[Threat model](threat-model.md) ·
[Changelog](CHANGELOG.md)

## Provenance

An independent continuation of [Spider](https://github.com/Zaedus/spider) as of
commit `dcf9d1080ce2bbd89c342b4766a94e18aaecf660`, created by Zaedus with a
contribution by Cameron Radmore. The Git history starts fresh; authorship of
the inherited code is recorded in [AUTHORS.md](AUTHORS.md). The previous
authors do not participate in Alcove.

Licensed under GPL-3.0-only — [NOTICE](NOTICE), [COPYING](../COPYING).
