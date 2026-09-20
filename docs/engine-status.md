# Chromium engine status

Alcove renders websites with WebKitGTK by default. Chromium is available for
sites WebKitGTK cannot serve. Both engines share one GTK 4 window built by
`src/ui/shell/web_app_shell.rs`: the same header, navigation, menu and
auto-hiding toolbar. The website is drawn by the engine; everything around it
belongs to the application.

## How the Chromium engine works

A CEF worker process renders the page and hands frames to the host, which
presents them inside the application's own window. The worker and the host
speak a private, versioned transport, so the language binding can be replaced
without touching the window. The Chromium sandbox is kept: the worker starts
through `zypak`, which maps it onto Flatpak's portals.

| Piece | Where |
| --- | --- |
| Worker (C++, CEF) | `engine/` |
| Host and adapter (Rust) | `src/engines/native_chromium/`, `src/engines/native_chromium_launch.rs` |
| Transport | [engine protocol](engine-protocol.md) |
| Audits | `tests/engine/` |

The engine ships as the Flatpak extension
`io.github.cheviiot.alcove.ChromiumNative`, mounted at
`/app/extensions/chromium-native`. It is installed separately, never appears
on its own, and is removed together with the application.

## Verified

- Renders real websites inside a native GTK window under Flatpak, with the
  extension mounted and the Chromium sandbox retained through zypak.
- Native Wayland without XWayland; GPU frame transport through EGL/GBM and
  `SCM_RIGHTS`; resize, scale, maximize and fullscreen.
- Header auto-hide, preserved viewport, zoom, history and the shared menu.
- Accessibility: website objects exposed inside GTK, text and actions usable
  by an independent AT-SPI client, and an unmodified Orca reading documents
  and input.
- Site requests: native dialogs and permissions, real file portals, downloads,
  navigation cancellation and modal action blocking.
- Saved permissions, navigation allowlist, proxy settings and background mode
  come from the same policy as WebKitGTK.
- Persistent per-application storage separate from WebKitGTK, restart, repeat
  launch, and recovery from network errors and renderer crashes.

## Not verified

- **Notification delivery.** Permission choices are honoured; delivering a
  notification and acting on it is untested.
- **Performance and media.** No scroll, animation, WebGL or video benchmarks.
- **Worker restart in place.** Reopening after a worker crash works; restarting
  it without closing the window does not exist yet.
- **Architectures.** The worker is built for `x86_64` only. On `aarch64`
  Alcove ships with WebKitGTK alone.
- **Screen reader breadth.** Beyond the interfaces and workflows listed above.

## Reproducing

Audit scripts, their exact runs and recorded limitations live in
[`tests/engine/README.md`](../tests/engine/README.md) and
[`tests/engine/RESULTS.md`](../tests/engine/RESULTS.md). They run in private
Wayland or Xvfb sessions with isolated XDG directories and never touch the
person's installed applications.
