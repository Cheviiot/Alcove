<!-- SPDX-License-Identifier: GPL-3.0-only -->

<div align="center">
  <img src="data/icons/hicolor/scalable/apps/io.github.cheviiot.alcove.svg" width="128" alt="Alcove icon">
  <h1>Alcove</h1>
  <p><strong>Every website, its own application.</strong></p>
  <p>
    <a href="https://github.com/Cheviiot/alcove/actions/workflows/ci.yml"><img alt="Build" src="https://img.shields.io/github/actions/workflow/status/Cheviiot/alcove/ci.yml?branch=main&amp;style=flat-square&amp;label=build"></a>
    <a href="COPYING"><img alt="GPL-3.0-only license" src="https://img.shields.io/badge/license-GPL--3.0--only-6f7782?style=flat-square"></a>
    <img alt="GNOME" src="https://img.shields.io/badge/GTK%204-libadwaita-4a86cf?style=flat-square">
    <img alt="Flatpak" src="https://img.shields.io/badge/package-Flatpak-1c1d22?style=flat-square">
  </p>
  <p><a href="README.md">Русский</a> · <strong>English</strong></p>
</div>

Alcove turns a valid HTTP(S) address into a standalone GNOME desktop
application. Each site gets its own profile, cookies, cache, permissions and
settings. There is no shared browser shell and no session bleed between sites:
signing in to your work mail knows nothing about your personal one.

## Features

- **Isolation.** A separate profile, storage and permission set per site.
- **Works offline.** An application can be created even when its title, icon or
  the site itself is unreachable — the hostname stays editable by hand.
- **Per-site settings.** Navigation, proxy, background behavior, content
  filters and the user agent string.
- **Real-world flows.** OAuth windows, popups, notifications and downloads.
- **Backups.** Portable archives; archives containing website data are
  encrypted.
- **Least privilege.** System access goes through XDG portals only, without
  granting the application broad Flatpak permissions.

## Two engines

| Engine | Role |
| --- | --- |
| **WebKitGTK** | Native to GNOME, used by default. |
| **Chromium** | An add-on for sites that WebKitGTK cannot serve. |

Chromium ships as a separate Flatpak add-on and is **never installed
implicitly**. The engine choice appears only after the add-on is installed and
Alcove restarts. The engine is never switched without confirmation, and the two
engines never share profiles, cookies or authenticated sessions.

## Installation

The project is at version 0.1.0 and has no published releases yet. Building
from source is the working path:

```sh
git clone https://github.com/Cheviiot/alcove.git
cd alcove
flatpak-builder --disable-rofiles-fuse --user --install --force-clean \
  --install-deps-from=flathub .flatpak-build \
  build-aux/io.github.cheviiot.alcove.Devel.json
```

The first release will bring a signed Flatpak repository, reducing installation
to a single `alcove.flatpakref` command followed by ordinary `flatpak update`.
See [packaging/README.md](packaging/README.md) for how that repository works.

## Building and development

Dependencies live in the Fedora 44 Distrobox container `alcove-dev`, not on the
host system. Container creation and the full package list are in
[CONTRIBUTING.md](CONTRIBUTING.md).

```sh
distrobox enter alcove-dev -- cargo fmt --check
distrobox enter alcove-dev -- cargo clippy --locked --all-targets -- -D warnings
distrobox enter alcove-dev -- cargo test --locked
distrobox enter alcove-dev -- meson setup build -Dui_tests=true
distrobox enter alcove-dev -- meson test -C build --print-errorlogs
```

Interface checks run only in an isolated Xvfb or Wayland session with private
XDG directories, and never touch the person's installed applications. The
screen structure is documented in [docs/gnome-hig.md](docs/gnome-hig.md).

## Icon privacy

When a site serves no usable favicon, the icon can be requested through
[Icon Horse](https://icon.horse/). The request happens **only on an explicit
click**, nothing but the hostname leaves the machine, and the normalized PNG is
stored locally. With no network the provider is neither needed nor contacted.

## Compatibility boundaries

Not supported and not part of the project's promises: DRM and Widevine, browser
extensions, anti-bot circumvention, and proprietary browser APIs.

## Provenance and license

Alcove is an independent continuation of
[Spider](https://github.com/Zaedus/spider) as it existed at commit
`dcf9d1080ce2bbd89c342b4766a94e18aaecf660`. Spider was created primarily by
Zaedus, with a contribution by Cameron Radmore. Alcove's Git history starts
fresh and does not carry the original Spider commits — those remain in the
Spider repository, and authorship of the inherited code is recorded in
[AUTHORS.md](AUTHORS.md). The previous authors do not participate in Alcove and
have not endorsed it.

Licensed under GPL-3.0-only. See [NOTICE](NOTICE), [AUTHORS.md](AUTHORS.md) and
[COPYING](COPYING).

## More

[Contributing](CONTRIBUTING.md) ·
[Interface](docs/gnome-hig.md) ·
[Threat model](docs/threat-model.md) ·
[Portal compatibility](docs/portal-compatibility.md) ·
[Changelog](CHANGELOG.md)
