# Native manager UI checks

Run inside the existing Fedora 44 Distrobox `alcove-dev`. Build the normal
manager with its diagnostic feature:

```sh
distrobox enter alcove-dev -- meson setup build --reconfigure -Dui_tests=true
distrobox enter alcove-dev -- meson compile -C build
distrobox enter alcove-dev -- meson test -C build --print-errorlogs
```

Do not rebuild while a GUI audit is running: the build copies the executable.
The normal Meson checks cover supporting dialogs as well as the library.

## Library rendering

```sh
distrobox enter alcove-dev -- python3 experiments/ui/render.py
distrobox enter alcove-dev -- python3 experiments/ui/render.py --high-contrast
distrobox enter alcove-dev -- python3 experiments/ui/render.py --large-text
```

Each invocation creates a private Xvfb/session bus and disposable XDG data.
PNG screenshots and a JSON result are written under `build/ui-renders/`.
The test checks empty/populated/search/no-results states, 360-pixel windows,
light and dark appearance, long names and a missing Chromium add-on. Doubled
text uses `Adwaita Sans 20`; high contrast asserts the actual Adwaita setting.
The 10,000-app scenario counts instantiated row widgets and scrolls to the
last item, checking actual virtualization rather than just the model size.

`ALCOVE_TEST_LOCALEDIR` and `--ui-test-library` are available only in builds
with `ui-tests`. Icon resources are embedded for uninstalled builds. Test
`XDG_DATA_DIRS` excludes host Flatpak exports and their icon caches.

## Real keyboard and screen reader

```sh
distrobox enter alcove-dev -- python3 experiments/ui/library-audit.py --orca
```

This launches the real manager with three disposable application records under
headless Mutter **without XWayland**. An independent AT-SPI client inspects the
widgets while Mutter injects Ctrl+F, Tab, Enter, Escape, Alt+Left and menu keys.
The audit verifies title/domain exposure, filtering, settings-only activation,
sorting and clean exit. It checks that opening settings creates no engine
profile. Headless Mutter uses a US keymap: domain search uses physical key
events; Cyrillic search is exercised through AT-SPI EditableText. This does
not claim Russian keyboard-layout or IME coverage.

With `--orca`, unmodified Orca processes focus events and its generated speech
is recorded in a debug log. Speech output is disconnected from the user's audio
service. The audit shares the native CEF experiment's test lock and never uses
Orca's replacement option. GTK list items expose `listitem.scroll-to` as an
AT-SPI action; opening a row is tested using its keyboard focus and Enter.

Results and logs are written under `build/ui-runs/`. The scripts clean up their
own processes, data and document-portal mounts. Build results and screenshots
are not shipped with the application.

## Permissions and behavior

```sh
distrobox enter alcove-dev -- python3 experiments/ui/library-audit.py --policy --orca
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen policy
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen policy --high-contrast
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen policy --large-text
```

The policy audit seeds only disposable settings. It checks immediate permission
choices, concurrent changes, failed writes, reset, navigation, proxy fields,
filter changes and the actual file chooser. Filter import covers cancellation,
invalid JSON and successful WebKit validation. Background settings exercise the
actual private desktop portal; a missing backend must leave the switch off with
an inline error. This does not establish a successful background grant in a
desktop session whose backend implements that portal.

Rendering covers populated/empty permissions and navigation, proxy, background
and filter groups. Each appearance run produces thirteen screenshots, including
light/dark themes and 360-pixel width. Scroll checks include groups below the
initial viewport. The audit records Orca's announcements of the actual settings.

Additional Fedora packages used by these audits (see also the native Chromium
experiment's setup):

```sh
distrobox enter alcove-dev -- sudo dnf install -y mutter python3-pyatspi orca fuse3
```

Xvfb, D-Bus and the GTK development environment are documented in
`CONTRIBUTING.md`. No development packages are installed on the host.

## Creation

```sh
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen creation
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen creation --high-contrast
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen creation --large-text
distrobox enter alcove-dev -- python3 experiments/ui/library-audit.py --creation --orca
```

Rendering uses the feature-gated `--ui-test-creation` entry point. The independent
Wayland audit uses the ordinary manager and the actual Dynamic Launcher portal.
It checks controlled metadata/delay/offline cases, GNOME/Wikipedia metadata,
cancellation and installation failure, then successful creation and return.
For the retry it places a `alcove` symlink to the build only in the disposable
session's PATH; the executable is never installed on the host. Only committed
app directories count toward success. With Orca it also checks spoken field
labels and the new focused row. Portal and file chooser data stay in the private
session. The installed Fedora portal backend may use English confirmation
labels even when the application's language is Russian.

## Application settings

```sh
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen settings
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen settings --high-contrast
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen settings --large-text
distrobox enter alcove-dev -- python3 experiments/ui/library-audit.py --settings --orca
```

The settings renderer uses the feature-gated `--ui-test-settings` entry point.
It captures the main form and the scrolled Advanced section at normal/360-pixel
width, with dark appearance as well. The full `--ui-test-app-page` smoke still
includes all supporting dialogs; their separate failures must not be treated
as settings acceptance.

The native audit first creates a disposable application through the real portal.
It checks saving on Enter/blur, immediate switches despite another invalid field,
rename cancellation/retry and launcher synchronization, custom user-agent saving,
Ctrl+Q draft protection and Alt+Left saving before returning to the library.
AT-SPI is used to inspect fields and portal buttons; real Tab and key events
exercise focus/navigation. GTK4 does not implement Component.grabFocus or
Action on every exposed row, so keyboard navigation is verified directly.
Orca must announce settings controls and the renamed application as well as the
previous library/creation checks. The separate policy audit covers autosaving in
Permissions and Privacy and Power.

## Supporting dialogs

```sh
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen utilities
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen utilities --high-contrast
distrobox enter alcove-dev -- python3 experiments/ui/render.py --screen utilities --large-text
distrobox enter alcove-dev -- python3 experiments/ui/library-audit.py --utilities --orca
```

The renderer produces 42 images per variant: backup, restore, encrypted password
errors, progress, four add-on states, capabilities, diagnostics, both shortcut
sets, About, and empty/populated downloads. Normal, dark and 360-pixel layouts
include active, complete and failed downloads with long names.

The ordinary manager audit uses actual file and Dynamic Launcher portals in a
private Wayland session. It verifies encrypted backup/password retry, cancelled
file selection, identical/conflicting/selected restoration and an actual launcher
round trip. Only disposable fixtures are changed. A deliberately invalid app
record exercises diagnostics while valid apps remain usable. Orca must announce
utility controls. The add-on check verifies its missing-state install action;
it does not install the add-on or establish CEF Flatpak compatibility.
