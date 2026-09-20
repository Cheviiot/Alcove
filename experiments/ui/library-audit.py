#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Check the real manager through AT-SPI and a private Mutter keyboard seat."""
import collections
import contextlib
import datetime
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import traceback

ROOT = Path(__file__).resolve().parents[2]


def main():
    if '--session' not in sys.argv:
        with contextlib.ExitStack() as cleanup:
            if '--orca' in sys.argv:
                lock_path=ROOT/'build/native-chromium/orca-test.lock'
                lock_path.parent.mkdir(parents=True,exist_ok=True)
                lock=cleanup.enter_context(lock_path.open('a'))
                fcntl.flock(lock,fcntl.LOCK_EX)
            runtime=cleanup.enter_context(tempfile.TemporaryDirectory(prefix='bastle-library-'))
            (Path(runtime)/'bin').mkdir()
            env = os.environ.copy()
            for name in ('DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'AT_SPI_BUS_ADDRESS',
                         'DBUS_SESSION_BUS_ADDRESS', 'DBUS_STARTER_ADDRESS', 'DBUS_STARTER_BUS_TYPE'):
                env.pop(name, None)
            env.update(XDG_RUNTIME_DIR=runtime, XDG_DATA_HOME=runtime+'/data',
                       XDG_CONFIG_HOME=runtime+'/config', XDG_CACHE_HOME=runtime+'/cache',
                       WAYLAND_DISPLAY='bastle-library', GDK_BACKEND='wayland', GTK_USE_PORTAL='1',
                       GSETTINGS_BACKEND='memory', NO_AT_BRIDGE='0',
                       XDG_DATA_DIRS='/usr/local/share:/usr/share',
                       LANGUAGE='ru', LC_ALL='ru_RU.UTF-8',
                       BASTLE_TEST_RESOURCE=str(ROOT/'build/src/bastle.gresource'),
                       BASTLE_TEST_LOCALEDIR=str(ROOT/'build/po'),
                       GSETTINGS_SCHEMA_DIR=str(ROOT/'build/data'))
            env['PATH']=runtime+'/bin:'+env.get('PATH','')
            settings = Path(runtime, 'config/gtk-4.0/settings.ini')
            settings.parent.mkdir(parents=True)
            settings.write_text('[Settings]\ngtk-xft-dpi=98304\n')
            try:
                return subprocess.call(['dbus-run-session', '--', sys.executable, __file__,
                    *sys.argv[1:], '--session'], env=env)
            finally:
                mount=Path(runtime,'doc')
                deadline=time.monotonic()+5
                while mount.is_mount() and time.monotonic()<deadline: time.sleep(.05)
                if mount.is_mount(): subprocess.run(['fusermount3','-u',str(mount)],check=True,timeout=5)
    runtime = Path(os.environ['XDG_RUNTIME_DIR'])
    assert runtime.name.startswith('bastle-library-') and os.environ['WAYLAND_DISPLAY'] == 'bastle-library'
    suffix='utilities' if '--utilities' in sys.argv else 'policy' if '--policy' in sys.argv else 'settings' if '--settings' in sys.argv else 'creation' if '--creation' in sys.argv else 'library'
    output = ROOT/'build/ui-runs'/datetime.datetime.now().strftime('%Y%m%d-%H%M%S-'+suffix)
    output.mkdir(parents=True)
    class Checks(dict):
        def __setitem__(self,key,value):
            super().__setitem__(key,value)
            print(f'{key}: {value}',flush=True)
    result = {'checks': Checks()}
    compositor = app = orca = None
    try:
        for index, title, url in [(0,'Альфа','https://alpha.example'),
                                  (1,'Бета','https://beta.example'),
                                  (2,'Очень длинное название приложения для проверки узкого окна','https://long.example')]:
            identifier = f'library{index:05}'
            directory = runtime/'data/bastle/apps'/identifier
            directory.mkdir(parents=True)
            (directory/'app.json').write_text(json.dumps(dict(schema_version=3, id=identifier,
                title=title, start_url=url, engine='webkit', sort_order=index*10, use_theme_color=True,
                window=dict(width=800,height=640,maximized=False)), ensure_ascii=False))
        if '--utilities' in sys.argv:
            import struct, zlib
            def chunk(kind,data):
                return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data))
            icon=(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!2I5B',32,32,8,2,0,0,0))+
                chunk(b'IDAT',zlib.compress((b'\0'+bytes([0,180,90])*32)*32))+chunk(b'IEND',b''))
            for directory in (runtime/'data/bastle/apps').iterdir():
                (directory/'icon.png').write_bytes(icon)
            invalid = runtime/'data/bastle/apps/invalid00000'
            invalid.mkdir()
            (invalid/'app.json').write_text('{invalid JSON')
            (runtime/'bin/bastle').symlink_to(ROOT/'build/src/bastle')
        if '--policy' in sys.argv:
            policy=dict(schema_version=2, permissions={'https://alpha.example':{'camera':'ask','notifications':'allow'}},
                navigation=dict(enabled=False,allowed_origins=[]),proxy=dict(mode='system',uri=None),
                background=dict(enabled=False,autostart=False),content_filters={'policyfilter':dict(name='Проверочный фильтр',enabled=True,
                    source=[{'trigger':{'url-filter':'.*tracker.*'},'action':{'type':'block'}}])})
            (runtime/'data/bastle/apps/library00000/policy.json').write_text(json.dumps(policy))
        with (output/'mutter.log').open('w') as log:
            compositor = subprocess.Popen(['mutter','--headless','--wayland','--no-x11',
                '--virtual-monitor=1280x900','--wayland-display=bastle-library'], stdout=log,stderr=subprocess.STDOUT)
        for _ in range(100):
            if (runtime/'bastle-library').exists(): break
            if compositor.poll() is not None: raise RuntimeError('Mutter failed')
            time.sleep(.05)
        else: raise RuntimeError('private Wayland socket not created')
        subprocess.run(['dbus-update-activation-environment','WAYLAND_DISPLAY','XDG_RUNTIME_DIR',
            'GDK_BACKEND','XDG_DATA_HOME','XDG_CONFIG_HOME','NO_AT_BRIDGE'],check=True)
        import pyatspi
        from gi.repository import Gio, GLib
        pyatspi.Registry.registerEventListener(lambda event: None, 'object', 'window')
        bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
        service='org.gnome.Mutter.RemoteDesktop'
        session,=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,
            GLib.VariantType.new('(o)'),Gio.DBusCallFlags.NONE,3000,None).unpack()
        def call(method, signature=None, values=()):
            return bus.call_sync(service,session,service+'.Session',method,
                GLib.Variant(signature,values) if signature else None,None,Gio.DBusCallFlags.NONE,3000,None)
        call('Start')
        def key(code,pressed): call('NotifyKeyboardKeysym','(ub)',(code,pressed))
        def press(code): key(code,True);time.sleep(.06);key(code,False)
        press(0xffe1)  # Create the keyboard seat before the first window maps.
        def settle():
            context=GLib.MainContext.default()
            for _ in range(100):
                if not context.pending(): break
                context.iteration(False)
            time.sleep(.08)
        def wait(predicate, description, seconds=10):
            deadline=time.monotonic()+seconds
            while time.monotonic()<deadline:
                try: value=predicate()
                except GLib.Error: value=None
                if value: return value
                settle()
            raise AssertionError(description)
        def nodes():
            pending=collections.deque([pyatspi.Registry.getDesktop(0)])
            while pending:
                node=pending.popleft()
                try:
                    node.clear_cache_single()
                    yield node
                    pending.extend(child for child in node if child)
                except GLib.Error: pass
        def snapshot():
            # GTK retires accessible objects during navigation and list
            # rebinding. Skip retired nodes in this diagnostic tree only.
            items=[]
            for node in nodes():
                try: items.append(dict(name=node.name,role=node.getRoleName()))
                except GLib.Error: pass
            return items
        def find(name, role=None):
            return wait(lambda: next((n for n in nodes() if n.name==name and
                (role is None or n.getRoleName()==role)),None), 'accessible: '+name)
        if '--orca' in sys.argv:
            with (output/'orca.log').open('w') as log:
                orca=subprocess.Popen(['orca','--debug-file',str(output/'orca-debug.log')],
                    env=os.environ | {'SPEECHD_ADDRESS':'unix_socket:'+str(runtime/'no-speech-service'),
                        'SPEECHD_CMD':'/bin/false'},stdout=log,stderr=subprocess.STDOUT)
            time.sleep(3)
            assert orca.poll() is None, 'isolated Orca did not start'
        with (output/'app.log').open('w') as log:
            app=subprocess.Popen([str(ROOT/'build/src/bastle')],stdout=log,stderr=subprocess.STDOUT)
        find('Альфа')
        find('alpha.example')
        result['checks']['russian_title_and_domain_accessible']=True
        assert not any('Запустить' in n.name or n.name=='Launch' for n in nodes())
        result['checks']['no_launch_controls']=True
        # Headless Mutter has a US keymap. Exercise real keyboard search by
        # domain, then test Cyrillic through the native EditableText interface.
        key(0xffe3,True);press(ord('f'));key(0xffe3,False)
        entry=wait(lambda: next((n for n in nodes() if n.getRoleName() in ('text','entry') and
            n.getState().contains(pyatspi.STATE_FOCUSED)),None),'search focused by Ctrl+F')
        for character in 'beta.example': press(ord(character))
        wait(lambda: entry.queryText().getText(0,-1)=='beta.example','keyboard domain search text')
        wait(lambda: not any(n.name=='Альфа' for n in nodes()),'search filters other rows')
        row=wait(lambda: next((n for n in nodes() if n.getRoleName()=='list item' and
            'Бета' in n.name),None),'accessible filtered row')
        assert 'beta.example' in row.name
        actions=row.queryAction()
        names=[actions.getName(index) for index in range(actions.nActions)]
        result['row_actions']=names
        # GtkListView exports scroll-to through AT-SPI Action. Move focus with
        # the real keyboard, then verify the focused accessible row and Enter.
        for _ in range(12):
            press(0xff09)
            settle()
            if row.getState().contains(pyatspi.STATE_FOCUSED): break
        else: raise AssertionError('filtered row is not reachable with Tab')
        if orca: time.sleep(2.5)  # Let Orca announce before retiring the row.
        press(0xff0d)
        find('Название','text')
        result['checks']['search_and_accessible_row_open_settings']=True
        assert not any('Запустить' in n.name for n in nodes())
        # Return through real keyboard and clear the active search.
        key(0xffe9,True);press(0xff51);key(0xffe9,False)
        find('beta.example')
        assert entry.queryEditableText().setTextContents('бета')
        wait(lambda: entry.queryText().getText(0,-1)=='бета','Cyrillic accessible search text')
        wait(lambda: not any(n.name=='Альфа' for n in nodes()),'Cyrillic title search filters rows')
        result['checks']['cyrillic_search_through_editable_text']=True
        press(0xff1b)
        find('Альфа')
        result['checks']['keyboard_back_and_clear_search']=True
        # Arrow navigation and Enter should open settings, never launch a site.
        press(0xff54);press(0xff0d)
        find('Название','text')
        result['checks']['keyboard_row_activation_opens_settings']=True
        key(0xffe9,True);press(0xff51);key(0xffe9,False)
        find('Альфа')
        press(0xffc7)  # F10 opens the native primary menu.
        time.sleep(.35)
        press(0xff53)  # Enter Sort Applications, the first submenu.
        time.sleep(.35)
        press(0xff54)  # Move from the submenu heading to A–Z.
        press(0xff54)  # Z–A.
        press(0xff0d)
        def row_names():
            return [n.name for n in nodes() if n.getRoleName()=='list item']
        wait(lambda: len(row_names())==3 and row_names()[0].startswith('Очень длинное'),
             'descending sort rebinds the list')
        result['checks']['native_sort_menu_reorders_rows']=True
        assert not (runtime/'data/bastle/profiles').exists(), 'manager navigation created an engine profile'
        result['checks']['settings_navigation_does_not_launch_engine']=True
        if '--creation' in sys.argv or '--settings' in sys.argv:
            from creation_checks import run
            run(nodes,find,wait,press,key,result,runtime)
            assert result['checks'].get('creation_real_portal_installs_launcher'), 'creation did not install its launcher'
        if '--settings' in sys.argv:
            from settings_checks import run
            run(nodes,find,wait,press,key,result,runtime)
        if '--policy' in sys.argv:
            from policy_checks import run
            run(nodes,find,wait,press,key,result,runtime)
        if '--utilities' in sys.argv:
            from utility_checks import run
            run(nodes,find,wait,press,key,result,runtime)
        result['accessible_snapshot']=snapshot()
        if orca:
            time.sleep(2.5)
            orca.terminate()
            orca.wait(timeout=3)
            speech=re.findall(r"SPEECH OUTPUT: '(.*?)' \{",
                (output/'orca-debug.log').read_text(),re.DOTALL)
            result['orca_speech']=speech
            assert any('Бета' in line and 'beta.example' in line for line in speech), speech
            result['checks']['orca_announces_focused_app_and_domain']=True
            if '--creation' in sys.argv or '--settings' in sys.argv:
                assert any('Адрес сайта' in line for line in speech), speech
                assert any('Название' in line for line in speech), speech
                assert any('Создано в проверке' in line and '127.0.0.1' in line for line in speech), speech
                result['checks']['orca_announces_creation_fields_and_created_app']=True
            if '--settings' in sys.argv:
                for label in ('Цвет заголовка','Собственный User-Agent','Переименовано в проверке'):
                    assert any(label in line for line in speech), label
                result['checks']['orca_announces_settings_controls_and_renamed_app']=True
            if '--policy' in sys.argv:
                for label in ('Камера','Режим прокси','Ограничить навигацию','Продолжать работу в фоне'):
                    assert any(label in line for line in speech), label
                result['checks']['orca_announces_policy_settings']=True
            if '--utilities' in sys.argv:
                for label in ('Дополнения','Системные возможности','Включить данные сайтов','Парольная фраза','Альфа'):
                    assert any(label in line for line in speech), label
                result['checks']['orca_announces_utility_controls']=True
        result['passed']=True
        key(0xffe3,True);press(ord('q'));key(0xffe3,False)
        wait(lambda: app.poll() is not None,'manager exits through Ctrl+Q')
        assert app.returncode==0
        call('Stop')
        return 0
    except Exception:
        result['error']=traceback.format_exc()
        result['passed']=False
        try: result['accessible_snapshot']=snapshot()
        except Exception: pass
        print(result['error'],file=sys.stderr)
        return 1
    finally:
        for proc in (app,orca,compositor):
            if proc is not None and proc.poll() is None:
                proc.terminate()
                try: proc.wait(timeout=5)
                except subprocess.TimeoutExpired: proc.kill();proc.wait()
        (output/'report.json').write_text(json.dumps(result,ensure_ascii=False,indent=2))
        print(output,flush=True)


if __name__=='__main__':
    raise SystemExit(main())
