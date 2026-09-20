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
   meson setup build
   meson compile -C build
   meson test -C build
   ```

5. Open a pull request. Changes are squash-merged after required checks pass.

## Development environment

Project-specific packages belong in the Fedora 44 Distrobox named
`alcove-dev`, not on the ALT Workstation host:

```sh
distrobox create --name alcove-dev --image registry.fedoraproject.org/fedora:44 --yes
distrobox enter alcove-dev -- sudo dnf install -y rust cargo rustfmt clippy cargo-deny gcc pkgconf-pkg-config meson ninja-build blueprint-compiler gtk4-devel libadwaita-devel webkitgtk6.0-devel openssl-devel appstream desktop-file-utils flatpak-builder gettext glib2-devel librsvg2-tools xorg-x11-server-Xvfb dbus-daemon git nodejs python3-aiohttp python3-pyyaml python3-tomlkit
```

Build the development Flatpak without FUSE-backed rofiles:

```sh
distrobox enter alcove-dev -- flatpak-builder --disable-rofiles-fuse --user --install --force-clean --install-deps-from=flathub .flatpak-build build-aux/io.github.cheviiot.alcove.Devel.json
```

The interface structure and UI checks are documented in [GNOME interface](docs/gnome-hig.md).

Run production GUI checks on a virtual X11 display. The isolated native
Chromium experiment also provides its own headless Wayland compositor; see
[the experiment instructions](experiments/native-chromium/README.md).
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
