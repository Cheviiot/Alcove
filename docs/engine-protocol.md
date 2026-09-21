# Native Chromium worker contract v4

The Rust GTK host starts one CEF browser worker. CEF may create its own
sandboxed renderer/GPU subprocesses. Worker stdin/stdout carry bounded,
newline-delimited JSON; an inherited Unix datagram socket carries GPU buffers.
These descriptors are private to the parent/worker and are not exposed to
page JavaScript. The worker marks its GPU socket close-on-exec before CEF starts
subprocesses. There is no listener socket or remote-debugging endpoint.

Each command includes `protocol: 4`, a numeric `view` and `command`. The host verifies the
worker's `ready.protocol` before treating the engine as ready. Unsupported
versions fail; there is no fallback to another ABI. Native mode additionally
requires `ready.native_atspi: 3`; workers without that capability are rejected.
The host also requires `ready.multi_view: 1`, `ready.site_requests: 1`,
`ready.surfaces: 1`, `ready.shell_actions: 1`, `ready.policy_ui: 1` and
`ready.window_lifecycle: 1` before accepting website UI requests.
The latter adds Home navigation, reload without cache and load-progress events.
Ordinary app launches also require `ready.app_launch: 1`: independent persistent
CEF profiles and disabled probe instrumentation. Before starting a worker,
Alcove validates the optional add-on manifest (schema 1, worker protocol 4,
exact pinned CEF version, and paths contained within the add-on directory).

| Command | Parameters |
| --- | --- |
| `resize` | Logical `width`, `height`, `scale` (1–4); physical dimensions bounded to 4096 |
| `back`, `forward`, `reload`, `reload-bypass-cache`, `stop` | None |
| `load` | HTTP(S) `url`; used by the shared Home action |
| `close-view` | Close this browser after its before-unload decision |
| `quit` | Process-wide forced shutdown of all browsers; no view required |
| `focus` | `focused` |
| `mouse` | Logical `x`, `y`, `modifiers`, optional `leave` |
| `click` | `x`, `y`, `button`, `count`, `up`, `modifiers` |
| `scroll` | `x`, `y`, `dx`, `dy`, `modifiers` |
| `key` | Windows virtual `key`, native keycode, `up`, `modifiers`; plain Return also produces the required CEF character event |
| `text`, `composition` | UTF-8 `text` |
| `cancel-composition` | None |
| `zoom` | CEF zoom `level` |
| `theme` | `dark`; synchronizes native theme preference into page CSS |
| `native-accessibility-inspect` | Read-only diagnostic snapshot of native ATK objects; requires `--alcove-diagnostics` |
| `native-accessibility-focus-response` | Pending accessibility action `id`, actual GTK site `focused` result |
| `evaluate` | Diagnostic-only `script`; rejected without `--alcove-diagnostics`, never a page API |
| `request-response` | Pending request `id`, `allow`; JS `text`, chooser `paths`, download `path` as appropriate |
| `download-control` | `download` id and `action`: `cancel`, `pause` or `resume` |

Events include `ready`, `navigation`, `load-progress`, `title`, `frame`, `gpu-frame`,
`gpu-export`, `gpu-dropped`, `gpu-error`, `accessibility`, `load-error`,
`zoom-changed` (requested/accepted CEF levels), `renderer-crashed`, `protocol-error` and `unsupported`.
`gpu-frame` only means CEF delivered a callback. `gpu-export` means a copied
buffer entered the socket. `gpu_presented` in the report counts GTK textures
assigned to the picture; screenshots provide the independent visual check.
None of these counters are display FPS or a performance comparison.

CPU frames use atomic replacement of `frame.bin`: four little-endian u32 words
(magic `0x42535446`, protocol, width, height), then exactly width × height × 4
premultiplied BGRA bytes. The reader validates all dimensions and the exact
length before allocating a texture. It can skip intermediate frames.

GPU datagrams contain bounded JSON (`protocol`, `view`, `surface`, `generation`, `sequence`, physical `width`,
`height`, `fourcc`, `modifier`, `stride`, `offset`) and exactly one transferred
descriptor. The output is linear ARGB8888, independent of CEF's pool. The
worker completes all GPU reads of the source before returning from CEF's
callback. The destination remains immutable. The socket owns queued file
references; GTK's texture release closes received references. Truncation,
missing descriptors, invalid dimensions and wrong versions are rejected.

The original `accessibility` tree events remain diagnostic. Native mode sends
only tree ID, update count and original serialized byte count; redundant full
trees do not enter the control channel. Tree-only mode omits dumps above 3 MiB.
This avoids oversized real documents disconnecting the 4 MiB bounded reader;
an invalid/truncated event is now reported as a protocol error. In native mode,
`native-accessibility-ready` carries an `AtkPlug` identifier (`bus:path`) from
the worker. The GTK host validates the address and attaches a public
`GtkAtSpiSocket`. The native AT-SPI bus carries semantic actions, text and
accessibility events; no renderer-private IPC is introduced for them.
`native-accessibility-document` records per-view document generations;
`native-accessibility-bound` records the view/tree association and
`native-accessibility-binding-error` fails conflicting association.
`native-accessibility-focus` records mirrored focus changes.
The `focus` command is replayed after the worker handshake and gates native
accessibility focus on the actual GTK window and site-container focus.
See [the Chromium engine](chromium-engine.md) for lifetime and event ordering rules.
WebKit and CEF use the shared Alcove window shell. Extended browser audits are
deferred under the clarified WebView parity scope; website notification delivery,
recovery and sandboxed packaging remain deployment work.

## Website requests capability v1

`site-request` contains a monotonically increasing `id` and one of these `kind`s:

- `navigation`: destination `url`; the actual CEF resource callback waits for
  the response, preserving redirects and POST bodies. Policy v4 hosts must reply
  even when the saved policy allows the destination without a dialog.
- `js-dialog`: requesting `origin`, CEF `type` (alert/confirm/prompt), plain-text
  `message` and `initial` text.
- `before-unload`: `origin` and `reload`; host supplies its own native wording.
- `permission`: `origin`, `media` (which CEF bitmask is used) and `permissions`.
- `file-dialog`: CEF `mode`, `filters`, title and suggested base `name`.
- `download-request`: CEF `download` id, URL and suggested base `name`.

CEF retains the callback, with a maximum of 32 pending requests. The renderer
cannot access the response pipe. Host dialogs are serialized. Unsupported
permission bits and desktop-capture requests cannot be granted by the generic
confirmation dialog. The requesting origin and page text never use markup.

The first response consumes the callback; repeated/stale responses are ignored.
`request-resolved` confirms handling. `request-cancelled` invalidates an active
or queued host surface when its page navigates or Chromium dismisses it. The
host closes any matching file portal request through `Request.Close`, not just
its local future. Closing the worker also cancels pending callbacks/downloads.
Downloads may continue across ordinary page navigation.

`download` reports id, suggested name, path, received/total bytes and
progress/paused/complete/cancelled states. Destination selection is explicitly
through SaveFile; a portal cancellation never authorizes a default destination.
The media test flag supplies fake devices only; permission decisions still go
through the real CEF → GTK → CEF path and no automatic grant flag is used.

## Native accessibility capability v3

Actions and explicit component focus requests first emit
`native-accessibility-focus-request` with an id. The host gives the site real
GTK focus and responds before the worker executes the queued native ATK action.
It rejects focus while a native dialog/portal is active, and allows up to 500 ms
for a closing dialog to release its focus grab. The worker bounds pending actions
to 32 and retains proxy references until acknowledgement; retired proxies still
reject the eventual action. This closes the gap where an AT-SPI action could
reach Chromium while keyboard focus remained in the GTK header.

Capability v3 adds independent per-view sockets, native root association and
document retirement. Commands, events and focus acknowledgements carry the
owning view. The worker rejects native mode on an unvalidated CEF runtime;
the ordering contract and exact version are in [the Chromium engine](chromium-engine.md).

## Native windows capability v1

One worker owns a monotonically numbered set of views. View 1 is the original
window; popup IDs are never reused. CEF itself creates popup browsers from
`OnBeforePopup`, retaining the real opener, named target and request context.
The GTK host must not implement this by launching another worker or loading the
URL into an unrelated browser. The limit is nine live/pending views per worker.

`view-created` includes the new `view`, its `opener`, and clamped initial
`width`/`height`. It precedes that view's `ready`. Each native window reuses the
same shell, input handlers and site request adapters. Request IDs are scoped to
their view. `view-closed` means CEF has completed closing; GTK then retires that
view's inbox and textures. `close-view` allows before-unload cancellation;
`quit` is reserved for the diagnostic process-wide teardown. Creation failures
emit `popup-aborted`; rejected requests emit `popup-blocked` with a reason.

Every browser event, native accessibility event and GPU packet carries `view`.
Shared process stage/error messages default to view 1.
CPU files for popup N live in `views/N/frame.bin`; the original file remains at
`frame.bin`. A single shared GPU bridge owns EGL/GBM/socket resources for all
browsers. The host consumes the socket once, keeps at most the newest pending
texture per live view/surface and releases frames for retired/unknown views.

The `--popups` diagnostic optionally verifies native website accessibility and
Orca alongside window/session/input behavior. The separate
`--accessibility-windows` audit checks identical documents in different windows
and independent document lifetimes.

## In-page popup surfaces capability v1

`surface` distinguishes `view` (generation 0) from `popup` (generation > 0)
in GPU packets and frame events. These surfaces belong to one CEF browser;
they are separate from the native windows created by `window.open`.
`popup-surface` events carry monotonically increasing `generation`, `visible`
and `rect: [x,y,width,height]` in logical page coordinates. Show, hide and bounds
changes retire the preceding generation; navigation also hides the popup.
The main picture never receives a popup frame.

CPU popup frames use `popup-GENERATION.bin` in the view's directory with the
same validated frame header. The worker removes the retired file. GPU frames
have the same owned immutable allocation and descriptor lifetime as main
frames. The host keeps at most one pending frame per surface, tolerates
independent socket/pipe arrival ordering, and rejects stale generations and
dimensions inconsistent with logical bounds times GTK scale.

A nonmeasuring GTK overlay snapshots the texture at logical bounds without
changing the site's viewport. It is not a separate accessible or input target:
native Chromium options remain in the site's AT-SPI tree and input retains
the site's coordinate space. Hiding drops the displayed texture immediately.
This handles HTML select menus; custom DOM menus already render in `view`.

## Saved policy and permissions

Protocol v4 makes native navigation decisions mandatory and rejects v3 workers
and add-ons. The host writes a private schema-v2 policy snapshot before launch.
Permission callbacks use the same `AppPolicyV2` and atomic repository writes as
WebKit. Unknown permission bits fail closed; a mixed request requires every
requested capability. Session grants are shared with child windows. `Not Now`
uses CEF's dismiss result, so a later request can ask again. Multiple downloads
use the existing explicit save portal for every file.

CEF 152 accepts/denies permissions through persistent Chrome decisions
([upstream implementation](https://github.com/chromiumembedded/cef/blob/83ffcba/libcef/browser/permission_prompt.cc)).
Before the initial navigation, the worker resets only matching permission
exception preferences and applies Alcove's saved grants/blocks with public CEF
content-setting APIs. Cookies, local storage and unrelated preferences remain.
The version pin and startup query tests are required when upgrading Chromium.

Custom proxy settings are supplied before Chromium initializes its network
context. HTTP, HTTPS, SOCKS4 and SOCKS5 are supported; ambiguous `socks` and
SOCKS4a are rejected explicitly. Navigation is paused through
`OnBeforeResourceLoad` rather than canceled and recreated as a GET; top-level
redirects and popup requests pass through the same native decision queue.
Navigation first dismisses permissions, file choosers and JavaScript dialogs
belonging to the departing document. Waiting until `OnLoadStart` would deadlock
the paused resource behind that document's dialog. CEF still controls the
before-unload confirmation. Permission cancellation uses DISMISS, so navigation
cannot silently create a permanent Chrome denial.

## Background window lifecycle

`window_lifecycle: 1` supports the per-view `visibility` command with a Boolean
`hidden`, mapped to CEF's `WasHidden`. The GTK host reports actual show/hide state
after the handshake; Chromium pauses OSR painting while hidden and resumes when
the same window is presented. A never-mapped background window uses configured
viewport dimensions until GTK allocates it, rather than resizing the site to 1×1.

Only the main application window follows Alcove's background policy. Child
windows still close normally. WebKit and CEF share `BackgroundSession`, which
owns the application hold and native show/stop notification. Dropping it removes
the notification and releases the hold. Explicit Stop follows normal CEF close
and before-unload callbacks; it does not bypass the site's close confirmation. Dialogs and
engine errors reveal a hidden window and end its background session. Engine
shutdown still owns the profile lock and temporary IPC through child reaping.
