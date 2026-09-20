<!-- SPDX-License-Identifier: GPL-3.0-only -->

<div align="center">
  <img src="data/icons/hicolor/scalable/apps/io.github.cheviiot.alcove.svg" width="128" alt="Alcove icon">
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

## Why it exists

A browser shares everything. Your work mail knows about your personal one
because the cookies sit in the same jar. Signing in to one service drags a
profile into the next. A notification permission granted to one site looks
exactly like the other forty tabs. And the tab you actually need gets lost
among the rest, then closes with the browser.

Alcove splits that apart. Each site gets its own window, profile, cookies,
cache and permissions. Two accounts on the same service live side by side and
know nothing about each other. The application launches from the GNOME menu
like any other, whether or not a browser is running.

## What you get

- **Two accounts on one service at once** — no private mode, no second
  browser.
- **Work apart from personal** — the sessions cannot cross, rather than
  relying on your discipline.
- **A site as a program** — its own icon in the menu and its own slot in the
  window switcher.
- **Permissions where they belong** — camera, microphone, notifications and
  location are granted to one site, not to a whole browser.
- **Data you control** — a profile can be exported to an archive, carried to
  another machine, or deleted together with its application.

## How it works

Alcove is a GNOME application built with GTK 4 and libadwaita. An embedded
engine renders the site, while the window, menus and navigation stay native to
the desktop.

| Engine | Role |
| --- | --- |
| **WebKitGTK** | Native to GNOME, used by default. |
| **Chromium** | An add-on for sites that WebKitGTK cannot serve. |

Chromium installs as a separate Flatpak add-on and **never appears on its
own**. The engine choice exists only once the add-on is installed, the engine
is never switched without confirmation, and the two engines never share
profiles or authenticated sessions.

Every application is configured separately: navigation rules, proxy,
background behavior, content filters and the user agent string. System access
goes through XDG portals only — Alcove asks for no broad Flatpak permissions
and reads no other application's directories.

## Installation

The project is at version 0.1.0 and has no published releases yet. The first
release will bring a signed Flatpak repository and a one-command install;
until then the application is built from source, as described in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Privacy

Alcove collects no analytics, contacts no third-party service at startup, and
never sends out the addresses you open.

The one optional outbound request is an icon lookup through
[Icon Horse](https://icon.horse/) when a site serves no usable favicon. It
happens **only on an explicit click**, nothing but the hostname leaves the
machine, and the result is stored locally. With no network the provider is
neither needed nor contacted.

Archives that carry website data are encrypted: a profile with its cookies and
storage cannot be exported without a passphrase.

## What Alcove does not do

Not supported and not part of the project's promises: DRM and Widevine,
browser extensions, anti-bot circumvention, and proprietary browser APIs.
Alcove gives a site its own place on the desktop; it does not replace a
browser.

## Contributing

Bug reports and proposals go to
[Issues](https://github.com/Cheviiot/Alcove/issues). The development
environment, build, checks and coding rules are described in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Provenance and license

Alcove is an independent continuation of
[Spider](https://github.com/Zaedus/spider) as it existed at commit
`dcf9d1080ce2bbd89c342b4766a94e18aaecf660`. Spider was created primarily by
Zaedus, with a contribution by Cameron Radmore. Alcove's Git history starts
fresh and does not carry the original Spider commits — those remain in the
Spider repository, and authorship of the inherited code is recorded in
[AUTHORS.md](AUTHORS.md). The previous authors do not participate in Alcove
and have not endorsed it.

Licensed under GPL-3.0-only. See [NOTICE](NOTICE), [AUTHORS.md](AUTHORS.md)
and [COPYING](COPYING).

## More

[Interface](docs/gnome-hig.md) ·
[Threat model](docs/threat-model.md) ·
[Portal compatibility](docs/portal-compatibility.md) ·
[Flatpak repository](packaging/README.md) ·
[Changelog](CHANGELOG.md)
