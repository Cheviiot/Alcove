<!-- SPDX-License-Identifier: GPL-3.0-only -->

# The Chromium engine

Alcove renders websites with WebKitGTK by default. Chromium is available for
sites WebKitGTK cannot serve. Both engines share one GTK 4 window built by
`src/ui/shell/web_app_shell.rs`: the same header, navigation, menu and
auto-hiding toolbar. The website is drawn by the engine; everything around it
belongs to the application.

## How it works

A CEF worker process renders the page and hands frames to the host, which
presents them inside the application's own window. The worker and the host
speak a private, versioned transport, so the language binding can be replaced
without touching the window. The Chromium sandbox is kept: the worker starts
through `zypak`, which maps it onto Flatpak's portals.

| Piece | Where |
| --- | --- |
| Worker (C++, CEF) | `src/cpp/` |
| Window and host (Rust) | `src/ui/shell/chromium/` |
| Add-on discovery and wire contract | `src/engines/chromium/` |
| Launch glue | `src/app/chromium.rs` |
| Transport | [engine protocol](engine-protocol.md) |

The engine ships as the Flatpak extension
`io.github.cheviiot.alcove.ChromiumNative`, mounted at
`/app/extensions/chromium-native`. It is installed separately, never appears
on its own, and is removed together with the application.

## Limits

- The worker is built for `x86_64` only. On `aarch64` Alcove ships with
  WebKitGTK alone.
- A crashed worker is recovered by reopening the window; restarting it in
  place does not exist.
- Notification delivery through the engine is not implemented.
- There are no scroll, animation, WebGL or video benchmarks, and no claim that
  the engine matches Chromium's performance.

## Accessibility

WebKitGTK exposes its own accessibility tree through GTK. The Chromium engine
renders off-screen, so it has none by default: `--native-accessibility` turns
on an ATK/AT-SPI bridge in the CEF worker that puts the website's real objects
inside the application's GTK hierarchy. It uses the CEF 152 SDK this build
pins.

### Route

1. Chromium runs with complete accessibility and `STATE_DEFAULT`; explicitly
   enabling CEF's OSR accessibility would instead select TreeOnly mode.
2. Ordered CEF accessibility batches and public ATK root load signals associate
   each native document with its browser (see below). The worker implements
   parent proxies while delegating semantics to the actual native objects.
   No Chromium private symbol or GType vtable is patched.
3. `AtkPlug` exposes that root across the private accessibility bus.
   `GtkAtSpiSocket` inserts it into the GTK site's accessible hierarchy.
   The narrow Rust FFI calls GTK's public Linux-specific header API.
4. An independent pyatspi process reads the hierarchy and invokes real native
   actions. The fixture verifies a trusted browser click; no JavaScript click
   or mouse-coordinate substitution implements the accessibility action.

### Implemented surface

- Accessible names, roles, descriptions, attributes, states, parents, children
  and mapped relations; the per-object toolkit hint selects Orca's Chromium script.
- Action, Component, Text, a subset of Document, Hypertext and Hyperlink APIs,
  only when the native object supplies the corresponding interface.
- Focus, state, children, property, document-load, text insert/remove, caret and
  selection events. Toolkit listeners export only proxy events, preventing a
  duplicate, unparented native hierarchy from leaking into AT-SPI.
- Observed native focus is gated by GTK's actual site/window focus. The host
  replays focus after worker startup. Focus events precede deferred text events
  so Orca announces the new field before processing its caret updates.
- On navigation, old proxies are retired and reject actions even if Chromium
  retains the old page in its back/forward cache. New document references are
  distinct. Remote clients may observe either `DEFUNCT` or a removed endpoint;
  the audit accepts both forms of stale-reference invalidation.

### GTK focus acknowledgement

CEF native `AtkAction.do_action` and `AtkComponent.grab_focus` now queue a bounded
focus request through the parent pipe. The GTK host focuses the site widget before
acknowledging it; the worker then sets CEF focus and invokes the original native
operation. Modal GTK dialogs/portals reject the request. Retired proxies are
checked again when the deferred operation runs. This preserves native trusted
actions while supporting the transition from header/dialog focus into web content.

In this runtime, generated GtkPopoverMenu items exposed empty AT-SPI names, even
with their labels set. The bridge therefore uses public custom menu slots with
real GTK buttons, explicit MenuItem roles and accessible labels. Keyboard opening
and semantic activation are checked by the external site-request audit.

### Multi-window association

The bridge relies on a verified ordering in the pinned Chromium/CEF release:

1. Chromium's [`RenderFrameHostImpl::HandleAXEvents`](https://github.com/chromium/chromium/blob/152.0.7977.83/content/browser/renderer_host/render_frame_host_impl.cc)
   calls `ProcessAccessibilityUpdatesAndEvents` before applying native updates.
2. The synchronous WebContents observer reaches CEF's per-browser
   [`OnAccessibilityTreeChange`](https://github.com/chromiumembedded/cef/blob/83ffcbaa0bd0aec210b39fb1e94d647d29bcffb2/libcef/browser/osr/browser_platform_delegate_osr.cc).
   Its batch supplies the tree ID, root ID and root load events. Child trees
   with a `parent_tree_id` cannot supply a top-level binding.
3. Native [`FireLoadingEvent`](https://github.com/chromium/chromium/blob/152.0.7977.83/ui/accessibility/platform/browser_accessibility_manager_auralinux.cc)
   emits the corresponding root `busy` state change on the same UI-thread stack.

A one-batch ticket consumes these matching load events. Public GObject qdata
records the immutable view/tree identity of the parentless DOCUMENT_WEB object.
Conflicting bindings fail explicitly. Tickets expire at the next batch or
message-loop iteration. URLs, titles, geometry, DOM changes and private object
layouts play no part in association. Native mode checks the full runtime version
against `152.0.7+g83ffcba+chromium-152.0.7977.83`; an upgrade requires revalidation
of this ordering, which is not a general CEF API guarantee.

Each view owns its AtkPlug, document, focus and proxy/link generations. Committed
main-frame navigation retires the old document immediately, preserving the
socket while the next document arrives. History cannot reactivate stale proxies.
The plug supplies its own state set even while empty: AtkPlug's default expects
its private child and otherwise returns NULL, which breaks bridge notifications.
Closing one view retires only that view. Deferred actions still require active
GTK site focus in the owning window.

Native and document-cleanup invalidation share an idempotent DEFUNCT notifier.
Forwarding Chromium's invalidation and then sending it again during navigation
would make AT-SPI deregister the same weak reference twice. Once invalidated,
the proxy rejects actions and further source events. The harness treats ATK
state/unref and weak-reference lifecycle warnings as diagnostic failures.

`--accessibility-windows` verifies two identical documents, separate frame
ancestors and selections, trusted per-view actions including a cross-origin
iframe, inactive-window rejection, navigation/history retirement and child
closure while the main window remains usable. `--popups --native-accessibility`
also checks local OAuth child/provider documents; both modes can run with Orca.

### What the bridge does not cover

This is not complete screen-reader support. Table/TableCell, Selection, Value
and document-wide text selections are not forwarded.
Native Chromium text fields do not expose AtkEditableText; ordinary input uses
the browser's keyboard/IME path. Rich editors, extended iframe workflows, live-region
speech policy, clipboard, magnification coordinates, long-lived dynamic trees
and physical keyboard/IME workflows are not covered.

The focus scan is bounded to 2048 nodes and the deferred text queue to 1024
events; overflow is a diagnostic error. Proxy retention is scoped to the
current document, with navigation/shutdown cleanup. Long-running applications
with continual DOM churn are untested, as are window recovery and the Flatpak
accessibility-bus permissions.

What has been exercised, how, and with which recorded limitations lives in
[`tests/engine/`](../tests/engine/README.md).
