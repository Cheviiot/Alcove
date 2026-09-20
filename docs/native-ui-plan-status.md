# Approved native UI plan — implementation tracker

This tracks the whole accepted plan. Passing one diagnostic or finishing one
subsystem does not complete the plan. The released Electron engine and user
profiles remain in use while the optional native adapter is integrated.

## Scope clarification — 2026-09-20

Alcove is a simple website-as-an-application launcher. The user clarified that
CEF should repeat the existing WebKit WebView's behavior in the same window.
GNOME HIG applies to the native Alcove shell; website content belongs to the
engine. A separate browser UI or an exhaustive browser certification project
is not the objective.

Priority is now shared shell → equivalent existing window actions → optional
launch adapter and essential native requests. Broad IME/media/benchmark audits
are deferred, with existing results retained below. They do not prevent shared
shell or manager work. Deployment still needs profile, lifecycle and sandbox
checks; this clarification does not authorize replacing Electron prematurely.

## Native Chromium evidence and remaining checks

| Requirement | Current evidence / next acceptance work |
| --- | --- |
| Optional, isolated experimental build | Implemented Cargo feature, separate executable, private run profiles and XDG directories; official C++ CEF worker and Rust GTK host |
| Genuine libadwaita shell | Shared WebKit/CEF component in `src/ui/shell/web_app_shell.rs`; native opaque overlay header, Back/Forward/title/menu/window controls; 1.5 s hiding and F10; viewport preservation verified |
| Native Wayland | Headless Mutter without XWayland verified; separate Weston 200% and Xvfb surface checks |
| GPU texture lifetime | EGL/GBM-owned copies and SCM_RIGHTS → GDK; basic FD stability; longer stress and Electron comparison remain |
| Resize / scale / maximize / fullscreen | Basic scripted checks pass; fractional scaling and more hardware remain |
| Input | Basic GTK IM commit, focus, AT-SPI selection and compositor Tab/Enter pass; Russian keyboard layout, composition/candidate location and clipboard remain |
| Website accessibility | Embedded AT-SPI site, native text/actions/links, navigation retirement and unmodified Orca verified; broader interfaces and workflows remain |
| Site requests | 22 integration checks pass on protocol v4: native dialogs/permissions, real file portals, downloads, navigation cancellation, permission re-request and modal action blocking; notification delivery still remains |
| Popups / OAuth | Native multi-view transport, local OAuth mechanics and per-window AT-SPI association implemented; identical-document and Orca audits cover routing/lifetimes; see final run evidence |
| In-page popup surfaces | Protocol v3 routes generation-checked PET_POPUP textures to a GTK overlay; real pointer/keyboard and native option actions tested at 100%/200%; see final evidence |
| Real websites | Wikipedia search/article/scroll/native Back; GNOME documentation also covers shared menu Home/reload/zoom/history (13 checks); MDN WebM playback and Khronos WebGL verified in isolated native windows; broader web-app/identity-provider compatibility remains |
| Saved policies | Existing Alcove permissions, session grants, navigation allowlist and explicit proxy settings integrated; startup permission state, restart, redirects, POST and popup decisions verified |
| Background | Shared WebKit/CEF notification and application hold; authorized hidden start, close-to-background, same-page presentation, explicit Stop, before-unload and errors verified in private Mutter |
| Notifications | Saved/temporary permission choices verified; native delivery/actions still unverified |
| Performance / media | Equivalent scroll, animation, WebGL and video benchmarks against existing Electron still required |
| Reliability | Ordinary-launch audit passes persistent per-app CEF storage, restart, separate app storage, repeated launch and graceful IPC cleanup; native network-error/renderer-crash retry and reopening after worker crash pass; in-place worker restart remains |
| Flatpak | CEF integration with sandbox retained still required |

Detailed reproducible evidence and known limitations live in
[the experiment](../experiments/native-chromium/README.md) and
[results](../experiments/native-chromium/RESULTS.md).

## Product integration

### Manager progress — 2026-09-20

The **library is implemented and verified**: virtualized `GtkListView` with
`AdwClampScrollable`, icon/name/domain rows, add/search/sort, settings-only
activation and no launch controls. Its 10,000-app check creates 205–206 row
widgets. Private native Wayland keyboard/AT-SPI/Orca audit
`20260920-160431-library` passes all nine checks. Russian normal, high-contrast
and doubled-text runs `20260920-155934-*` pass light/dark and 360-pixel layouts,
including empty states and long titles. See [UI evidence](../experiments/ui/RESULTS.md).

**Creation is implemented and verified**: URL → metadata review → real portal
installation → library with the new row focused. Cancellation, offline/skip,
validation, failed installation/retry and real GNOME/Wikipedia metadata pass.
Native Wayland audit `20260920-164522-creation` passes 22 checks, including Orca
announcing creation fields and the newly created app. Three Russian render
variants cover 360-pixel width, small windows, light/dark, high contrast and
doubled text. See the same UI evidence for exact runs.

The **main application page now saves individual settings**: direct name/address
fields, compact identity, appearance, permissions/behavior links and Advanced.
Native audit `20260920-171956-settings` passes 34 library/creation/settings/Orca
checks, including Enter/blur, independent switches, real rename cancellation and
retry, launcher synchronization, user-agent changes, Ctrl+Q protection and saving
before Alt+Left. Metadata field edits retain concurrent runtime window state.
Only name/icon changes invoke launcher confirmation. See UI evidence for render
variants and limitations.

**Permissions and Privacy and Power now save individual changes** in native
preferences dialogs. Immediate choices/reset/switches, Enter/blur text saving,
failed writes, concurrent settings, draft discard and real file-portal filter
import pass their native audit. Russian light/dark, high-contrast and doubled-
text rendering passes thirteen images per variant, including all settings groups
at 360 pixels. See UI evidence for exact runs and the background-portal limitation.

**Supporting dialogs are implemented and verified.** Backup/restore, add-ons,
capabilities, diagnostics, help, About and populated downloads pass 42 images
in each Russian normal/high-contrast/doubled-text run, including 360-pixel widths.
Native Wayland/Orca audit `20260920-181548-utilities` passes 22 checks, including
actual file portals, encrypted password retry and selected restoration through
the Dynamic Launcher portal. Capabilities refresh in place without reopening a
closed dialog. See UI evidence for the exact scope; CEF packaging remains separate.

1. Share the web-app shell and WebKit/Chromium actions. The shared
   `src/ui/shell/web_app_shell.rs` now builds both windows’ navigation/header/menu/content
   layout and owns overlay visibility. The optional `native-chromium` feature
   now connects the same runtime to `alcove APP_ID`, behind an explicit add-on
   manifest environment setting. Incompatible manifests/workers and unsupported
   proxy configurations fail explicitly. Saved permissions, background mode,
   navigation and supported proxies now share the existing Alcove policy.
   Public commands, engine choice and default Electron behavior are preserved.
   Complete add-on/backup integration
   and establish a tested profile migration policy separately.
2. Library, URL → review → create, the compact main detail page and separate
   permission/behavior sections are implemented and verified.
3. Main-page immediate switch saving, text saving on Enter/blur, inline failures
   and next-launch notices are implemented and verified, including Permissions
   and Privacy and Power. Existing portal authorization remains in place.
4. Native Adwaita backup, restore, add-ons, diagnostics and help are implemented
   and verified. Folders, categories and bulk actions are outside this redesign.
5. Validate every screen at narrow sizes, in light/dark/high contrast, with
   enlarged text, keyboard-only navigation, Russian translation and a screen
   reader. Run normal application checks before final acceptance.

Manager acceptance is recorded per screen in the UI evidence. It does not
establish remaining CEF integration or Flatpak acceptance. No release, commit,
profile migration or engine replacement has been performed as part of the experiment.
