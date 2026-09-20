#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Exercise the ordinary Alcove launch command in a disposable Wayland session.

The host runs without probe instrumentation. Interact through real AT-SPI
actions and the private compositor keyboard, including graceful window close.
"""
import collections
import datetime
import http.server
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import threading
import time
import traceback

ROOT = pathlib.Path(__file__).resolve().parents[2]


def main():
    if '--session' not in sys.argv:
        with tempfile.TemporaryDirectory(prefix='alcove-launch-') as runtime:
            env = os.environ.copy()
            for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'AT_SPI_BUS_ADDRESS',
                        'DBUS_SESSION_BUS_ADDRESS', 'DBUS_STARTER_ADDRESS', 'DBUS_STARTER_BUS_TYPE'):
                env.pop(key, None)
            env.update(XDG_RUNTIME_DIR=runtime, XDG_DATA_HOME=runtime + '/data',
                       XDG_CONFIG_HOME=runtime + '/config', XDG_CACHE_HOME=runtime + '/cache',
                       GSETTINGS_BACKEND='memory', NO_AT_BRIDGE='0', GDK_BACKEND='wayland',
                       WAYLAND_DISPLAY='alcove-launch', LANGUAGE='ru', LC_ALL='ru_RU.UTF-8',
                       ALCOVE_TEST_RESOURCE=str(ROOT / 'build/src/alcove.gresource'),
                       GSETTINGS_SCHEMA_DIR=str(ROOT / 'build/data'),
                       ALCOVE_NATIVE_CHROMIUM_ADDON=str(ROOT / 'build/native-chromium/addon.json'))
            settings = pathlib.Path(runtime, 'config/gtk-4.0/settings.ini')
            settings.parent.mkdir(parents=True)
            settings.write_text('[Settings]\ngtk-xft-dpi=98304\n')
            return subprocess.call(['dbus-run-session', '--', sys.executable, __file__, *sys.argv[1:], '--session'], env=env)

    runtime = pathlib.Path(os.environ['XDG_RUNTIME_DIR'])
    assert runtime.name.startswith('alcove-launch-') and os.environ['WAYLAND_DISPLAY'] == 'alcove-launch'
    policy_mode = '--policy' in sys.argv
    background_mode = '--background' in sys.argv
    output = ROOT / 'build/native-chromium/runs' / (datetime.datetime.now().strftime('%Y%m%d-%H%M%S') + ('-background' if background_mode else '-policy' if policy_mode else '-launch'))
    output.mkdir(parents=True)
    result = {'checks': {}, 'snapshots': {}}
    processes = []
    compositor = None

    class Fixture(http.server.BaseHTTPRequestHandler):
        page_requests = 0
        recover = False
        proxy_requests = []
        target_requests = 0
        posts = []
        visibility = []

        def do_POST(self):
            Fixture.posts.append(self.rfile.read(int(self.headers.get('Content-Length', 0))).decode())
            self.do_GET()

        def do_GET(self):
            if self.path.startswith('/visibility?'):
                Fixture.visibility.append(self.path.split('?', 1)[1])
                self.send_response(204)
                self.end_headers()
                return
            if self.path == '/redirect':
                self.send_response(302)
                self.send_header('Location', f'http://localhost:{server.server_port}/target')
                self.end_headers()
                return
            if self.path == '/target':
                Fixture.target_requests += 1
            if self.path.startswith('http://alcove-proxy.invalid/'):
                Fixture.proxy_requests.append(self.path)
            if self.path == '/recover' and not Fixture.recover:
                self.close_connection = True
                return
            if self.path == '/':
                Fixture.page_requests += 1
            body = '''<!doctype html><html lang="ru"><meta charset="utf-8"><title>CEF App Storage</title>
            <h1>Проверка приложения</h1><p id="state"></p><button id="save">Сохранить</button>
            <p id="agent"></p><button id="permission">Запросить уведомления</button><p id="permission-result"></p><p id="initial-permission"></p><p id="initial-location"></p><script>
            const update=()=>document.querySelector('#state').textContent=localStorage.saved?'Сохранено':'Пусто';
            document.querySelector('#save').onclick=()=>{localStorage.saved='yes';update()};update();
            document.querySelector('#agent').textContent=navigator.userAgent;
            document.querySelector('#permission').onclick=async()=>document.querySelector('#permission-result').textContent='Permission: '+await Notification.requestPermission();
            document.querySelector('#initial-permission').textContent='Initial permission: '+Notification.permission;
            navigator.permissions.query({name:'geolocation'}).then(p=>document.querySelector('#initial-location').textContent='Initial location: '+p.state);
            const visibility=()=>fetch('/visibility?'+document.visibilityState);
            document.addEventListener('visibilitychange',visibility);visibility();
            </script></html>'''
            other = f'http://localhost:{server.server_port}/target'
            home = f'http://127.0.0.1:{server.server_port}/'
            body += f'<a href="{other}">Другой сайт</a><a href="/redirect">Перенаправление</a><a href="{home}">На начальный сайт</a>'
            body += f'<form action="{other}" method="post"><input type="hidden" name="fixture" value="retained"><button>Отправить форму</button></form>'
            body += f'<button onclick="window.open(\'{other}\',\'alcove-policy-popup\')">Открыть дочернее окно</button>'
            body += '<button onclick="setTimeout(()=>alert(\'Сообщение из фона\'),2000)">Сообщение позже</button>'
            body += '<button onclick="window.onbeforeunload=e=>{e.preventDefault();e.returnValue=\'\'}">Подтверждать закрытие</button>'
            if self.path == '/target':
                body += '<h2>Целевой сайт открыт</h2>'
            elif self.path == '/':
                body += '<h2>Начальный сайт</h2>'
            body = body.encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    data = runtime / 'data/alcove'

    def config(identifier, url, title):
        path = data / 'apps' / identifier / 'app.json'
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps(dict(schema_version=3, id=identifier, title=title,
            start_url=url, engine='chromium', use_theme_color=False,
            user_agent='Alcove-Native-Launch-Audit/1.0' if url.startswith('http:') else None,
            window=dict(width=800, height=640, maximized=False))))

    def progress(stage):
        result['stage'] = stage
        (output / 'launch.json').write_text(json.dumps(result, ensure_ascii=False, indent=2))
        print(stage, flush=True)

    try:
        with (output / 'compositor.log').open('w') as log:
            compositor = subprocess.Popen(['mutter', '--headless', '--wayland', '--no-x11',
                '--virtual-monitor=1280x900', '--wayland-display=alcove-launch'], stdout=log, stderr=subprocess.STDOUT)
        for _ in range(100):
            if (runtime / 'alcove-launch').exists():
                break
            if compositor.poll() is not None:
                raise RuntimeError('private compositor exited')
            time.sleep(.05)
        else:
            raise RuntimeError('private Wayland socket missing')
        subprocess.run(['dbus-update-activation-environment', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR',
                        'GDK_BACKEND', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'NO_AT_BRIDGE'], check=True)
        import pyatspi
        from gi.repository import Gio, GLib
        pyatspi.Registry.registerEventListener(lambda e: None, 'object', 'window')
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        service = 'org.gnome.Mutter.RemoteDesktop'
        session, = bus.call_sync(service, '/org/gnome/Mutter/RemoteDesktop', service,
            'CreateSession', None, GLib.VariantType.new('(o)'), Gio.DBusCallFlags.NONE, 3000, None).unpack()

        def call(method, signature=None, values=()):
            return bus.call_sync(service, session, service + '.Session', method,
                GLib.Variant(signature, values) if signature else None, None, Gio.DBusCallFlags.NONE, 3000, None)

        call('Start')
        # Mutter creates virtual devices lazily on the first input event.
        # Supply a keyboard seat before presenting the first GTK window.
        call('NotifyKeyboardKeysym', '(ub)', (0xffe1, True))
        call('NotifyKeyboardKeysym', '(ub)', (0xffe1, False))

        def settle():
            for _ in range(100):
                if not GLib.MainContext.default().pending():
                    break
                GLib.MainContext.default().iteration(False)
            time.sleep(.1)

        def wait(predicate, description, seconds=20):
            until = time.monotonic() + seconds
            while time.monotonic() < until:
                try:
                    value = predicate()
                except GLib.Error as error:
                    if not any(s in str(error) for s in ('does not exist', 'no longer exists', 'не существует')):
                        raise
                    value = None  # A navigation legitimately retires old AT-SPI nodes.
                if value:
                    return value
                settle()
            raise AssertionError(description)

        def nodes():
            pending = collections.deque([pyatspi.Registry.getDesktop(0)])
            count = 0
            while pending and count < 1600:
                node = pending.popleft()
                count += 1
                try:
                    yield node
                    pending.extend(c for c in node if c)
                except GLib.Error:
                    pass

        def find(name, role=None):
            return wait(lambda: next((n for n in nodes() if n.name == name and
                (role is None or n.getRoleName() == role)), None), 'accessible content: ' + name)

        def snapshot(name):
            result['snapshots'][name] = [dict(name=n.name, role=n.getRoleName()) for n in nodes()][:100]
            progress(name)

        def launch(identifier, background=False):
            with (output / f'{identifier}-{len(processes)}.log').open('w') as log:
                proc = subprocess.Popen([str(ROOT / 'target/debug/alcove'), *(['--start-background'] if background else []), identifier],
                    stdout=log, stderr=subprocess.STDOUT)
            processes.append(proc)
            return proc

        def descendants(proc):
            pending = [proc.pid]
            while pending:
                parent = pending.pop()
                try:
                    children = set()
                    # Chromium can launch processes from non-main threads.
                    for path in pathlib.Path(f'/proc/{parent}/task').glob('*/children'):
                        children.update(path.read_text().split())
                except FileNotFoundError:
                    continue
                for child in map(int, children):
                    pending.append(child)
                    try:
                        yield child, pathlib.Path(f'/proc/{child}/cmdline').read_bytes().split(b'\0')
                    except FileNotFoundError:
                        pass

        def close(proc):
            # These keys reach only the private compositor, never the desktop.
            for sym, pressed in ((0xffe9, True), (0xffc1, True), (0xffc1, False), (0xffe9, False)):
                call('NotifyKeyboardKeysym', '(ub)', (sym, pressed))
                time.sleep(.08)
            wait(lambda: proc.poll() is not None, 'ordinary application exited after window close', 10)
            assert proc.returncode == 0, proc.returncode
            wait(lambda: not list(runtime.glob('alcove-cef-*')), 'temporary IPC resources removed')

        def click(name):
            button = wait(lambda: next((n for n in nodes() if n.name == name and
                'button' in n.getRoleName()), None), 'button: ' + name)
            assert button.queryAction().doAction(0)
            if name in ('Блокировать', 'Открыть один раз', 'Всегда разрешать origin',
                        'Всегда разрешать', 'Всегда блокировать', 'Не сейчас', 'Разрешить на эту сессию'):
                def dismissed():
                    try:
                        return not button.getState().contains(pyatspi.STATE_SHOWING)
                    except GLib.Error as error:
                        if any(s in str(error) for s in ('does not exist', 'no longer exists', 'не существует')):
                            return True
                        raise
                wait(dismissed, 'native dialog dismissed', 5)

        if background_mode:
            # A recorder on this private bus receives the real GNotification
            # payload. No host notification service or desktop is contacted.
            notifications = {}
            notification_events = []
            info = Gio.DBusNodeInfo.new_for_xml("""<node><interface name="org.gtk.Notifications">
              <method name="AddNotification"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/></method>
              <method name="RemoveNotification"><arg type="s" direction="in"/><arg type="s" direction="in"/></method>
            </interface></node>""")
            def notification_call(connection, sender, path, interface, method, parameters, invocation):
                values = parameters.unpack()
                key = (values[0], values[1])
                notification_events.append(dict(method=method, app=values[0], id=values[1]))
                if method == 'AddNotification':
                    notifications[key] = values[2]
                else:
                    notifications.pop(key, None)
                invocation.return_value(None)
            registration = bus.register_object('/org/gtk/Notifications', info.interfaces[0], notification_call, None, None)
            bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
                'RequestName', GLib.Variant('(su)', ('org.gtk.Notifications', 0)), None, Gio.DBusCallFlags.NONE, 3000, None)
            identifier = 'cefbackgrond'
            app_id = 'io.github.cheviiot.alcove.' + identifier
            notification_key = (app_id, 'background-' + identifier)
            def app_action(action):
                bus.call_sync(app_id, '/' + app_id.replace('.', '/'), 'org.gtk.Actions', 'Activate',
                    GLib.Variant('(sava{sv})', (action, [GLib.Variant('s', identifier)], {})),
                    None, Gio.DBusCallFlags.NONE, 3000, None)
            def visible_window():
                return any(n.getRoleName() == 'frame' and n.getApplication().name == 'alcove'
                           and n.getState().contains(pyatspi.STATE_SHOWING) for n in nodes())
            def hidden():
                return Fixture.visibility and Fixture.visibility[-1] == 'hidden' and not visible_window()
            def request_close():
                for sym, pressed in ((0xffe9, True), (0xffc1, True), (0xffc1, False), (0xffe9, False)):
                    call('NotifyKeyboardKeysym', '(ub)', (sym, pressed))
                    time.sleep(.08)
            def stopped(proc):
                wait(lambda: proc.poll() is not None, 'background app exited', 10)
                assert proc.returncode == 0, proc.returncode
                wait(lambda: not list(runtime.glob('alcove-cef-*')), 'background IPC removed')
                wait(lambda: notification_key not in notifications, 'background notification removed')
            url = f'http://127.0.0.1:{server.server_port}/'
            config(identifier, url, 'Background App')
            disabled = launch(identifier, background=True)
            wait(lambda: disabled.poll() is not None, 'unapproved autostart exits')
            assert disabled.returncode == 0 and not list(runtime.glob('alcove-cef-*'))
            assert Fixture.page_requests == 0
            result['checks']['autostart_requires_saved_authorization'] = True
            policy_path = data / 'apps' / identifier / 'policy.json'
            policy_path.write_text(json.dumps(dict(schema_version=2, background=dict(enabled=True, autostart=True))))
            proc = launch(identifier, background=True)
            wait(hidden, 'CEF starts hidden and reports hidden visibility')
            assert proc.poll() is None and len(list(runtime.glob('alcove-cef-*'))) == 1
            result['checks']['background_launch_without_visible_window'] = True
            payload = wait(lambda: notifications.get(notification_key), 'background notification delivered')
            assert payload['default-action'] == 'app.show-background'
            assert payload['buttons'][0]['action'] == 'app.stop-background'
            result['checks']['native_background_notification_actions'] = True
            repeat = launch(identifier)
            wait(lambda: repeat.poll() is not None, 'repeated launch returns')
            assert repeat.returncode == 0
            find('Пусто')
            wait(lambda: notification_key not in notifications, 'notification withdrawn on presentation')
            assert visible_window() and Fixture.page_requests == 1
            result['checks']['relaunch_reveals_same_page'] = True
            click('Сохранить')
            find('Сохранено')
            request_close()
            wait(hidden, 'window close keeps website hidden and alive')
            assert proc.poll() is None and Fixture.page_requests == 1
            wait(lambda: notification_key in notifications, 'notification restored after hiding')
            result['checks']['close_retains_site_in_background'] = True
            app_action('show-background')
            find('Сохранено')
            wait(lambda: notification_key not in notifications, 'show action withdraws notification')
            assert Fixture.page_requests == 1
            result['checks']['notification_show_restores_same_page'] = True
            request_close()
            wait(hidden, 'background before stop')
            app_action('stop-background')
            stopped(proc)
            result['checks']['notification_stop_exits_and_releases_profile'] = True
            proc = launch(identifier)
            find('Сохранено')
            click('Сообщение позже')
            request_close()
            wait(hidden, 'background before website dialog')
            find('Сообщение из фона')
            wait(lambda: notification_key not in notifications, 'dialog reveals background window')
            assert visible_window()
            click('ОК')
            result['checks']['website_dialog_reveals_background_window'] = True
            click('Подтверждать закрытие')
            request_close()
            wait(hidden, 'background before before-unload')
            app_action('stop-background')
            find('Остаться')
            assert visible_window()
            click('Остаться')
            find('Сохранено')
            assert proc.poll() is None
            result['checks']['stop_confirmation_can_cancel_without_losing_site'] = True
            # The existing native menu must stop a foreground background-enabled
            # app, rather than merely hiding it again.
            for sym in (0xffc7, 0x20):  # F10, Space
                call('NotifyKeyboardKeysym', '(ub)', (sym, True))
                call('NotifyKeyboardKeysym', '(ub)', (sym, False))
                settle()
            item = find('Остановить фоновую работу')
            while item.getRoleName() != 'menu item':
                item = item.parent
                assert item is not None
            assert item.queryAction().doAction(0)
            click('Покинуть')
            stopped(proc)
            result['checks']['menu_stop_exits_and_storage_survives_restart'] = True
            Fixture.visibility.clear()
            proc = launch(identifier, background=True)
            wait(hidden, 'background before worker failure')
            workers = [pid for pid, argv in descendants(proc) if argv and
                pathlib.Path(os.fsdecode(argv[0]).split(' ')[0]).name == 'alcove-cef-worker'
                and not any(b'--type=' in arg for arg in argv)]
            assert len(workers) == 1
            os.kill(workers[0], signal.SIGKILL)
            find('Chromium завершил работу')
            wait(lambda: notification_key not in notifications, 'failed background session withdrawn')
            assert visible_window()
            result['checks']['worker_failure_reveals_error_and_releases_background_hold'] = True
            close(proc)
            result['notification_events'] = notification_events
            progress('passed')
            bus.unregister_object(registration)
            return 0

        if policy_mode:
            url = f'http://127.0.0.1:{server.server_port}/'
            origin = url.rstrip('/')
            config('cefpolicyask', url, 'Permissions')
            proc = launch('cefpolicyask')
            click('Запросить уведомления')
            find('Разрешение сайта')
            click('Всегда разрешать')
            find('Permission: granted')
            policy_path = data / 'apps/cefpolicyask/policy.json'
            saved = json.loads(policy_path.read_text())
            assert saved['permissions'][origin]['notifications'] == 'allow'
            result['checks']['always_allow_saved_in_alcove'] = True
            close(proc)
            saved['permissions'][origin]['geolocation'] = 'allow'
            policy_path.write_text(json.dumps(saved))
            proc = launch('cefpolicyask')
            find('Initial permission: granted')
            find('Initial location: granted')
            result['checks']['saved_permissions_visible_to_site_on_startup'] = True
            close(proc)
            saved['permissions'][origin]['notifications'] = 'block'
            saved['permissions'][origin]['geolocation'] = 'block'
            policy_path.write_text(json.dumps(saved))
            proc = launch('cefpolicyask')
            find('Initial permission: denied')
            find('Initial location: denied')
            click('Запросить уведомления')
            find('Permission: denied')
            result['checks']['saved_block_overrides_previous_allow'] = True
            close(proc)
            config('cefpolicytmp', url, 'Session Permission')
            proc = launch('cefpolicytmp')
            click('Запросить уведомления')
            find('Разрешение сайта')
            click('Разрешить на эту сессию')
            find('Permission: granted')
            close(proc)
            proc = launch('cefpolicytmp')
            click('Запросить уведомления')
            find('Разрешение сайта')
            click('Не сейчас')
            find('Permission: default')
            result['checks']['session_permission_expires_on_restart'] = True
            click('Запросить уведомления')
            find('Разрешение сайта')
            click('Всегда блокировать')
            find('Permission: denied')
            assert json.loads((data / 'apps/cefpolicytmp/policy.json').read_text())['permissions'][origin]['notifications'] == 'block'
            result['checks']['not_now_can_prompt_again_and_block_is_saved'] = True
            close(proc)
            config('cefpolicyprx', 'http://alcove-proxy.invalid/proxy-test', 'Proxy')
            policy_path = data / 'apps/cefpolicyprx/policy.json'
            policy_path.write_text(json.dumps(dict(schema_version=2, proxy=dict(mode='custom',uri=origin))))
            proc = launch('cefpolicyprx')
            find('Проверка приложения')
            assert 'http://alcove-proxy.invalid/proxy-test' in Fixture.proxy_requests
            result['checks']['configured_proxy_routes_requests'] = True
            close(proc)
            config('cefpolicynav', url, 'Navigation')
            nav_path = data / 'apps/cefpolicynav/policy.json'
            nav_path.write_text(json.dumps(dict(schema_version=2,
                navigation=dict(enabled=True,allowed_origins=[origin]))))
            proc = launch('cefpolicynav')

            def link(name):
                assert find(name, 'link').queryAction().doAction(0)

            link('Другой сайт')
            find('Открыть другой origin?')
            assert Fixture.target_requests == 0
            click('Блокировать')
            settle()
            assert Fixture.target_requests == 0
            result['checks']['navigation_blocked_before_network'] = True
            link('Перенаправление')
            find('Открыть другой origin?')
            assert Fixture.target_requests == 0
            click('Открыть один раз')
            find('Целевой сайт открыт')
            result['checks']['redirect_requires_decision'] = True
            link('На начальный сайт')
            find('Начальный сайт')
            click('Отправить форму')
            find('Открыть другой origin?')
            assert not Fixture.posts
            click('Открыть один раз')
            wait(lambda: Fixture.posts == ['fixture=retained'], 'original POST body preserved')
            find('Целевой сайт открыт')
            result['checks']['navigation_preserves_post'] = True
            link('На начальный сайт')
            find('Начальный сайт')
            before_popup = Fixture.target_requests
            click('Открыть дочернее окно')
            find('Открыть другой origin?')
            assert Fixture.target_requests == before_popup
            click('Блокировать')
            for sym, pressed in ((0xffe9, True), (0xffc1, True), (0xffc1, False), (0xffe9, False)):
                call('NotifyKeyboardKeysym', '(ub)', (sym, pressed))
                time.sleep(.08)
            wait(lambda: sum(n.getRoleName() == 'frame' and n.getApplication().name == 'alcove' for n in nodes()) == 1,
                 'blocked child window closed while parent remains')
            assert proc.poll() is None and Fixture.target_requests == before_popup
            result['checks']['popup_obeys_same_navigation_policy'] = True
            link('Другой сайт')
            find('Открыть другой origin?')
            click('Всегда разрешать origin')
            find('Целевой сайт открыт')
            assert f'http://localhost:{server.server_port}' in json.loads(nav_path.read_text())['navigation']['allowed_origins']
            result['checks']['navigation_allowlist_saved'] = True
            close(proc)
            proc = launch('cefpolicynav')
            link('Другой сайт')
            find('Целевой сайт открыт')
            result['checks']['saved_navigation_allowlist_applies_on_restart'] = True
            close(proc)
            progress('passed')
            return 0

        url = f'http://127.0.0.1:{server.server_port}/'
        config('ceflaunchone', url, 'Storage One')
        config('ceflaunchtwo', url, 'Storage Two')
        config('ceflaunchweb', 'https://www.wikipedia.org/', 'Wikipedia App')
        profile = data / 'profiles/ceflaunchone'
        profile.mkdir(parents=True)
        marker = profile / 'existing-webkit-marker'
        marker.write_text('unchanged')
        proc = launch('ceflaunchone')
        find('Пусто')
        find('Alcove-Native-Launch-Audit/1.0')
        result['checks']['configured_user_agent'] = True
        snapshot('fixture-loaded')
        button = wait(lambda: next((n for n in nodes() if n.name == 'Сохранить' and
            'button' in n.getRoleName()), None), 'save button')
        assert button.queryAction().doAction(0)
        find('Сохранено')
        result['checks']['site_interaction'] = True
        requests_before = Fixture.page_requests
        call('NotifyKeyboardKeysym', '(ub)', (0xffc2, True))  # F5 with website focus.
        call('NotifyKeyboardKeysym', '(ub)', (0xffc2, False))
        wait(lambda: Fixture.page_requests > requests_before, 'F5 reloads website')
        result['checks']['keyboard_reload_from_site'] = True
        snapshot('saved')
        repeat = subprocess.run([str(ROOT / 'target/debug/alcove'), 'ceflaunchone'],
            capture_output=True, timeout=10)
        assert repeat.returncode == 0 and proc.poll() is None
        assert len(list(runtime.glob('alcove-cef-*'))) == 1
        result['checks']['relaunch_presents_existing_window'] = True
        assert not list(runtime.glob('alcove-cef-*/events.jsonl'))
        assert not list(runtime.glob('alcove-cef-*/*.png'))
        result['checks']['no_probe_recording'] = True
        close(proc)
        result['checks']['graceful_close_and_ipc_cleanup'] = True
        result['profile_directories'] = [str(p.relative_to(profile)) for p in profile.rglob('*')
            if p.is_dir() and len(p.relative_to(profile).parts) < 4]
        progress('closed')
        assert any(p.is_dir() for p in (profile / 'chromium-cef').rglob('Local Storage'))
        assert marker.read_text() == 'unchanged'
        result['checks']['separate_cef_profile'] = True
        proc = launch('ceflaunchone')
        find('Сохранено')
        result['checks']['storage_survives_restart'] = True
        close(proc)
        proc = launch('ceflaunchtwo')
        find('Пусто')
        result['checks']['apps_have_independent_storage'] = True
        close(proc)
        config('ceflaunchbad', url + 'recover', 'Unavailable Site')
        proc = launch('ceflaunchbad')
        find('Не удалось загрузить сайт')
        Fixture.recover = True
        retry = wait(lambda: next((n for n in nodes() if 'button' in n.getRoleName()
            and n.name in ('Reload', 'Перезагрузить')), None), 'native retry button')
        assert retry.queryAction().doAction(0)
        find('Пусто')
        result['checks']['network_failure_native_retry'] = True
        # Crash only this disposable app's renderer/worker processes.
        children = list(descendants(proc))
        result['child_processes'] = [dict(pid=pid, executable=os.fsdecode(argv[0]).split(' ')[0],
            renderer=any(b'--type=renderer' in arg for arg in argv)) for pid, argv in children]
        progress('before-renderer-crash')
        renderers = [pid for pid, argv in children if any(b'--type=renderer' in arg for arg in argv)]
        assert renderers, 'fixture renderer exists'
        for pid in renderers:
            os.kill(pid, signal.SIGKILL)
        find('Сайт перестал отвечать')
        retry = wait(lambda: next((n for n in nodes() if 'button' in n.getRoleName()
            and n.name in ('Reload', 'Перезагрузить')), None), 'renderer retry button')
        assert retry.queryAction().doAction(0)
        find('Пусто')
        result['checks']['renderer_crash_native_retry'] = True
        workers = [pid for pid, argv in descendants(proc) if argv and
            pathlib.Path(os.fsdecode(argv[0]).split(' ')[0]).name == 'alcove-cef-worker'
            and not any(b'--type=' in arg for arg in argv)]
        assert len(workers) == 1
        os.kill(workers[0], signal.SIGKILL)
        find('Chromium завершил работу')
        result['checks']['worker_crash_native_error'] = True
        close(proc)
        proc = launch('ceflaunchbad')
        find('Пусто')
        result['checks']['reopen_after_worker_crash'] = True
        close(proc)
        proc = launch('ceflaunchweb')
        wait(lambda: next((n for n in nodes() if n.getRoleName() == 'document web'
            and 'Wikipedia' in n.name and n.getApplication().name == 'alcove'), None), 'real site in Alcove')
        wait(lambda: next((n for n in nodes() if n.getRoleName() == 'link'
            and n.name.startswith('English')), None), 'Wikipedia language link')
        snapshot('wikipedia')
        result['checks']['real_site_in_ordinary_app'] = True
        close(proc)
        result['checks']['native_wayland'] = 'DISPLAY' not in os.environ
        progress('passed')
        call('Stop')
        return 0
    except Exception:
        result['error'] = traceback.format_exc()
        result['fixture_observations'] = dict(target_requests=Fixture.target_requests, posts=Fixture.posts)
        try:
            snapshot('failure')
        except Exception:
            pass
        progress('failed')
        print(result['error'], file=sys.stderr)
        return 1
    finally:
        for proc in reversed(processes):
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
        if compositor is not None and compositor.poll() is None:
            compositor.terminate()
            compositor.wait(timeout=5)
        server.shutdown()
        print(output, flush=True)


if __name__ == '__main__':
    raise SystemExit(main())
