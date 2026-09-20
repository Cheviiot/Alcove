# Native Chromium experiment

This is the optional native Chromium adapter experiment for Alcove.
It does not replace the released Electron add-on. The host is a Rust GTK4/libadwaita application.
The separate worker currently uses the official C++ CEF API (not `cef-rs`). Its
private, versioned transport makes the language binding replaceable without
changing the GTK window. The test harnesses use disposable Alcove applications
and profiles; the ordinary launch adapter is an explicit opt-in.

**Status: rendering works; native adapter integration is in progress.** See
[measured results and remaining gaps](RESULTS.md). Following the user's scope
clarification, CEF should mirror the existing WebView in a simple app window.
Both hosts now use `src/ui/shell/web_app_shell.rs` for the same header, content layout,
menu and autohide behavior. Broad browser audits are deferred rather than a
prerequisite for shared-shell and manager work; release readiness remains separate.

CEF is pinned to `152.0.7+g83ffcba+chromium-152.0.7977.83`. The SDK is downloaded
from CEF's official binary distribution into ignored `build/native-chromium/`
and verified against the upstream archive checksum. It is not vendored into
the repository or included in the main application.

## Build

All development packages and commands belong in the existing Fedora 44
Distrobox `alcove-dev`:

```sh
distrobox enter alcove-dev -- sudo dnf install -y --allowerasing --setopt=install_weak_deps=False cmake gcc-c++ weston nlohmann-json-devel libXcomposite libXdamage libXrandr libXtst mesa-libgbm-devel mesa-libEGL-devel mesa-libGLES-devel libdrm-devel gtk3 alsa-lib nss atk-devel at-spi2-core-devel at-spi2-atk-devel python3-pyatspi mutter orca xorg-x11-server-Xvfb xauth gettext fuse3 xdg-desktop-portal xdg-desktop-portal-gtk
distrobox enter alcove-dev -- bash engine/build.sh
```

For Orca, Fedora's minimal container may replace `systemd-standalone-tmpfiles`
with `systemd` to satisfy speech-dispatcher. The `--allowerasing` above applies
only inside `alcove-dev`; no host packages or services are changed.

The build also compiles the existing Russian catalog for the shared shell.
The isolated harness selects it through `ALCOVE_PROBE_LOCALEDIR` and its own
Russian locale; it does not change the desktop locale.

The runtime lives in `src/engines/native_chromium/`; the probe binary is a thin entry
point behind `native-chromium-probe`. Normal Alcove builds and `cargo run`
still select `alcove` and do not download or link CEF.

## Ordinary application launch

The optional `native-chromium` Cargo feature (Meson `-Dnative_chromium=true`)
enables the native adapter in the existing `alcove APP_ID` command. It is used
only when `ALCOVE_NATIVE_CHROMIUM_ADDON` points to the versioned `addon.json`
written by `build.sh`; otherwise the Chromium choice keeps using Electron.
The manifest verifies the CEF version, worker protocol and contained paths
before starting the add-on. The worker then confirms `app_launch: 1`.

The adapter uses the configured title, window size, maximized state and user
agent. A repeated launch presents the existing window. Site storage lives in
`profiles/<id>/chromium-cef`, separate from existing WebKit/Electron data;
there is no profile migration. Private IPC and frame files live in a temporary
directory under `XDG_RUNTIME_DIR`, removed after CEF shuts down. The repository
profile lock lasts through that shutdown. Ordinary windows do not execute
probe scripts, capture screenshots or record page console/events.

The isolated end-to-end launch check runs the actual `alcove APP_ID` path:

```sh
distrobox enter alcove-dev -- bash -c 'ALCOVE_LOCALEDIR="$PWD/build/native-chromium/locale" cargo build --locked --features native-chromium,ui-tests --bin alcove'
distrobox enter alcove-dev -- python3 engine/launch-audit.py
```

It exercises site interaction, F5, data persistence, independent app storage,
existing-window presentation, native network-error and renderer-crash retry,
worker-crash handling and reopening, cleanup and public Wikipedia in a private
Mutter session without XWayland. Reports contain only
the disposable fixture and public site. It does not touch installed launchers.

Saved permission decisions, session grants, navigation restrictions and proxy
settings now use the existing Alcove policy. The native choices match WebKit.
Run the ordinary-launch policy audit with `launch-audit.py --policy`.

The existing background policy is also supported. With background mode enabled,
closing hides the window; reopening presents the same page. The shared native
notification can show or stop it, and the window menu can stop it explicitly.
`--start-background` still requires the existing saved background/autostart
authorization. Website dialogs and engine errors reveal a hidden window when
they need attention. Run `launch-audit.py --background` to verify this in a
private Mutter session, including notification payloads and before-unload.

**Still experimental:** HTTP(S), SOCKS4 and SOCKS5 proxies are supported; other proxy schemes
fail explicitly. WebKit content filters remain inapplicable to Chromium, as
with the existing Electron adapter.
Theme-color customization, notification delivery, complete engine restart,
add-on installation/backup integration and Flatpak deployment remain work.

## Run safely

```sh
distrobox enter alcove-dev -- python3 engine/run.py --backend wayland
distrobox enter alcove-dev -- python3 engine/run.py --backend x11 --surface-only
distrobox enter alcove-dev -- python3 engine/run.py --backend wayland --gpu
distrobox enter alcove-dev -- python3 engine/run.py --backend wayland --gpu --layout --seconds 32
distrobox enter alcove-dev -- python3 engine/run.py --backend wayland --compositor weston --gpu --scale 2 --theme dark --surface-only
# Native accessibility, cross-page lifetimes and real Orca event handling:
distrobox enter alcove-dev -- python3 engine/run.py --backend wayland --compositor mutter --gpu --native-accessibility --accessibility-navigation --orca --seconds 28
# Shift+Tab, Tab, Enter through the private compositor while Orca reads the site:
distrobox enter alcove-dev -- python3 engine/run.py --backend wayland --compositor mutter --gpu --native-accessibility --orca --orca-keyboard --seconds 28
```

The harness starts a separate session bus, a loopback-only fixture HTTP server,
private XDG directories and headless Weston, headless Mutter, or Xvfb.
Mutter is the default for Wayland at scale 1; scale 2 defaults to Weston.
Mutter also gets a private RemoteDesktop session with virtual input devices;
it does not share the user's compositor or input seat.
It removes the desktop's display variables first. It never falls back to the
active desktop. Wayland runs do not start XWayland. The harness prints the run
directory under `build/native-chromium/runs/`.

Every run writes two GTK screenshots (hidden/revealed header), worker log,
JSONL events, `report.json` and an independent AT-SPI `accessibility.json`.
With `--orca`, it records Orca's actual speech-generation debug output, while
preventing connection to the user's speech service and auto-spawning one.
The installed Orca remains unmodified. Orca checks are serialized by a project
lock because its CLI detects peer processes across different session buses;
the harness never uses `orca --replace`. If the desktop already has Orca, its
CLI may refuse the isolated launch; that is reported as a failed diagnostic. The log checks demonstrate generated
speech, not audible output, Braille hardware or complete screen-reader coverage.
`diagnostic_passed` checks rendering, viewport preservation, F10 handler,
Unicode commit, WebGL and requested scaling/layout. Requested native
accessibility and Orca checks also participate in the pass/fail result.
Use Mutter for full fixture checks: the Weston headless backend lacks a keyboard
seat and active-window focus. Correct focus gating means the timed IM commit
cannot reach the inactive browser there. `--surface-only` explicitly limits
acceptance to transport/scaling/layout/header checks for Weston/Xvfb; its report
still records the input/WebGL observations. It never certifies input or accessibility. It is distinct from
`production_ready`. Xvfb may fail WebGL even when CPU page rendering works.
The process environment and CEF profile are isolated from real applications.
No `--no-sandbox` or renderer security bypass is used. A successful diagnostic
run is **not** production acceptance; `production_ready` remains false until
the adapter’s actual deployment requirements are demonstrated.

## Transport and limitations

- Control protocol version 4 uses per-view newline-delimited JSON over inherited pipes;
  the web renderer never receives the pipe. Input lines and frames are bounded.
- The CPU diagnostic path writes a complete BGRA frame through atomic file
  replacement in the private run directory. The GTK host validates its version,
  dimensions and length before making a `GdkMemoryTexture`.
- `--gpu` imports CEF's DMA-BUF into EGL and copies it on the GPU to a new GBM
  allocation. `glFinish` completes that copy before CEF may reuse its buffer.
  An inherited bounded Unix socket transfers the immutable allocation using
  `SCM_RIGHTS`; `GdkDmabufTextureBuilder` owns the receiving descriptors until
  its release callback. No CPU pixel readback is used for page presentation
  (saving diagnostic screenshots naturally reads pixels).
- This first GPU path intentionally uses one allocation and a blocking GPU
  synchronization per frame. It supports full-frame RGBA/BGRA input and
  single-plane linear ARGB output on the first DRM render node. Cropped frames
  and unsupported imports fail explicitly. Multi-GPU, modifiers across other
  drivers, buffer pooling and asynchronous fences still need validation.
- `--native-accessibility` uses Chromium's actual ATK objects, public ATK
  interface proxies, `AtkPlug` and `GtkAtSpiSocket` to place the document inside
  the GTK window's AT-SPI tree. Text, actions, focus, links and text events are
  exercised by an independent client. See [implementation and limits](ACCESSIBILITY.md).
  Without that flag, the original tree-only diagnostic remains available.
- Popups create real CEF browsers and native GTK child windows in one worker,
  with independent frames, focus and accessibility trees. Local OAuth mechanics,
  native JS/permission dialogs, file portals and downloads have dedicated audits.
  In-page CEF `PET_POPUP` surfaces use independent generation-checked GPU/CPU
  frames and a nonmeasuring GTK overlay. Notification delivery and remaining
  site features still need implementation/validation.
- The timed fixture check uses the GTK IM context commit signal to exercise
  Unicode delivery. This does not certify physical keyboard layout, IME
  composition, clipboard, or screen-reader operation.
- The `evaluate` command exists only in this experimental parent's private
  transport for deterministic fixture inspection. It is not a production API.
- The harness has no GNOME settings daemon. Its private GTK settings supply
  96 DPI; this avoids a Chromium 152 GTK4 startup crash in the missing-DPI
  fallback. No host settings are changed. CSS color-scheme is synchronized
  through the in-process CDP API; no remote debugging port is opened.

The native header overlays the site and hides after 1.5 seconds. Pointer,
keyboard focus, its menu and native dialogs hold it open. Dialogs use a hold
count so overlapping About/site/download surfaces cannot hide the panel early. F10 reveals
it and moves focus into the header. Shared shell and policy choices use Alcove's
gettext catalog. Remaining prototype-only labels still use Russian text.

## Checks

```sh
distrobox enter alcove-dev -- cargo test --locked --features native-chromium-probe --bin alcove-native-chromium
distrobox enter alcove-dev -- cargo clippy --locked --features native-chromium-probe --bin alcove-native-chromium -- -D warnings
```

The migration gate requires native Wayland rendering, GPU presentation, real
input and accessibility, popups/OAuth, portals, profile isolation and Flatpak
sandbox verification. Results and architectural blockers must be recorded
before replacing Electron or advancing the dependent manager redesign.

The experimental contract is documented in [PROTOCOL.md](PROTOCOL.md). It is
not yet a new production add-on ABI or a shared WebKit/Chromium adapter.

## Native site requests

`site_requests.h` owns CEF callbacks on its UI thread. The GTK host queues real
Adwaita dialogs, shows the requesting origin as plain text and returns responses
through the private pipe. Navigation cancels pending page requests; stale replies
are ignored. `file_portal.rs` calls the desktop FileChooser portal, retains the
exported parent and closes an active portal request when its page disappears.
Downloads use the SaveFile portal before CEF receives a destination, and appear
in the native Downloads dialog with progress and cancellation. Native AT-SPI
actions wait for real GTK site focus and are rejected behind modal surfaces. No permission is
automatically granted. Desktop capture still needs a source-selection portal.

```sh
distrobox enter alcove-dev -- python3 engine/run.py --gpu --native-accessibility --site-requests --seconds 75
```

This mode has its own fixture and uses external AT-SPI actions plus private
Mutter keyboard input to operate actual dialogs, including the installed GTK
file portal. It writes `site-requests.json`. It does not claim the ordinary
input/WebGL/Orca checks ran against this different fixture. Portal services and
their permission store use the same private bus and XDG directories.

The whole approved plan is tracked in [native-ui-plan-status.md](../../docs/native-ui-plan-status.md).

## Native popup / local OAuth diagnostic

```sh
distrobox enter alcove-dev -- python3 engine/run.py --gpu --popups --seconds 38
```

This opens real libadwaita child windows in one worker, with independent frame
routing and shared Chromium context. Compositor keyboard input drives a local
flow from `127.0.0.1` to `localhost` and back. The audit checks trusted activation,
window.opener, cookies, same-origin enforcement, postMessage with origin/source/
state validation, script close, and native before-unload cancel/confirm. No real
account or credentials are used. This is evidence for the mechanics of popup
OAuth, not compatibility with every external identity provider.

Add `--native-accessibility --orca` to verify the child/provider documents in
the actual GTK accessibility hierarchy, native window titles and Orca's speech
for the controls at all three login steps. The fixture allows time for queued
speech before each navigation. A same-window autofocus navigation need not
repeat the document title in Orca. CPU frame
transport can be checked by omitting `--gpu`.

The separate duplicate-document audit opens two windows with identical URL,
title and DOM, then checks independent actions, selections, a cross-origin
iframe, navigation, history and closure:

```sh
distrobox enter alcove-dev -- python3 engine/run.py --gpu --native-accessibility --accessibility-windows --orca --seconds 38
distrobox enter alcove-dev -- python3 engine/run.py --gpu --native-accessibility --popups --orca --seconds 45
```

This association is tied to the validated CEF release; native capability v3
rejects other runtime versions. See [the ordering contract](ACCESSIBILITY.md).

## cef-rs assessment (2026-09-20)

The current worker uses official C++ CEF; the GTK host uses Rust. The user asked
whether cef-rs already provides the missing adapters. Inspected
[tauri-apps/cef-rs at 4e566e6](https://github.com/tauri-apps/cef-rs/tree/4e566e628a361f3ade6f9a211cb33ea3c7a9153f):
its `osr_texture_import` module supplies DMA-BUF → Vulkan/wgpu import on Linux,
and its OSR example uses winit/wgpu. These are useful GPU building blocks.
No ready GTK4/libadwaita widget, portal adapter or native AT-SPI embedding layer
was found in that revision. Moving the worker to Rust remains possible but does
not by itself resolve these integration gates. No migration has been performed.

## In-page dropdowns and real websites

HTML select menus have their own CEF popup texture, bounds and lifetime. The
GTK overlay draws physical textures at logical coordinates and never changes
page size. Generation checks prevent delayed frames from reopening a hidden
menu. The native Chromium options supply AT-SPI names/actions. Its native option
action can leave the menu open; Escape dismisses it without reverting selection.

```sh
distrobox enter alcove-dev -- python3 engine/run.py --gpu --native-accessibility --dropdowns --seconds 40
distrobox enter alcove-dev -- python3 engine/run.py --compositor mutter --scale 2 --gpu --native-accessibility --dropdowns --seconds 40
distrobox enter alcove-dev -- python3 engine/run.py --compositor mutter --scale 2 --native-accessibility --dropdowns --seconds 40
```

For explicit Mutter scale 2, the harness sets its private virtual monitor to
2560×1800 with logical scale 2 through installed `gdctl`, rather than relying
on `GDK_SCALE`. RemoteDesktop pointer coordinates remain logical. The CEF
Wayland worker disables `WaylandFractionalScaleV1`: Chromium 152 otherwise
overrides OSR's supplied scale with 1 because it has no native window. GTK owns
the actual Wayland surface and supplies the buffer scale via `GetScreenInfo`.
This worker-only workaround retains native Wayland and the sandbox; it must be
revalidated on CEF upgrades. GTK fractional scaling is unaffected, but fractional
monitor and monitor-move tests remain outside the current evidence.
See Chromium's [per-window override](https://github.com/chromium/chromium/blob/152.0.7977.83/content/browser/renderer_host/render_widget_host_view_base.cc#L586)
and [Wayland runtime properties](https://github.com/chromium/chromium/blob/152.0.7977.83/ui/ozone/platform/wayland/ozone_platform_wayland.cc#L420).

The user also requires real-site checks. These use the same native GTK/CEF
binary and isolated Wayland session, not an external browser. Each run has a
fresh profile. The audit uses AT-SPI actions and actual private compositor input;
the optional JavaScript sampler only reads page state. It does not click, type,
start playback or bypass cross-origin frame restrictions. The regular fixture's
synthetic IM commit is disabled for external sites. Public pages are used without
accounts, submissions of personal data or desktop input access.

```sh
distrobox enter alcove-dev -- python3 engine/run.py --gpu --native-accessibility --url https://www.wikipedia.org/ --site-scenario wikipedia --seconds 65
distrobox enter alcove-dev -- python3 engine/run.py --gpu --native-accessibility --url https://developer.gnome.org/hig/ --site-scenario gnome --seconds 90
distrobox enter alcove-dev -- python3 engine/run.py --gpu --native-accessibility --url https://interactive-examples.mdn.mozilla.net/pages/tabbed/video.html --site-scenario video --seconds 90
distrobox enter alcove-dev -- python3 engine/run.py --gpu --native-accessibility --url https://get.webgl.org/ --site-scenario webgl --seconds 40
```

`--url URL` without a scenario checks initial document/rendering only.
`real-site.json` records checks, bounded independent AT-SPI snapshots, page
observations and failures; `site-N.png` captures the native window every six
seconds, up to 72 seconds. Network/site changes can fail these runs and are
reported, not silently replaced with local pages. They supplement deterministic
fixtures and do not establish full application/provider or codec compatibility.
