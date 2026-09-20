# Native Chromium gate — 2026-09-20

**Decision: keep Electron while integrating the native adapter.**
The user clarified that CEF should match the current WebView in a simple app
window. Shared shell and product integration now take priority; the broader
browser audits below are recorded limitations, not a block on all UI work.
The prototype demonstrates real GTK4/libadwaita presentation of Chromium in
native Wayland, including GPU transport. It does not meet the complete adoption
criteria. The native accessibility adapter now exposes real website objects
inside GTK and supports independently tested text/actions/navigation. Orca
reads the document and input in an isolated Mutter session. Full website,
input, notification and sandbox acceptance remains incomplete. Multiple native
windows and local popup/OAuth flows now have dedicated accessibility checks.

Environment: Fedora 44 Distrobox `alcove-dev`, GTK 4.22.5, libadwaita 1.9.4,
Weston 15 headless GL / Mutter 50.5 headless, Orca 50.2, CEF 152.0.7 / Chromium 152.0.7977.83, Linux x86_64.
The GTK/CEF/session-bus runtimes and data directories are private. Weston is
started without XWayland; GTK uses `GdkWaylandDisplay`, CEF uses Ozone Wayland.
The Chromium sandbox was not disabled. This is not a Flatpak sandbox test.

## Evidence

| Test | Observed result |
| --- | --- |
| Wayland CPU diagnostic | 344 CPU frame events in 12 seconds; site, animation and WebGL rendered |
| Wayland GPU, 32 seconds | 945 GTK texture assignments, zero CPU frames; no protocol/render errors |
| GPU ownership | After warm-up, host descriptors remain 28–29 and worker descriptors 168 during the 32-second run; this is a short leak check, not a long-term guarantee |
| Resize | 800×640 → 360×640 → 800×640, live GPU frames |
| Maximize/restore | 1280×868 maximized; restored 800×640 |
| Fullscreen/restore | 1280×900 fullscreen; restored 800×640 |
| Scale 200% | 1600×1280 CEF frame for 800×640 logical content |
| Header | Hidden and revealed screenshots; 1.5-second timer; F10 handler returns focus to header; page viewport remains unchanged |
| Unicode | GTK IM commit `Привет` reaches the page input and is confirmed by the fixture |
| AT-SPI native mode | Document, entry, button and link appear inside the GTK window. External client reads `Привет`, sets caret 2, selects [1,4], and activates a trusted browser button action |
| AT-SPI navigation | Link action loads the next document and returns; retired document is defunct and its button action is rejected |
| Orca | Unmodified Orca selects its Chromium script, reads document/input/Russian text and returns to the native menu; speech generation is recorded without audio output |
| Xvfb CPU | 702 CPU frame events and working Unicode/header checks; WebGL is blocklisted because Xvfb has no DRI3; diagnostic therefore fails the complete WebGL check |
| Initial application baseline | 104 application tests, 4 Meson checks (including UI smoke), all-target Clippy and production Meson build passed before the native accessibility work; production code was not changed in this continuation |
| Initial accessibility code checks | CEF worker build, 4 probe tests, probe Clippy with warnings denied, Rust formatting and Python syntax checks pass; later iterations have 5 probe tests |
| Mutter + Orca navigation, 32 seconds | 885 GPU texture assignments, zero CPU frames; AT-SPI text/actions, cross-page replacement/stale-action rejection and Orca document/input speech checks pass |
| Mutter + Orca keyboard, 32 seconds | 950 GPU texture assignments, zero CPU frames; Shift+Tab → Tab → Enter activates the button, with a second trusted click; all requested fixture checks pass |

Raw local runs are under `build/native-chromium/runs/`:

- `20260920-004814-wayland-429048`: CPU Wayland.
- `20260920-004814-wayland-429054`: GPU layout and 32-second resource check.
- `20260920-004814-wayland-429051`: GPU 200% scale.
- `20260920-004628-x11-425457`: Xvfb with the WebGL limitation.
- `20260920-005316-wayland-436830`: original dark GPU run, 200% scale, 351 texture assignments.
- `20260920-063256-wayland-709913`: final Mutter/Orca navigation checks, all requested checks pass.
- `20260920-063329-wayland-710781`: final Mutter/Orca compositor keyboard checks, all requested checks pass.
- `20260920-063402-wayland-711832`: strict Weston full-fixture attempt; surface/scale/WebGL pass, IM commit fails because the corrected focus gate rejects input into an inactive window.
- `20260920-063402-x11-711816`: strict Xvfb full-fixture attempt; surface works, WebGL remains unavailable.
- `20260920-063655-wayland-715004`: explicit surface-only check, dark theme at 200%; passes.
- `20260920-063656-x11-715007`: explicit Xvfb surface-only check; passes, WebGL remains false.

Each run contains logs, report, screenshots and an independent accessibility
audit. A compact, source-controlled evidence snapshot is in `results.json`.
Reproduce using the commands in [README.md](README.md). Frame counters above
are transport observations, **not** measured display FPS or Electron benchmarks.

## Accessibility architecture finding

The original tree-only diagnostic could not expose a usable website to AT-SPI.
CEF `SetAccessibilityState(STATE_ENABLED)` restricts a windowless browser to
WebContentsOnly. `STATE_DEFAULT` preserves the explicit
`--force-renderer-accessibility=complete` mode, in which Chromium creates
native ATK objects, including real semantic actions. See the
[CEF implementation](https://github.com/chromiumembedded/cef/blob/83ffcba/libcef/browser/browser_platform_delegate.cc).

The worker supplies the missing OSR parent hierarchy through public ATK
proxies and `AtkPlug`. The Rust host uses the Linux-specific public GTK API
[`GtkAtSpiSocket`](https://github.com/GNOME/gtk/blob/4.22.5/gtk/a11y/gtkatspisocket.h)
(since 4.14; not introspected in Gtk.gir). The document is embedded in the
actual GTK window tree, rather than appearing as a separate visible app.
Changing language bindings is not required for this bridge.

Native/proxy event duplication, stale actions across navigation, focus replay
at startup and focus-before-caret ordering were found and corrected through
independent AT-SPI/Orca runs. See [ACCESSIBILITY.md](ACCESSIBILITY.md) for the
implemented surface and remaining limits. This evidence supersedes the initial
finding that no semantic action path was available.

## Remaining acceptance work

| Area | Status / required work |
| --- | --- |
| Native GNOME | Isolated Mutter 50.5, native Wayland and virtual compositor input now run; a full GNOME Shell session and real devices remain unverified |
| Input | Physical Russian layout, composition/candidate positioning, clipboard, selection, drag/drop and complete keyboard focus traversal remain unverified |
| Rendering | Basic external WebM playback and integer 200% popup scaling pass; fractional scale, different GPU drivers/multi-GPU, broader codecs/DRM and extended GPU resource stress remain unverified |
| Performance | Run equivalent scroll/animation/WebGL/video scenarios against the existing Electron implementation; per-frame GPU allocation and `glFinish` are deliberately conservative |
| Site features | Native requests, local popup/OAuth and in-page PET_POPUP dropdowns pass dedicated audits, including native accessibility. Notification delivery and desktop capture remain open |
| Reliability | Separate output/profile roots are enforced; process exit shows a native error page. Restart/recovery, crash injection and Flatpak sandbox integration still need implementation/tests |
| Accessibility | Native text/actions and multi-window routing work, including a cross-origin iframe action. Full table/selection/range interfaces, rich editors, magnification, Braille and extended Orca workflows remain unverified; high contrast and enlarged text also need testing |
| Compatibility | Production launch commands, engine choice and existing profiles are untouched. Electron-to-CEF profile compatibility/migration is not established |

## Harness and input findings

- Orca's CLI detects other Orca processes even on separate session buses. Two
  simultaneous checks refused to start (`20260920-063106-*`), correctly failing
  their speech checks. A project lock now serializes Orca runs. No `--replace`
  option is used and no existing desktop screen reader is stopped.
- The default Wayland fixture now uses Mutter at scale 1, with private virtual
  input. Weston without an input seat remains useful for surface/scale tests.
  `--surface-only` is an explicit narrower scope; it retains input/WebGL
  observations and does not certify them. Full-fixture failures are preserved.
- Plain Return requires both CEF raw-keydown and a character event to activate
  buttons. The compositor keyboard test found this missing step; it is fixed,
  along with modifier virtual-key codes. Physical Russian layout and complete
  IME behavior still require separate acceptance tests.

## Startup findings

- `CefExecuteProcess` initializes resources before `CefSettings`: ICU, PAKs,
  V8 snapshot and runtime libraries must be beside the worker/libcef.
- Without `--no-first-run`, CEF blocked in Chromium's first-run EULA dialog.
- In a headless private session with unset Xft DPI, Chromium 152 GTK4 falls
  through to the removed GTK3 `gdk_screen_get_default` function and crashes.
  The harness supplies 96 DPI in its own GTK settings; the host is unchanged.
- The session bus must inherit the private runtime before activation, otherwise
  accessibility services can reuse the desktop runtime. The harness now isolates
  the bus, AT-SPI, display variables and XDG paths together.
- CEF's GPU buffer is reusable as soon as `OnAcceleratedPaint` returns. The
  prototype copies into owned GPU storage and completes the copy before return;
  merely duplicating CEF's FD would not protect the pixels from reuse.

## Native website requests — 2026-09-20

Run `20260920-114229-wayland-1039121` passes all **22** request checks and the
host rendering, scale, header hold and overlay checks. The audit uses actual
GTK/CEF AT-SPI objects, private Mutter keyboard/pointer events and the installed
`xdg-desktop-portal-gtk`, not fabricated portal responses.

- F10 and keyboard menu opening; semantically named native menu actions.
- Alert, confirm accept/reject, prompt text round-trip.
- Notification denial, geolocation grant, multiple-download permission.
- Synthetic microphone grant and camera denial (no physical recording devices).
- File portal upload, user cancellation and actual bytes read by the page.
- Save portal download, content verification, save cancellation, and cancellation
  of an in-progress network download from the native Downloads dialog.
- Before-unload stay/leave decisions.
- Navigation dismisses both a pending permission and an open file portal.
- Native menus/dialogs prevent deferred website AT-SPI actions behind them.

`native-dialog.png` was visually inspected: the actual Adwaita dialog shows the
origin and literal page message over the dimmed site, while the native header
remains visible. `production_ready` is still false. Geolocation permission was
verified; geolocation service/hardware availability was not. Notification delivery
and screen-capture portal selection remain separate unfinished capabilities.

That request iteration introduced native AT-SPI capability **v2**: actions and component focus wait
for an acknowledgement of real GTK site focus. This fixes the transition from
header/download-dialog focus to a web action such as requesting the clipboard.
`20260920-114229-wayland-1039118` repeats the real Orca, text/selection/action and
compositor keyboard checks successfully after that change.
`20260920-114229-x11-1039124` passes the separate Xvfb surface checks; its known
WebGL limitation remains explicit.

Integration tests also found and fixed a null-path cancellation error, missing
download names and a disabled-button accessibility issue. Generated GTK model
menu items exposed empty names in this runtime; public GtkPopoverMenu custom
slots with labelled GTK buttons preserve native rendering and menu behavior.
The test bus must inherit `NO_AT_BRIDGE=0` before service activation so that the
GTK3 portal participates in accessibility. Failed development runs are retained
in the ignored run directory; only the final runs above represent acceptance.

## Native popup transport and local OAuth — 2026-09-20

This iteration introduced control/frame protocol **v2**, requiring per-view routing. A single CEF
worker creates genuine popup browsers and native libadwaita child windows reuse
the main shell/input/site-request components. One GPU bridge owns the shared
EGL/GBM resources; GTK keeps textures and request queues separate per window.
CPU frames likewise use a separate directory per child view.

GPU run `20260920-121017-wayland-1089930` and CPU diagnostic run
`20260920-120752-wayland-1084205` pass **11 popup checks**. The local flow covers:
trusted compositor Enter activation; a returned WindowProxy; shared cookie and
opener; cross-origin document access denial at a localhost provider; redirect
back to 127.0.0.1 with postMessage validated by origin/source/state; script close;
native before-unload stay/leave; independently rendered child windows; one
worker PID; and closure of both child windows. The GPU callback page screenshot
was visually inspected and contains the expected green confirmation page.

This checks OAuth popup mechanics with local fixtures. It does not certify
compatibility with external providers or their embedded-browser policies.

At that transport iteration, native mode rejected popups because public ATK
geometry queries did not expose a reliable browser association. The runs above
therefore exclude website screenreader acceptance. The later native capability
v3 below resolves that association using ordered load events, with an exact
CEF runtime pin. No Electron replacement is approved.

Protocol-v2 regressions passed:

- `20260920-120241-wayland-1067896`: actual Orca speech, native web text/actions,
  compositor Shift+Tab/Tab/Enter and WebGL.
- `20260920-120537-wayland-1074473`: all 22 native website-request checks,
  including real portals, dialog focus protection and before-unload.
- `20260920-120539-x11-1075059`: separate Xvfb CPU surface/scaling/header checks;
  WebGL remains unavailable and is not claimed by that test scope.

CEF/C++ and Rust builds, five probe unit tests, warning-free probe Clippy, Rust
formatting and Python syntax checks passed. Normal Alcove application tests are
still the earlier baseline; this iteration changed the isolated experiment.

## Independent native accessibility for multiple windows — 2026-09-20

Native capability **v3** now associates each Chromium root with its CEF view
through ordered root-load callbacks and public ATK signals. Each GTK window
has its own socket, document generation, focus and retired proxies. Native mode
checks the exact validated CEF runtime; this ordering is version dependent.
The source-backed contract is in [ACCESSIBILITY.md](ACCESSIBILITY.md).

Final runs, all passing their requested scope:

| Run | Evidence |
| --- | --- |
| `20260920-125926-wayland-1203857` | **15** identical-document checks: same URL/title in two distinct GTK windows, independent actions/selections, cross-origin iframe action, inactive-window rejection, child-only navigation/history retirement, close and surviving parent; actual Orca speech |
| `20260920-125840-wayland-1201229` | **14** popup/OAuth checks, including native child/provider documents and native window titles; Orca identifies the windows and speaks website buttons at all three login steps; trusted compositor Enter completes the flow |
| `20260920-124935-wayland-1173695` | Single-document navigation, return, text/selection/action and stale-reference rejection; Unicode/WebGL/header checks |
| `20260920-124934-wayland-1173686` | All **22** website-request checks, including installed file portals and modal focus protection |
| `20260920-125453-wayland-1191150` | Orca document/input speech and actual Shift+Tab/Tab/Enter regression |
| `20260920-124617-x11-1165391` | Separate Xvfb CPU surface check; no WebGL/accessibility claim in this scope |

The native runs have no ATK state/unref or weak-reference lifecycle warnings.
The harness now makes those warnings fail the diagnostic. Debugging found two
adapter errors: AtkPlug's default state query returned NULL for the custom child
hierarchy, and a native DEFUNCT event could be sent again by navigation cleanup.
The plug now supplies a valid state set even between pages; invalidation is
idempotent and rejects subsequent actions/events.

Page titles now update both the native header and the actual GTK window title.
The OAuth speech audit allows queued speech to finish before navigation and
reads Orca's flushed log. It checks the real control names on every page; Orca
can announce an autofocus control without repeating a same-window navigation's
document title. The independent AT-SPI audit still requires the provider's
document and native window title. The final child screenshot was inspected and
shows the expected confirmation page.

CEF/Rust builds, **5** probe tests, probe Clippy with warnings denied, Rust
formatting and Python syntax checks pass. `results.json` retains older runs with
their original limited scopes and adds this iteration separately. The full plan
and Electron replacement remain unaccepted. At that point in-page select
surfaces were still open; the following iteration covers them. Input/IME/
clipboard, broader accessibility, media/performance, notifications, crash
recovery and Flatpak remain separate acceptance work.

## In-page popup surfaces and public websites — 2026-09-20

Experimental protocol **v3**, surfaces capability **v1**, separates the main
page from CEF `PET_POPUP` frames. Both the GPU and CPU paths carry per-view
surface generations. GTK snapshots popup textures at logical bounds, without
adding layout size or duplicating accessible controls. Delayed or retired
frames cannot resurrect a closed menu. Native AT-SPI capability remains v3.

| Run | Evidence |
| --- | --- |
| `20260920-132742-wayland-1256939` | GPU at 100%: all **11** dropdown checks |
| `20260920-133218-wayland-1269869` | GPU at actual Mutter 200%: all **11** dropdown checks; 1600×1280 page and 480×216 popup pixels at 800×640 and 240×108 logical sizes |
| `20260920-133219-wayland-1269874` | CPU diagnostic at actual Mutter 200%: same **11** dropdown checks |
| `20260920-133219-wayland-1269877` | Protocol-v3 regression: **15** independent-window accessibility checks and actual Orca speech |
| `20260920-133529-wayland-1282748` | All **22** native site-request/real portal checks, including modal focus protection and navigation cancellation |
| `20260920-133345-x11-1274785` | Separate Xvfb CPU surface/header regression; no WebGL or input acceptance claim |

The dropdown checks include native names, mouse opening and selection,
keyboard selection, reopening with a new generation, upward placement near the
lower edge, Escape, native option action and unchanged page dimensions. Chromium's
native action selects an option without necessarily closing the menu; a further
Escape closes it and preserves the choice. The 200% edge screenshot was visually
inspected. Six probe unit tests, probe Clippy with warnings denied, Rust
formatting and Python syntax checks pass.
The first concurrent 50-second request run reached 19 checks before the timed
host closed; the complete 90-second run above passed all 22. The incomplete run
is retained and is not counted as acceptance.

Mutter exposed a scaling problem hidden by Weston: Chromium's per-window scale
override uses 1 for a windowless OSR view even when `GetScreenInfo` returns 2.
Disabling `WaylandFractionalScaleV1` only in the CEF worker restores GTK-provided
scale. This is a version-dependent workaround, not fractional-monitor acceptance;
the actual GTK surface remains native Wayland. A headless Ozone trial did not
produce GPU frames and was not adopted. Failed runs are retained in the ignored
run directory. The harness now configures the private Mutter monitor through
`gdctl`; environment scaling alone was not sufficient.

The user's requested external-site checks use the actual GTK/CEF executable,
private Mutter input, fresh profiles and independent AT-SPI. A read-only page
sampler and native-window screenshots record results; no injected script performs
the tested actions. No accounts or existing profiles were used.

| Public page / run | Verified behavior |
| --- | --- |
| [Wikipedia](https://www.wikipedia.org/), `20260920-133343-wayland-1274167` | **8** checks: accessible page/search, actual keyboard entry of GNOME, Enter navigation to the article, PageDown and native Back to the starting site; no main-load errors |
| [GNOME HIG](https://developer.gnome.org/hig/), `20260920-132408-wayland-1246371` | **7** checks: accessible link to Design Principles, page navigation, keyboard scroll and native Back |
| [MDN video example](https://interactive-examples.mdn.mozilla.net/pages/tabbed/video.html), `20260920-132408-wayland-1246384` | **5** checks: native accessible Play action starts the external 960×540 WebM; observed 4.096 s, 128 decoded frames, 0 reported dropped frames and no media error |
| [Khronos WebGL check](https://get.webgl.org/), `20260920-132829-wayland-1259429` | **6** checks: live site reports WebGL support and renders its canvas; independent comparison of `site-12.png` and `site-18.png` finds 3503 changed pixels confined to the cube region `[343,222,464,357]` |

Wikipedia revealed a real integration bug: its accessibility batch reached
**6,593,023 bytes**, exceeding the host's 4 MiB event limit. The old reader silently
stopped, leaving a rendered but disconnected page. Native accessibility now
keeps full trees inside CEF and exports semantic objects through AT-SPI; the
control channel gets small metadata only. Tree-only diagnostics omit dumps above
3 MiB. Oversized/truncated control events now fail explicitly. In the final
Wikipedia run the largest control record is 2036 bytes and navigation/scroll/back
all work. Native-header discovery uses bounded breadth-first traversal so long
articles cannot hide the native controls behind thousands of content nodes.

The video and WebGL screenshots were visually inspected. This is basic public
site compatibility evidence, not a frame-rate benchmark, DRM/codec certification,
identity-provider approval or broad web-app acceptance. The required equivalent
Electron performance comparison, real IME/clipboard, notification delivery,
recovery and Flatpak gates remain open. Electron and user profiles are unchanged.


## Shared WebView shell — 2026-09-20

Following the scope clarification, the normal WebKit window and the isolated
CEF host now use one `src/ui/shell/web_app_shell.rs`: header, Back/Forward, core menu,
load progress, overlay layout, keyboard shortcuts and focus/hover/menu/dialog
holds. Hiding uses a cancellable 1.5-second timeout, without an idle animation
loop. The CEF host implements the existing Home, reload-without-cache and zoom
actions. `ready.shell_actions: 1` rejects workers missing that adapter contract.
No production Electron launch/profile behavior is changed.

| Check | Result / run |
| --- | --- |
| Wikipedia | 8 checks pass: real search typing, article navigation, scrolling and shared Back action; `20260920-135429-wayland-1319442` |
| GNOME documentation | 13 checks pass: native menu zoom/reset, links, scroll, Back/Forward, zoom after history, Home, reload without cache and embedded website accessibility; `20260920-141230-wayland-1384044` |
| Native dialogs / downloads | Existing 22-check request/portal audit passes with the shared menu; `20260920-135730-wayland-1325402` |
| Xvfb | CPU surface, overlay and scaling check passes on the final host; `20260920-141403-x11-1386977` |
| Main app | Normal Meson build, all 4 package/UI checks, 104 application unit tests, shared-shell tests in both binaries and Clippy with warnings denied pass |

The real-site test found two integration defects and now covers them:

- In this CEF 152 OSR integration, `SetZoomLevel` could acknowledge a changed
  level while a document restored through history kept its old layout. Calling
  `NotifyScreenInfoChanged` and `WasResized` after setting zoom updates the fixed
  GTK viewport. The final audit independently observes DPR 1 → 1.1 and CSS
  viewport 800×640 → 727×582, then reset to 800×640, including after history.
- Adwaita's `background` class alone did not make the overlaid header opaque.
  A shared, theme-aware header rule now paints `--window-bg-color`; screenshots
  show that website headers no longer bleed through Alcove's title/buttons.

Audit corrections: wait for usable document content rather than every network
asset; separately wait for the website's AT-SPI attachment; open the native
menu with F10/Space so a site's own Menu button cannot be mistaken for it.
Earlier unsuccessful runs are retained under the ignored run directory.

The remaining product step is extracting the reusable CEF view/worker adapter
from the timed diagnostic host and connecting it to optional app launching.
This shared-shell iteration does not claim full engine parity or a completed
manager redesign. Broad browser certification remains deferred.

## Ordinary app launch — 2026-09-20

The same Rust CEF runtime now lives in `src/engines/native_chromium/` and serves both
an isolated probe and the ordinary `alcove APP_ID` command. The latter requires
an optional build feature and an explicit, version-checked add-on manifest.
Electron remains the default Chromium implementation.

Final native-Wayland launch run: **`20260920-144447-launch`**, **15 checks pass**:
configured user agent; real site button interaction; F5 from website focus;
repeated launch presents the existing window; no probe recording; graceful
close removes temporary IPC; separate CEF profile; localStorage survives a
restart; two apps have independent storage; native network-error retry;
renderer-crash retry; worker-crash error; reopening after worker crash;
public Wikipedia in the ordinary Alcove window; no XWayland dependency.
All destructive crash checks target only descendants of disposable test apps.

Fixed a lifetime issue during this check: GTK signal references could retain
the runtime directory and profile lock after CEF exited. The worker now owns
those resources separately from view options and releases them explicitly
after reaping the child; an application hold allows graceful shutdown.
The native error page keeps the viewport allocation and restores website focus
when a retry starts. Error messages are included in the Russian catalog.

Regression evidence after sharing the runtime: GNOME documentation run
`20260920-144045-wayland-1424094` passes all **13** shared-window checks;
Xvfb run `20260920-144045-x11-1424119` passes the CPU surface check.
A preceding 50-second GNOME run ended before the audit completed; the full
scenario uses its established 80-second duration. No CEF protocol/load errors
were recorded in either GNOME run. The launch harness also needed its private
Mutter keyboard seat initialized before native actions, Chromium process titles
parsed correctly, and the actual Chrome `Default` profile layout recognized.

Validation: **7** library tests, **104** application tests, **4** normal Meson
checks including UI smoke, all-target Clippy with warnings denied, catalog
compilation and syntax/diff checks pass. The normal build remains independent
of the CEF SDK.

This is an experimental adapter, not completed migration or deployment.
Custom permission/navigation/proxy/background configurations fail explicitly
until mapped. Theme-color preference, notifications, restarting a worker inside
the existing window, add-on installation/backup integration and Flatpak remain.

## Saved WebView policies — 2026-09-20

The native adapter now uses the existing Alcove model, policy and atomic
repository updates for permission decisions and navigation allowlists.
Permission choices match WebKit: Not Now, Always Block, Allow for This Session
and Always Allow. Supported custom proxy settings are applied before CEF's
network context starts. Background mode remains explicitly unsupported.

Worker protocol **v4** requires the `policy_ui: 1` capability. Incompatible
manifests are rejected before launch. The top-level resource callback waits for
the native navigation decision, preserving redirects, popup requests and POST
bodies. The host does not recreate an accepted request as a GET.

| Check | Final evidence |
| --- | --- |
| Saved policy | `20260920-151849-policy`: **12** checks pass — persistent Allow/Block; initial Notification/geolocation permission state; Block overriding an old Chrome grant; session expiry; Not Now asking again; actual proxy routing; navigation blocked before network; redirect decision; retained POST body; popup policy; saved allowlist and restart |
| Native site requests | `20260920-151748-wayland-1543842`: **22** checks pass — shared menu/modal gating, JS dialogs, permissions, real upload/save/cancel portals, download cancellation, before-unload, navigation cancellation and permission re-request after cancellation |
| Ordinary launch / public site | `20260920-151450-launch`: all **15** launch, storage, isolation, recovery and Wikipedia checks pass on protocol v4 |
| Xvfb | `20260920-151451-x11-1533268`: CPU frame transport and overlay pass on protocol v4 |
| Code / package checks | **27** library + **86** application tests, **4** normal Meson checks, all-target Clippy with warnings denied, Python syntax, Russian catalog compilation and diff checks pass |

Two integration bugs were reproduced and fixed:

- CEF's permission acceptance persists a Chrome exception. Without startup
  synchronization, an old grant could bypass a later Alcove block and a session
  grant survived a restart. The worker now resets only corresponding permission
  exception preferences, then seeds saved Alcove choices through CEF's public
  content-setting API before navigating. Cookies and site storage remain intact.
  Preference names are tied to the pinned Chromium version and require the
  startup/restart audit when upgrading.
- Navigation could wait behind the previous document's open permission dialog,
  while cancellation waited for that navigation to reach `OnLoadStart`.
  The first regression run `20260920-151451-wayland-1533251` exposed this stall.
  The navigation gate now cancels departing-document UI before queuing its
  decision, leaving before-unload under CEF control. Cancellation uses DISMISS,
  so it does not silently save a permanent denial. The final audit covers both
  navigation completion and a subsequent request for the same permission.

The previous generic multiple-download prompt is no longer duplicated: every
file still requires the existing destination portal, matching WebKit. Its audit
step was replaced by the cancellation/re-request regression above. The current
suite therefore still contains 22 meaningful checks.

This completes the policy mapping iteration, not the accepted plan or a release.
Background lifecycle, theme-color preference, notification delivery, in-place
worker restart, add-on installation/backup and Flatpak remain. The manager draft
also still needs the accepted simplification and screen-by-screen review.
Electron remains the default and existing user profiles are untouched.

## Existing background mode — 2026-09-20

CEF now follows the existing saved background/autostart policy. WebKit and CEF
share `BackgroundSession` for the application hold, background notification and
its show/stop actions. Closing the primary window hides it when authorized;
reopening presents the same page. Child windows still close normally. Explicit
Stop follows CEF's normal before-unload confirmation and graceful shutdown.
Dialogs and errors reveal the hidden window when they require attention.

`ready.window_lifecycle: 1` confirms support for visibility forwarding to CEF's
`WasHidden`. A background start does not present the GTK window and retains the
configured viewport until its first visible allocation. Hiding also saves the
window state. Profile locks and temporary IPC keep their existing worker-owned
shutdown lifetime.

| Check | Final evidence |
| --- | --- |
| Background lifecycle | `20260920-152924-background`: **11** checks pass — authorization required for autostart; hidden launch; native notification actions; same-page presentation on relaunch; close retaining the site; notification Show; notification Stop and cleanup; website dialog revealing the window; cancelling before-unload; menu Stop and persistent storage; worker failure revealing the error |
| Ordinary launch / real site | `20260920-152846-launch`: all **15** checks pass, including Wikipedia, persistence, isolation and crash recovery |
| Xvfb | `20260920-153144-x11-1586023`: CPU surface/overlay pass, with successful harness cleanup |
| Code / package checks | **27** library + **86** application tests; all-target Clippy with warnings denied; all **4** normal Meson checks including the existing WebKit background UI test; formatting, catalog and syntax checks pass |

The background audit receives the actual GNotification payload in an isolated
`org.gtk.Notifications` recorder and invokes Alcove's real application actions.
It verifies transport and actions; it does not certify the notification's visual
presentation by GNOME Shell. All compositor input, profiles and notifications
belong to the disposable session.

The first Xvfb rendering run passed but teardown encountered the private document
portal's asynchronous FUSE unmount. Python's `ignore_cleanup_errors` did not
cover that permission-reset path. The harness now waits for the private mount
to finish unmounting before removing its runtime directory, with a bounded
`fusermount3` fallback targeting only that run's document mount. The final run
exits successfully and the earlier disposable runtime was removed.

Background integration is complete for the tested native adapter. The whole
plan remains open: theme-color preference, website notification delivery,
in-place worker restart, add-on/backup integration, Flatpak and the accepted
manager UI remain. Electron is still the default; no user profiles were migrated.
