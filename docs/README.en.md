<!-- SPDX-License-Identifier: GPL-3.0-only -->

<div align="center">
  <img src="../data/icons/hicolor/scalable/apps/io.github.cheviiot.alcove.svg" width="128" alt="Alcove icon">
  <h1>Alcove</h1>
  <p><strong>Every website, its own application.</strong></p>
  <p>
    <a href="https://github.com/Cheviiot/Alcove/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/Cheviiot/Alcove?style=flat-square&amp;label=release&amp;color=4a86cf"></a>
    <a href="https://github.com/Cheviiot/Alcove/actions/workflows/ci.yml"><img alt="Build" src="https://img.shields.io/github/actions/workflow/status/Cheviiot/Alcove/ci.yml?branch=main&amp;style=flat-square&amp;label=build"></a>
    <a href="../COPYING"><img alt="GPL-3.0-only licence" src="https://img.shields.io/badge/licence-GPL--3.0--only-6f7782?style=flat-square"></a>
    <img alt="GTK 4 and libadwaita" src="https://img.shields.io/badge/GTK%204-libadwaita-4a86cf?style=flat-square">
    <img alt="Flatpak" src="https://img.shields.io/badge/package-Flatpak-1c1d22?style=flat-square">
  </p>
  <p><a href="../README.md">Русский</a> · <strong>English</strong></p>
</div>

Alcove turns a website into a GNOME application: its own window, profile,
cookies, cache and permissions. Two accounts of the same service run side by
side and know nothing of each other. The application starts from the menu and
does not depend on a browser.

## Features

- Two accounts of one service at once — no private mode, no second browser.
- Its own icon in the menu and its own place in the window switcher.
- Camera, microphone and notifications go to the site, not to a whole browser.
- Navigation, proxy, background mode, content filters and user agent, per site.
- A profile exports into an encrypted archive and moves to another machine.
- System access through XDG portals only, with no broad Flatpak permissions.

## Installation

```sh
flatpak install --user https://cheviiot.github.io/Alcove/alcove.flatpakref
```

A 5 MB download, 13 MB installed. You need Flatpak and the
`org.gnome.Platform` 50 runtime; if it is not there yet, Flatpak pulls it from
Flathub for you.

Bundles and install references live on the
[project page](https://cheviiot.github.io/Alcove/) and under
[releases](https://github.com/Cheviiot/Alcove/releases). The repository and
every build are signed with the key
`FA64 0607 BEBF D82E 61EF 72EF 33AD DA41 5AF7 FE09`.

Built for x86_64 and aarch64. GNOME is the target; on other desktops it works
as far as their XDG portals do.

## Using it

1. Press the plus button in the header and enter a website address.
2. Next — Alcove fetches the site's own name and icon. You need not wait: you
   can interrupt it and enter everything by hand.
3. Create — the system asks for confirmation, and the application appears in
   your menu.

From there each application is configured on its own: permissions, navigation,
proxy, background mode, content filters, engine choice.

## Two engines

**WebKitGTK** is the default and is already inside — nothing to install.

**Chromium** is for sites WebKitGTK cannot serve. It installs separately, and
only if you choose it:

```sh
flatpak install --user https://cheviiot.github.io/Alcove/alcove-chromium-native.flatpakref
```

A 150 MB download, 364 MB installed, x86_64 only — on aarch64 WebKitGTK is the
only engine. Close and reopen Alcove after installing it. The engine is never
switched without confirmation, and the two engines never share a profile or a
session.

DRM and Widevine, browser extensions and anti-bot bypasses are not supported.

## Privacy

No analytics, no third-party calls, no reporting of the addresses you open.
Alcove reaches out only to the sites you opened yourself.

Its Flatpak permissions are network, sound and display, and nothing else:

```
shared=network;ipc;  sockets=wayland;pulseaudio;fallback-x11;  devices=dri;
```

Files, launchers, downloads and background mode all go through XDG portals,
each time with your confirmation. The details are in the
[threat model](threat-model.md).

## Building from source

Building, the checks and the repository layout are described in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Contributing

[Issues](https://github.com/Cheviiot/Alcove/issues) ·
[CONTRIBUTING.md](CONTRIBUTING.md) ·
[Interface](interface.md) ·
[Chromium engine](chromium-engine.md) ·
[Threat model](threat-model.md) ·
[Changelog](CHANGELOG.md)

## Provenance

An independent continuation of [Spider](https://github.com/Zaedus/spider) from
commit `dcf9d1080ce2bbd89c342b4766a94e18aaecf660`, created by Zaedus with a
contribution from Cameron Radmore. Alcove's Git history starts fresh;
authorship of the inherited code is recorded in [AUTHORS.md](AUTHORS.md). The
original authors take no part in Alcove.

Licensed GPL-3.0-only — [NOTICE](NOTICE), [COPYING](../COPYING).
