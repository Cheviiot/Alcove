# Library verification — 2026-09-20

Environment: Fedora 44 Distrobox `alcove-dev`, actual GTK4/libadwaita manager,
disposable XDG directories and private graphical/session buses. Electron and
native CEF code were not changed by this library work.

| Run | Result |
| --- | --- |
| `build/ui-runs/20260920-160431-library` | Nine checks pass under native Wayland, headless Mutter without XWayland and unmodified Orca |
| `build/ui-renders/20260920-155934-normal` | Eight Russian screenshots; light/dark, empty/populated/search/no-results, 360-pixel width and long names |
| `build/ui-renders/20260920-155934-high-contrast` | Same checks with actual Adwaita high contrast enabled, including dark appearance |
| `build/ui-renders/20260920-155934-normal-large-text` | Same checks with doubled text (`Adwaita Sans 20`) |
| 10,000 application records | 205 row widgets initially, 206 after scrolling to the final app; all three render variants pass |
| Meson suite | 4/4 pass, including full manager/dialog UI smoke |
| Rust checks | 27 library + 86 application tests pass; final Clippy with `ui-tests,native-chromium`, all targets and warnings denied passes |
| Resources | Blueprints compile; Russian catalogue, whitespace and formatting checks pass |

The independent Wayland audit verifies title/domain semantics, absence of
launch controls, keyboard domain search, Cyrillic filtering via EditableText,
Tab/Enter row activation, Alt+Left and Escape, native menu sorting and exit.
It confirms that visiting settings does not create an engine profile. Orca's
recorded speech contains the focused application's Russian name and domain.
No user speech service is used.

The large-text run exposed a long empty-state button forcing a 477-pixel
minimum. The action now uses the concise “Add” label; the header omits its
redundant subtitle. Russian search, sort and empty-state translations were
also corrected. Both empty and populated windows now fit at 360 pixels with
doubled text. Standard icons come from embedded resources; test sessions
exclude stale host Flatpak icon caches.

Limits: the section above covers the library. Creation evidence follows;
settings verification is still in progress.
Physical Russian layout and IME are not claimed: headless Mutter uses a US
keymap. AT-SPI row activation uses keyboard focus and Enter; GTK's exported
list-item action itself is scroll-to. Orca speech is generated and recorded,
not an audio quality evaluation. Flatpak and real touchscreen checks remain
separate. These results do not accept CEF as a replacement for Electron.

Commands and dependencies: [README](README.md). Generated logs, JSON reports
and screenshots remain in ignored `build/` directories.

## Creation verification — 2026-09-20

- Native Wayland/Orca: `build/ui-runs/20260920-164522-creation`, all 22 checks pass
  (eight library, twelve creation, two screen-reader checks).
- Russian normal and high contrast: `build/ui-renders/20260920-163945-creation-normal`
  and `20260920-163945-creation-high-contrast`, ten images each.
- Doubled text: `build/ui-renders/20260920-163530-creation-normal-large-text`,
  ten images. The dialog child minimum also fits 360 pixels; header titles
  disappear at the native breakpoint so action buttons retain their space.

The audit uses a controlled slow/offline website and actual GNOME and Russian
Wikipedia pages. Both public pages returned their real titles in the final
run. It verifies that Back keeps edited details, skipped lookups cannot replace
later input, invalid fields remain inline, and no app is installed before
confirmation. Actual portal cancellation and a failed installation preserve
the draft. Retrying installs a launcher containing the correct public command,
commits the config/icon and returns to the unfiltered library with keyboard
focus on the new row. Orca announces the address/name fields and new app/domain.

A portal fixture starts without the uninstalled development command on PATH,
then exposes it through a private runtime symlink for the retry. No user
launcher is installed. The audit excludes `.tmp-*` transaction directories
when counting committed applications. The new app order follows the greatest
existing order, so deleted entries cannot cause an incorrect newest position.
The portal backend confirmation is in English in this test container; Alcove
itself is in Russian. This checks the Dynamic Launcher portal outside Flatpak;
it does not claim Flatpak packaging acceptance.

## Main application settings — 2026-09-20

The main page now exposes name/address directly, with compact identity,
appearance, permission/behavior links and Advanced. It has no global edit/save
mode. Field changes are serialized and written against the latest metadata
record; runtime window state and unrelated fields survive.

Native Wayland run `build/ui-runs/20260920-171956-settings` passes all 34
checks (eight library, twelve creation, eleven settings, three Orca).
The native audit covers Enter and focus-leave saving, a switch saving while
another field is invalid, actual portal rename cancellation/retry, matching
launcher metadata, custom user-agent enable/disable, Ctrl+Q draft protection
and Alt+Left saving before returning to the updated library. A failed rename
can be discarded on Back without reopening its portal; declining the discard
keeps the draft. Actual discard preserves all previously saved settings. Orca speech
contains the setting names and renamed application. Name/icon changes still
use the actual system launcher confirmation; local preferences do not reinstall
it. Failed fields stay editable with inline errors and next-launch changes have
a persistent notice.

Russian renders `build/ui-renders/20260920-171655-settings-{normal,high-contrast,normal-large-text}`
all pass, five images each: normal/narrow main page, dark appearance, normal/narrow
Advanced after scrolling. Engine selection uses its subtitle for the selected
value so it remains readable with doubled text; the destructive button uses a
short label. No custom CSS or host profile is used.

Three service regressions cover runtime settings without a launcher request,
rejected name/icon changes without data loss, and window changes occurring
while the rename portal is open. Rust tests pass (27 library, 89 application),
Clippy passes with all targets and `ui-tests,native-chromium`, and the ordinary
build also passes. The Meson suite passes 4/4.

At this milestone the separate Permissions and Privacy and Power dialogs still
had their old explicit Save flow; their completion is recorded below.
Supporting windows have not completed their own
large-text/keyboard/screen-reader acceptance. In particular, the full doubled-
text smoke exposed a 513-pixel minimum in the backup dialog at a requested 360
pixels; this remains tracked work, not a passing result. These manager checks
do not establish CEF packaging or Flatpak acceptance.

## Permissions and behavior — 2026-09-20

Both sections now use native preferences dialogs with individual saves. Choices,
switches, reset, and filter changes save immediately; text saves on Enter, focus
leave, and closing. Valid text is preserved when its navigation switch is turned
off. An invalid field does not block an unrelated edit. Saved settings survive
discarding a remaining draft. All writes merge into the latest policy.

Native Wayland/Orca run `build/ui-runs/20260920-174747-policy` passes 26 checks:
eight library checks, sixteen policy checks, and two screen-reader checks.
The policy scenarios include a concurrent permission change, a deliberately
unwritable policy path, reset without losing other policy sections, normalization,
proxy Enter/blur, close/discard, and real portal filter import. Cancellation and
invalid JSON do not add a filter; valid rules are compiled by WebKit and saved.
Ctrl+Q closes the policy dialog before the manager, preserving pending text;
Escape is routed to the visible dialog instead of navigating its parent page.

The private test desktop has no Background portal implementation. Its actual
failure appears inline and restores both the switch and saved state to disabled;
this run does not claim a successful desktop background grant. Service tests
cover grant/denial/rollback separately. The CEF background-window checks remain
documented with that experiment.

Russian render runs `build/ui-renders/20260920-174247-policy-{normal,high-contrast,normal-large-text}`
pass thirteen screenshots each. They cover populated/empty permissions, light
and dark appearance, and navigation/proxy/background/filter groups at 360 pixels,
including scrolling to lower sections. Selected combo values use their subtitles
to fit enlarged text. Reset clears obsolete field errors and permits making a
new choice immediately. Main-window keyboard actions respect the open dialog.

Rust tests pass (27 library, 90 application), with all-target Clippy for
`ui-tests,native-chromium` and the default build. Meson passes 4/4. Supporting
windows still need their own acceptance, especially doubled-text backup/restore.
The shared action header now has a narrow-width breakpoint; that alone is not
complete validation of the supporting screens. CEF packaging/Flatpak remain
separate unfinished work.

## Supporting dialogs — 2026-09-20

Final Russian render runs
`build/ui-renders/20260920-181355-utilities-{normal,high-contrast,normal-large-text}`
pass 42 images each. They cover backup/restore, encrypted password retry errors,
archive progress, four add-on states, diagnostics, capabilities, both shortcut
sets, About, and empty/populated downloads. All include relevant light/dark and
360-pixel layouts. The previously failing doubled-text backup header now fits
using an explicitly labelled cancel icon; add-on actions use full-width native
button rows. Download filenames elide while progress and actions remain visible.

Native Wayland/Orca run `build/ui-runs/20260920-181548-utilities` passes 22 checks:
eight library, twelve utility, and two screen-reader checks. Actual file portals
save plain/encrypted archives, cancellation preserves the library, and an incorrect
password can be retried against the same selected archive. Identical restore
entries cannot be selected; conflicting entries explain separate-copy behavior.
Restoring only the selected missing app installs its launcher through the actual
Dynamic Launcher portal and preserves the unselected modified app. All profiles,
archives and launchers belong to the disposable session. Orca announces the
utility controls and restore choices.

Capabilities load into the current dialog and refresh in place; a closed dialog
does not return after an asynchronous probe. Diagnostics exposes a deliberately
corrupt fixture without hiding valid apps. The missing add-on action is checked,
but installation of that add-on is outside this audit.

Clippy passes for all targets with `ui-tests,native-chromium`; the default build,
27 library/90 application tests, gettext validation and Meson 4/4 pass. These
manager results do not establish native CEF packaging or Flatpak acceptance.

## Installed UI review snapshot — 2026-09-20

At the user's request, the current worktree was built in `alcove-dev` and
installed as the user Flatpak `io.github.cheviiot.alcove//master` (0.7.0), commit
`59556f0b9b1538c489ab08ef099181eb98495cc5ce4503c03ee710f10c24c645`.
The GNOME launcher is exported and ordinary desktop startup holds the application
D-Bus name with an empty application error log. Permissions remain the existing
network/IPC/Wayland/fallback-X11/audio/DRI set, without filesystem access additions.

This is a main-application preview with WebKit. The generated local manifest
`build/current-review-flatpak.json` omits the optional Electron/Zypak artifact
modules after GitHub fetches stalled; it retains the broker and optional extension
declaration. Neither experimental CEF nor the Electron extension was installed.
Build and install logs are `build/current-flatpak-main.log` and
`build/current-flatpak-install.log`. The normal development manifest remains the
full build; its source exclusions now also omit the local export repository,
release state and generated bundle.

The installed application also survived a 12-second isolated Xvfb startup without
an application panic. That session had a document-portal mount conflict, so it
does not establish Flatpak file-portal or launcher-portal acceptance. The existing
private native portal audits above remain separate evidence. No CEF sandbox or
profile migration acceptance is implied by this installation.
