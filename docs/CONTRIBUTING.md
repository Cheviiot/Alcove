# Contributing to Alcove

Thank you for helping build Alcove. Please search existing issues before filing
a report and keep each pull request focused on one change.

## Workflow

1. Create a branch from `main`.
2. Work inside the documented `alcove-dev` Fedora 44 Distrobox.
3. Add tests for behavior changes.
4. Run the local gate:

   ```sh
   cargo fmt --check
   cargo check
   cargo clippy --all-targets -- -D warnings
   cargo test
   meson setup build/meson
   meson compile -C build/meson
   meson test -C build/meson
   ```


5. Open a pull request. Changes are squash-merged after required checks pass.

## Development environment

Alcove needs GTK 4, libadwaita and WebKitGTK 6 development packages. Any
distribution that ships them will do; the reference environment is a Fedora 44
container, which keeps these packages off the host system:

```sh
distrobox create --name alcove-dev --image registry.fedoraproject.org/fedora:44 --yes
distrobox enter alcove-dev -- sudo dnf install -y rust cargo rustfmt clippy cargo-deny gcc pkgconf-pkg-config meson ninja-build blueprint-compiler gtk4-devel libadwaita-devel webkitgtk6.0-devel openssl-devel appstream desktop-file-utils flatpak-builder gettext glib2-devel librsvg2-tools xorg-x11-server-Xvfb dbus-daemon git nodejs python3-aiohttp python3-pyyaml python3-tomlkit
```

Build the development Flatpak without FUSE-backed rofiles:

```sh
distrobox enter alcove-dev -- flatpak-builder --disable-rofiles-fuse --user --install --force-clean --install-deps-from=flathub --state-dir build/flatpak build/flatpak-app packaging/flatpak/io.github.cheviiot.alcove.Devel.json
```

## Repository layout

Every directory holds one subject, and a directory that grows past a handful of
files is split into subsystems rather than flattened.

| Directory | Contents |
| --- | --- |
| `src/` | All source: the `alcove` Rust crate with its two binaries, and the CEF worker in C++ under `cpp/`. |
| `data/` | Desktop entry, AppStream metadata, GSettings schema, icons and the translation catalogs under `po/`. |
| `docs/` | Project documents and the interface, protocol, portal and threat-model references. |
| `packaging/` | How Alcove is built and published: `flatpak/` manifests, `scripts/` that Meson and the engine build call, and `repo/`, the signed Flatpak repository with its landing page. |
| `tests/` | Reproducible interface and engine audit harnesses. |

The repository root keeps only what build tools insist on finding there:
`Cargo.toml`, `Cargo.lock`, `meson.build`, `meson_options.txt`, `COPYING` and
`README.md`, plus the hidden `.cargo/` (which also holds `deny.toml`),
`.rustfmt.toml`, `.gitignore` and `.github/`. Every build artifact lands under
a single ignored `build/`: `build/meson` is the Meson build directory,
`build/cargo` is Cargo's target directory, `build/flatpak` is flatpak-builder's
state, and the audit harnesses write their runs beside them.

`src/` is organised by layer, and each layer may depend only on the ones above
it:

| Directory | Contents |
| --- | --- |
| `domain/` | Application and policy models, the on-disk repository and website metadata. No GTK, and no call into another layer. |
| `system/` | Desktop integration: XDG portals, launcher, background permission, archives, and the service layer that ties storage to the system. |
| `engines/` | Which engine is available and suitable, content filters, and under `chromium/` the add-on discovery and the worker's wire protocol. No widgets. |
| `ui/` | GTK 4 and libadwaita. Everything with a widget lives here. |
| `app/` | The GTK application object, its global actions, and the glue that opens a window on either engine. |

`ui/shell/` is the runtime window of a web app: `web_app_shell.rs` is the chrome
both engines share, `webkit/` is the WebKitGTK view, and `chromium/` hosts the
CEF worker's output, itself split into `worker/` (the child process and its
frames), `widgets/` (the parts of the window) and `site/` (what the website asks
for).

Everything lives in the `alcove` library crate; `src/main.rs` is a thin entry
point, and the optional probe binary reuses the same modules. A `mod.rs` only
declares modules — implementations live in named files, and no file shares its
name with the directory beside it. Blueprint files sit next to the widgets they
describe, and code built only for the `ui-tests` feature lives in a `ui_test.rs`
beside the module it drives.

The interface structure and UI checks are documented in [GNOME interface](gnome-hig.md).

Run production GUI checks on a virtual X11 display. The engine audits provide
their own headless Wayland compositor; see
[the engine audit instructions](../tests/engine/README.md).
Never allow a GUI check to fall back to the active desktop session:

```sh
distrobox enter alcove-dev -- dbus-run-session -- env -u WAYLAND_DISPLAY xvfb-run -a timeout 10s flatpak run --nosocket=wayland --socket=x11 --env=GDK_BACKEND=x11 io.github.cheviiot.alcove
```

Use English for code, identifiers, and technical documentation. User-facing
strings must be translatable; update the Russian catalog when adding UI text.

Commit messages are bilingual. Write the subject line in English, following
Conventional Commits (`feat:`, `fix:`, `docs:`). Write the body in Russian
first, then repeat the same explanation in English below a `--` separator.

Do not add broad Flatpak permissions, another browser engine or runtime, WebKit
patches, Chromium user-agent shims, or access to another application's sandbox.
Discuss any permission change in an issue first.

By contributing, you agree that your contribution is provided under
`GPL-3.0-only`.
