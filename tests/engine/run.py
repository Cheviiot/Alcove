#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Isolated compositor, bus, local fixture server and ephemeral probe profiles."""
import argparse
import contextlib
import datetime
import functools
import fcntl
import http.server
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

def finish_document_portal(runtime):
    # dbus-run-session has exited, but the private portal's FUSE unmount may
    # still be in flight. Python's TemporaryDirectory can otherwise fail while
    # resetting permissions inside that read-only mount, even with ignore_errors.
    document_mount = pathlib.Path(runtime, "doc")
    deadline = time.monotonic() + 5
    while document_mount.is_mount() and time.monotonic() < deadline:
        time.sleep(.05)
    if document_mount.is_mount():
        subprocess.run(["fusermount3", "-u", str(document_mount)], check=True, timeout=5)

def configure_mutter_scale(env, scale, output):
    # This bus and compositor belong solely to the disposable test session.
    # GDK_SCALE alone is not sufficient on Wayland: set actual monitor scale.
    from gi.repository import Gio
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    service = "org.gnome.Mutter.DisplayConfig"
    state = bus.call_sync(service, "/org/gnome/Mutter/DisplayConfig", service,
        "GetCurrentState", None, None, Gio.DBusCallFlags.NONE, 3000, None).unpack()
    assert len(state[1]) == 1, "expected a single private virtual monitor"
    connector = state[1][0][0][0]
    with (output / "monitor.log").open("w") as log:
        subprocess.run(["gdctl", "set", "--layout-mode", "logical", "--logical-monitor",
            "--monitor", connector, "--primary", "--scale", str(scale)],
            env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        subprocess.run(["gdctl", "show", "--verbose"], env=env, stdout=log,
            stderr=subprocess.STDOUT, check=True)

class FixtureHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/slow-download":
            return super().do_GET()
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", 'attachment; filename="alcove-slow.bin"')
        self.send_header("Content-Length", str(1024 * 1024))
        self.end_headers()
        try:
            for _ in range(256):
                self.wfile.write(b"B" * 4096)
                self.wfile.flush()
                time.sleep(.1)
        except (BrokenPipeError, ConnectionResetError):
            pass

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--backend", choices=("wayland", "x11"), default="wayland")
    parser.add_argument("--compositor", choices=("weston", "mutter"), default=None)
    parser.add_argument("--surface-only", action="store_true", help="check frame transport/scaling/layout only; input and WebGL are observations, not acceptance criteria")
    parser.add_argument("--seconds", type=int, default=12)
    parser.add_argument("--gpu", action="store_true")
    parser.add_argument("--native-accessibility", action="store_true", help="export Chromium native ATK objects into GTK via AT-SPI")
    parser.add_argument("--layout", action="store_true", help="exercise resize/maximize/fullscreen (requires >=16 seconds)")
    parser.add_argument("--scale", type=int, choices=(1, 2), default=1)
    parser.add_argument("--theme", choices=("light", "dark"), default="light")
    parser.add_argument("--accessibility-navigation", action="store_true", help="test AT-SPI link actions and stale objects across navigation")
    parser.add_argument("--orca-keyboard", action="store_true", help="test Tab/Enter through Mutter while Orca announces the button")
    parser.add_argument("--orca", action="store_true", help="run isolated Orca with speech output recorded in its debug log")
    parser.add_argument("--site-requests", action="store_true", help="exercise native site dialogs and real file portals through AT-SPI")
    parser.add_argument("--accessibility-windows", action="store_true", help="test independent native AT-SPI trees for identical popup pages")
    parser.add_argument("--dropdowns", action="store_true", help="test CEF in-page select popup rendering and real input")
    parser.add_argument("--popups", action="store_true", help="exercise native multi-view local OAuth; add --native-accessibility --orca for screenreader checks")
    parser.add_argument("--url", help="external HTTP(S) site in a fresh profile; enables real-site audit instead of fixture tests")
    parser.add_argument("--site-scenario", choices=("inspect", "wikipedia", "gnome", "video", "webgl"), default="inspect")
    parser.add_argument("--worker", type=pathlib.Path, help="optional worker/debugger wrapper")
    parser.add_argument("--flatpak", action="store_true", help="run the installed experimental Flatpak in its private test installation")
    parser.add_argument("--session", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.flatpak and args.worker:
        parser.error("--flatpak uses the packaged worker and cannot use --worker")
    args.compositor = args.compositor or ("mutter" if args.backend == "wayland" and args.scale == 1 else "weston")
    if args.url:
        url = urllib.parse.urlsplit(args.url)
        if url.scheme not in ("https", "http") or not url.hostname or url.username or url.password:
            parser.error("--url requires an HTTP(S) URL without credentials")
        if args.compositor != "mutter" or not args.native_accessibility or args.seconds < 40 or any((args.dropdowns, args.popups, args.site_requests, args.accessibility_windows, args.layout, args.accessibility_navigation, args.orca_keyboard, args.orca, args.surface_only)):
            parser.error("--url requires Mutter, native accessibility, >=40 seconds and no fixture modes")
    elif args.site_scenario != "inspect":
        parser.error("--site-scenario requires --url")
    if args.dropdowns and (args.compositor != "mutter" or not args.native_accessibility or args.seconds < 35 or args.popups or args.accessibility_windows or args.site_requests or args.layout or args.accessibility_navigation or args.orca_keyboard or args.orca):
        parser.error("--dropdowns requires Mutter, native accessibility and >=35 seconds without other fixture/Orca modes")
    if args.popups and (args.compositor != "mutter" or args.seconds < 35 or args.site_requests or args.layout or args.accessibility_windows or args.accessibility_navigation or args.orca_keyboard):
        parser.error("--popups requires Mutter and >=35 seconds without other fixture modes")
    if args.accessibility_windows and (args.compositor != "mutter" or args.seconds < 35 or not args.native_accessibility or args.site_requests or args.popups or args.layout or args.accessibility_navigation or args.orca_keyboard):
        parser.error("--accessibility-windows requires Mutter, native accessibility and >=35 seconds without other fixture modes")
    if args.site_requests and (args.compositor != "mutter" or not args.native_accessibility or args.seconds < 45
            or args.orca or args.layout or args.accessibility_navigation):
        parser.error("--site-requests requires Mutter, native accessibility and >=45 seconds, without Orca/layout/navigation checks")
    if args.surface_only and (args.native_accessibility or args.orca):
        parser.error("--surface-only cannot be combined with accessibility/Orca checks")
    if args.orca_keyboard and (args.compositor != "mutter" or not args.native_accessibility
            or not args.orca or args.accessibility_navigation or args.seconds < 22):
        parser.error("--orca-keyboard requires Mutter, --native-accessibility, --orca, >=22 seconds, without navigation checks")
    if args.compositor == "mutter" and args.backend != "wayland":
        parser.error("the Mutter diagnostic requires Wayland")
    if args.site_requests and args.scale != 1:
        parser.error("the portal input diagnostic currently requires scale 1")
    if args.accessibility_navigation and (not args.native_accessibility or args.seconds < 22):
        parser.error("--accessibility-navigation requires --native-accessibility and --seconds 22 or longer")
    if args.layout and args.seconds < 16:
        parser.error("--layout requires --seconds 16 or longer")
    if args.seconds != 0 and args.seconds < 10:
        parser.error("timed checks require at least 10 seconds")
    if not args.session:
        # The bus launcher must inherit the private runtime too: isolating only
        # the GTK process can accidentally reuse the desktop's AT-SPI socket.
        with contextlib.ExitStack() as cleanup:
            if args.orca:
                # Orca's CLI detects peer processes across separate session
                # buses. Serialize our own checks; never replace the user's Orca.
                lock_path = pathlib.Path(__file__).resolve().parents[2] / "build/native-chromium/orca-test.lock"
                lock_path.parent.mkdir(parents=True, exist_ok=True)
                lock = cleanup.enter_context(lock_path.open("a"))
                fcntl.flock(lock, fcntl.LOCK_EX)
            runtime = cleanup.enter_context(tempfile.TemporaryDirectory(prefix="alcove-native-"))
            cleanup.callback(finish_document_portal, runtime)
            env = os.environ.copy()
            for key in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "AT_SPI_BUS_ADDRESS",
                        "DBUS_SESSION_BUS_ADDRESS", "DBUS_STARTER_ADDRESS", "DBUS_STARTER_BUS_TYPE"):
                env.pop(key, None)
            env.update(XDG_RUNTIME_DIR=runtime, GSETTINGS_BACKEND="memory", NO_AT_BRIDGE="0",
                       XDG_CONFIG_HOME=runtime + "/config", XDG_DATA_HOME=runtime + "/data",
                       XDG_CACHE_HOME=runtime + "/cache")
            if args.flatpak:
                env["FLATPAK_USER_DIR"] = str(pathlib.Path(__file__).resolve().parents[2]
                    / "build/native-chromium/flatpak/installation")
                if pathlib.Path("/run/host/var/lib/flatpak").is_dir():
                    env["FLATPAK_SYSTEM_DIR"] = "/run/host/var/lib/flatpak"
            # No GNOME settings daemon runs here. Supply its usual 96-DPI
            # baseline explicitly: Chromium 152 GtkUi otherwise calls the
            # removed GTK3 gdk_screen_get_default symbol when XftDpi is -1.
            settings = pathlib.Path(runtime, "config/gtk-4.0/settings.ini")
            settings.parent.mkdir(parents=True)
            settings.write_text("[Settings]\ngtk-xft-dpi=98304\n"
                + f"gtk-application-prefer-dark-theme={int(args.theme == 'dark')}\n")
            if args.site_requests:
                env.update(GTK_MODULES="atk-bridge", ACCESSIBILITY_ENABLED="1", GNOME_ACCESSIBILITY="1")
                portal_config = pathlib.Path(runtime, "config/xdg-desktop-portal/portals.conf")
                portal_config.parent.mkdir(parents=True)
                portal_config.write_text("[preferred]\ndefault=gtk\n")
            return subprocess.call(["dbus-run-session", "--", sys.executable, __file__,
                                    *sys.argv[1:], "--session"], env=env)
    root = pathlib.Path(__file__).resolve().parents[2]
    cef = "/app/extensions/chromium-native/cef" if args.flatpak else subprocess.check_output(
        [sys.executable, str(root / "packaging/scripts/fetch-cef.py")], text=True).strip()
    identifier = datetime.datetime.now().strftime("%Y%m%d-%H%M%S") + f"-{'flatpak-' if args.flatpak else ''}{args.backend}-{os.getpid()}"
    output = root / "build/native-chromium/runs" / identifier
    output.mkdir(parents=True, mode=0o700)
    if args.flatpak:
        deployed = {"installation": os.environ["FLATPAK_USER_DIR"]}
        for name in ("io.github.cheviiot.alcove", "io.github.cheviiot.alcove.ChromiumNative"):
            deployed[name] = subprocess.check_output(
                ["flatpak", "info", "--user", "--show-commit", name], text=True).strip()
        deployed["permissions"] = subprocess.check_output(["flatpak", "info", "--user",
            "--show-permissions", "io.github.cheviiot.alcove"], text=True)
        (output / "flatpak.json").write_text(json.dumps(deployed, indent=2))
    handler = functools.partial(FixtureHandler, directory=str(root / "tests/engine/pages"))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    compositor = None
    accessibility = None
    orca = None
    virtual_input = None
    with contextlib.nullcontext(os.environ["XDG_RUNTIME_DIR"]) as runtime:
        env = os.environ.copy()
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"):
            env.pop(key, None)
        env.update(XDG_RUNTIME_DIR=runtime, GDK_BACKEND=args.backend, GSETTINGS_BACKEND="memory",
                   XDG_CONFIG_HOME=runtime + "/config", XDG_DATA_HOME=runtime + "/data",
                   XDG_CACHE_HOME=runtime + "/cache", NO_AT_BRIDGE="0")
        env["LANGUAGE"] = "ru"
        env["LC_ALL"] = "ru_RU.UTF-8"
        env["ALCOVE_PROBE_LOCALEDIR"] = str(root / "build/native-chromium/locale")
        env["GDK_SCALE"] = str(args.scale)
        env["ADW_DEBUG_COLOR_SCHEME"] = "prefer-dark" if args.theme == "dark" else "prefer-light"
        launcher = [str(root / "build/cargo/debug/alcove-native-chromium")]
        worker = str(args.worker or root / "build/native-chromium/worker/alcove-cef-worker")
        if args.flatpak:
            worker = "/app/extensions/chromium-native/bin/alcove-cef-launch"
            # Only the diagnostic report directory is exposed. The installed
            # application/extension have no filesystem grants. State is private
            # inside the sandbox, and Zypak restricts renderer subprocesses.
            launcher = ["flatpak", "run", "--user", "--command=alcove-native-probe",
                "--filesystem=" + str(output),
                "--env=GDK_BACKEND=" + args.backend,
                "--env=GDK_SCALE=" + str(args.scale),
                "--env=ADW_DEBUG_COLOR_SCHEME=" + env["ADW_DEBUG_COLOR_SCHEME"],
                "--env=NO_AT_BRIDGE=0", "--env=LANGUAGE=ru", "--env=LC_ALL=ru_RU.UTF-8",
                "--env=ZYPAK_DEBUG=1",
                *(["--nosocket=x11", "--nosocket=fallback-x11"] if args.backend == "wayland"
                  else ["--nosocket=wayland", "--socket=x11"]),
                "io.github.cheviiot.alcove"]
        command = [*launcher,
            "--worker", worker,
            "--cef-root", cef, "--output", str(output / "app"),
            "--url", args.url or f"http://127.0.0.1:{server.server_port}/{'dropdowns.html' if args.dropdowns else 'multi-window.html' if args.accessibility_windows else 'popups.html' if args.popups else 'requests.html' if args.site_requests else 'index.html'}",
            "--seconds", str(args.seconds), "--gpu", str(args.gpu).lower(),
            "--real-site", str(bool(args.url)).lower(),
            "--layout", str(args.layout).lower(),
            "--native-accessibility", str(args.native_accessibility).lower()]
        if args.site_requests:
            command.extend(["--fake-media", "true"])
        try:
            if args.backend == "wayland":
                env["WAYLAND_DISPLAY"] = "alcove-probe"
                if args.compositor == "weston":
                    compositor_command = ["weston", "--backend=headless", "--renderer=gl",
                        f"--width={1280 * args.scale}", f"--height={900 * args.scale}",
                        f"--scale={args.scale}", "--socket=alcove-probe", "--no-config"]
                else:
                    compositor_command = ["mutter", "--headless", "--wayland", "--no-x11",
                        f"--virtual-monitor={1280 * args.scale}x{900 * args.scale}", "--wayland-display=alcove-probe"]
                with (output / "compositor.log").open("w") as compositor_log:
                    compositor = subprocess.Popen(compositor_command, env=env,
                        stdout=compositor_log, stderr=subprocess.STDOUT)
                for _ in range(100):
                    if pathlib.Path(runtime, "alcove-probe").exists(): break
                    if compositor.poll() is not None: raise RuntimeError("isolated Wayland compositor exited")
                    time.sleep(.05)
                else: raise RuntimeError("isolated Wayland socket was not created")
                if args.compositor == "mutter" and args.scale != 1:
                    configure_mutter_scale(env, args.scale, output)
                subprocess.run(["dbus-update-activation-environment", "WAYLAND_DISPLAY",
                    "XDG_RUNTIME_DIR", "GDK_BACKEND"], env=env, check=True)
            else:
                command = ["xvfb-run", "-a", "-s", "-screen 0 1600x1200x24", *command]
            if args.flatpak:
                # Resolve the outer accessibility bus before Flatpak starts.
                # Its /run/user path inside the sandbox is not a valid address
                # for observers in our private /tmp runtime directory.
                from gi.repository import Gio, GLib
                bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
                address, = bus.call_sync("org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus",
                    "GetAddress", None, GLib.VariantType.new("(s)"),
                    Gio.DBusCallFlags.NONE, 5000, None).unpack()
                env["AT_SPI_BUS_ADDRESS"] = address
            if args.compositor == "mutter" and args.backend == "wayland":
                with (output / "input.log").open("w") as input_log:
                    virtual_input = subprocess.Popen([sys.executable,
                        str(root / "tests/engine/mutter-input.py"),
                        *(["--keyboard-check"] if args.orca_keyboard else [])],
                        env=env, stdout=input_log, stderr=subprocess.STDOUT)
            if args.orca:
                # Exercise real Orca event handling and speech generation without
                # connecting to the user's speech/audio service or auto-spawning one.
                orca_env = env | {"SPEECHD_ADDRESS": "unix_socket:" + runtime + "/no-speech-service",
                                  "SPEECHD_CMD": "/bin/false"}
                with (output / "orca.log").open("w") as orca_log:
                    orca = subprocess.Popen(["orca", "--debug-file", str(output / "orca-debug.log")],
                        env=orca_env, stdout=orca_log, stderr=subprocess.STDOUT)
            with (output / "accessibility.log").open("w") as audit_log:
                audit_command = [sys.executable,
                    str(root / "tests/engine/accessibility-audit.py"),
                    "--output", str(output / "accessibility.json"),
                    "--delay", str(min(6, max(1, args.seconds - 3))),
                    *(["--navigate"] if args.accessibility_navigation else [])]
                if args.site_requests:
                    audit_command = [sys.executable, str(root / "tests/engine/site-requests-audit.py"),
                                     "--output", str(output / "site-requests.json")]
                if args.popups:
                    audit_command = [sys.executable, str(root / "tests/engine/popup-audit.py"),
                                     "--output", str(output / "popups.json"),
                                     *(["--native"] if args.native_accessibility else []),
                                     *(["--orca"] if args.orca else [])]
                if args.accessibility_windows:
                    audit_command = [sys.executable, str(root / "tests/engine/multi-window-audit.py"),
                                     "--output", str(output / "multi-window.json")]
                if args.dropdowns:
                    audit_command = [sys.executable, str(root / "tests/engine/dropdown-audit.py"),
                                     "--output", str(output / "dropdowns.json")]
                if args.url:
                    audit_command = [sys.executable, str(root / "tests/engine/real-site-audit.py"),
                        "--output", str(output / "real-site.json"), "--url", args.url,
                        "--scenario", args.site_scenario]
                accessibility = subprocess.Popen(audit_command,
                    env=env, stdout=audit_log, stderr=subprocess.STDOUT)
            with (output / "host.log").open("w") as log:
                result = subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT,
                                        timeout=args.seconds + 30 if args.seconds else None)
            print(output)
            if accessibility:
                try: accessibility.wait(timeout=3)
                except subprocess.TimeoutExpired: accessibility.kill(); accessibility.wait()
            orca_exit_before_shutdown = orca.poll() if orca else None
            if orca:
                # The installed reader buffers debug output. Finish it before
                # assessing speech so a valid final utterance cannot disappear
                # from the report merely because it was not flushed yet.
                orca.terminate()
                try: orca.wait(timeout=3)
                except subprocess.TimeoutExpired: orca.kill(); orca.wait()
            report_path = output / "app/report.json"
            if report_path.exists():
                report = json.loads(report_path.read_text())
                displayed = report["gpu_presented"] if args.gpu else report["cpu_frames"]
                checks = {"rendering": displayed > 0,
                    "scale": report["scale_factor"] == args.scale and report["frame_size"] == [v * args.scale for v in report["size"]]}
                if args.native_accessibility:
                    worker_log = output / "app/worker-stderr.log"
                    warnings = [line for line in worker_log.read_text(errors="replace").splitlines()
                                if "Atk-CRITICAL" in line or "g_object_weak_unref_cb" in line
                                or "g_object_unref: assertion" in line] if worker_log.exists() else []
                    report["native_accessibility_lifetime_warnings"] = warnings
                    checks["native_accessibility_lifetime"] = not warnings
                if args.layout:
                    states = report["layout_checks"]
                    checks["layout"] = (len(states) == 7 and states[1]["size"][0] == 360
                        and states[3]["maximized"] and states[5]["fullscreen"]
                        and not states[6]["fullscreen"] and not states[6]["maximized"])
                if args.native_accessibility and not args.site_requests and not args.popups and not args.accessibility_windows and not args.dropdowns and not args.url:
                    audit_path = output / "accessibility.json"
                    audit = json.loads(audit_path.read_text()) if audit_path.exists() else {}
                    interaction = audit.get("interactions", {})
                    events = {item["type"] for item in audit.get("events", [])}
                    event_log = (output / "app/events.jsonl").read_text()
                    checks["native_accessibility"] = (audit.get("document_embedded_in_gtk", False)
                        and interaction.get("text") == "Привет" and interaction.get("focused", False)
                        and interaction.get("caret_offset") == 2 and interaction.get("selection") == [1, 4]
                        and interaction.get("button_action_accepted", False)
                        and interaction.get("link_target_matches", False)
                        and "ALCOVE_CLICK_TRUSTED:true" in event_log
                        and {"object:text-caret-moved", "object:text-selection-changed", "object:text-changed:insert"} <= events)
                    if args.accessibility_navigation:
                        checks["accessibility_navigation"] = all(interaction.get(key, False) for key in (
                            "navigation_document_replaced", "previous_document_defunct",
                            "previous_button_action_rejected", "return_document_restored"))
                if args.orca:
                    speech_path = output / "orca-debug.log"
                    speech = [line for line in speech_path.read_text().splitlines()
                              if "SPEECH OUTPUT:" in line] if speech_path.exists() else []
                    report["orca_speech"] = speech
                    report["orca_exit_code_before_shutdown"] = orca_exit_before_shutdown
                    if args.popups:
                        # The provider navigates within the existing popup and
                        # autofocuses its button; Orca need not repeat its title.
                        # Check window identification plus actual web controls
                        # at every step, alongside the independent document audit.
                        checks["orca_popup_window_titles"] = all(any(title in line for line in speech) for title in ("Alcove popup fixture", "Alcove OAuth step"))
                        checks["orca_popup_controls"] = all(any(name in line for line in speech) for name in ("Начать проверку входа", "Перейти к провайдеру входа", "Подтвердить вход"))
                    elif args.accessibility_windows:
                        checks["orca_multiple_windows"] = sum("Одинаковая страница" in line for line in speech) >= 2
                    else:
                        checks["orca_document_and_input"] = any("Chromium внутри GTK" in line for line in speech) and any("Проверка ввода" in line for line in speech)
                    if args.orca_keyboard:
                        checks["orca_keyboard_button"] = (any("Проверить нажатие" in line for line in speech)
                            and (output / "app/events.jsonl").read_text().count("ALCOVE_CLICK_TRUSTED:true") == 2)
                if not args.surface_only and not args.site_requests and not args.popups and not args.accessibility_windows and not args.dropdowns and not args.url:
                    checks["unicode_commit"] = report["unicode_commit_confirmed"]
                    checks["webgl"] = report["webgl"]
                if args.site_requests:
                    audit_path = output / "site-requests.json"
                    audit = json.loads(audit_path.read_text()) if audit_path.exists() else {}
                    checks["site_requests"] = audit.get("passed", False)
                    checks["header_held_for_dialog"] = report.get("dialog_hold_observed", False) and not report.get("dialog_hold_failed", True)
                if args.popups:
                    audit_path = output / "popups.json"
                    audit = json.loads(audit_path.read_text()) if audit_path.exists() else {}
                    checks["popups"] = audit.get("passed", False)
                if args.accessibility_windows:
                    audit_path = output / "multi-window.json"
                    audit = json.loads(audit_path.read_text()) if audit_path.exists() else {}
                    checks["independent_native_accessibility_windows"] = audit.get("passed", False)
                if args.dropdowns:
                    audit_path = output / "dropdowns.json"
                    audit = json.loads(audit_path.read_text()) if audit_path.exists() else {}
                    checks["dropdowns"] = audit.get("passed", False)
                    checks["popup_surfaces"] = report["popup_surface"]["presented"] > 0 and not report["popup_surface"]["visible"]
                if args.url:
                    audit_path = output / "real-site.json"
                    audit = json.loads(audit_path.read_text()) if audit_path.exists() else {}
                    checks["real_site"] = audit.get("passed", False)
                    report["site_url"] = args.url
                    report["site_scenario"] = args.site_scenario
                checks["header_overlay"] = (report["autohide_observed"]
                    and report["header_visible_at_capture"] and report["overlay_preserved_viewport"])
                passed = (result.returncode == 0 and all(checks.values()) and not report["errors"])
                report.update(diagnostic_scope="real-site" if args.url else "dropdowns" if args.dropdowns else "multi-window-native-accessibility" if args.accessibility_windows else ("popups-native-accessibility" if args.native_accessibility else "popups-without-site-accessibility") if args.popups else "site-requests" if args.site_requests else "surface-only" if args.surface_only else "full-fixture",
                              compositor=args.compositor if args.backend == "wayland" else "xvfb",
                              deployment="flatpak" if args.flatpak else "distrobox",
                              diagnostic_checks=checks, diagnostic_passed=bool(passed))
                report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2))
                print(json.dumps(report, ensure_ascii=False, indent=2))
                return 0 if passed else 1
            return result.returncode or 1
        finally:
            if accessibility:
                try: accessibility.wait(timeout=3)
                except subprocess.TimeoutExpired: accessibility.kill(); accessibility.wait()
                audit = pathlib.Path(audit_command[audit_command.index("--output") + 1])
                if not audit.exists():
                    audit.write_text(json.dumps({"error": "AT-SPI audit did not complete",
                        "exit_code": accessibility.returncode}))
            if virtual_input:
                virtual_input.terminate()
                try: virtual_input.wait(timeout=3)
                except subprocess.TimeoutExpired: virtual_input.kill(); virtual_input.wait()
            if orca:
                orca.terminate()
                try: orca.wait(timeout=3)
                except subprocess.TimeoutExpired: orca.kill(); orca.wait()
            if compositor:
                compositor.terminate()
                try: compositor.wait(timeout=5)
                except subprocess.TimeoutExpired: compositor.kill(); compositor.wait()
            server.shutdown()

if __name__ == "__main__":
    raise SystemExit(main())
