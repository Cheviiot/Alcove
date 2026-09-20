#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Public sites in the actual GTK/CEF prototype, using AT-SPI and Wayland input.

No browser replacement, DOM-click injection, accounts or desktop input access.
Page observations come from a separate read-only sampler in the probe host.
"""
import argparse
import collections
import json
import os
import pathlib
import time
import traceback
import urllib.parse

import pyatspi
from gi.repository import Gio, GLib

parser = argparse.ArgumentParser()
parser.add_argument('--output', type=pathlib.Path, required=True)
parser.add_argument('--url', required=True)
parser.add_argument('--scenario', required=True)
args = parser.parse_args()
if os.environ.get('WAYLAND_DISPLAY') != 'alcove-probe' or not os.path.basename(
        os.environ.get('XDG_RUNTIME_DIR', '')).startswith('alcove-native-'):
    raise SystemExit('Refusing UI control outside isolated Alcove session')
result = {'url': args.url, 'scenario': args.scenario, 'checks': {}, 'snapshots': {}}
def progress(stage):
    result['stage'] = stage
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(stage, flush=True)
pyatspi.Registry.registerEventListener(lambda e: None, 'object', 'window')
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
service = 'org.gnome.Mutter.RemoteDesktop'
session, = bus.call_sync(service, '/org/gnome/Mutter/RemoteDesktop', service,
    'CreateSession', None, GLib.VariantType.new('(o)'), Gio.DBusCallFlags.NONE, 3000, None).unpack()

def call(method, signature=None, values=()):
    return bus.call_sync(service, session, service + '.Session', method,
        GLib.Variant(signature, values) if signature else None,
        None, Gio.DBusCallFlags.NONE, 3000, None)

def key(sym, pressed):
    call('NotifyKeyboardKeysym', '(ub)', (sym, pressed))

def press(sym):
    key(sym, True)
    time.sleep(.07)
    key(sym, False)

def settle(seconds=.2):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        for _ in range(100):
            if not GLib.MainContext.default().pending():
                break
            GLib.MainContext.default().iteration(False)
        time.sleep(.02)

def wait(predicate, description, seconds=15):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        value = predicate()
        if value:
            return value
        settle()
    raise AssertionError(description)

def events():
    path = args.output.parent / 'app/events.jsonl'
    if not path.exists():
        return []
    # The producer may still be writing its final line.
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith('\n')]

def samples():
    return [json.loads(e['message'].split(':', 1)[1]) for e in events()
            if e.get('message', '').startswith('ALCOVE_SITE_STATE:')]

def latest():
    states = samples()
    return states[-1] if states else {}

def nodes(limit=1500):
    count = 0
    pending = collections.deque([(pyatspi.Registry.getDesktop(0), 0)])
    while pending:
        node, depth = pending.popleft()
        if depth > 45 or count >= limit:
            continue
        try:
            count += 1
            yield node
            for child in node:
                if child:
                    pending.append((child, depth + 1))
        except GLib.Error as error:
            if not any(text in str(error) for text in ('does not exist', 'no longer exists')):
                raise

def snapshot(name):
    progress('snapshot:' + name)
    records = []
    for node in nodes(limit=120):
        try:
            application = node.getApplication() if node.getRoleName() == 'document web' else None
            records.append({'name': node.name, 'role': node.getRoleName(),
                'app': application.name if application else ''})
        except GLib.Error:
            pass
    result['snapshots'][name] = records
    progress('snapshot-complete:' + name)
    return records

def find(predicate, description):
    return wait(lambda: next((n for n in nodes() if predicate(n)), None), description)

def action(node):
    wait(lambda: node.getState().contains(pyatspi.STATE_SENSITIVE) and node.queryAction().nActions > 0, 'native action enabled', 5)
    assert node.queryAction().doAction(0), 'native accessible action rejected'
    settle(.3)

def focus(node):
    assert node.queryComponent().grabFocus(), 'native focus rejected'
    wait(lambda: node.getState().contains(pyatspi.STATE_FOCUSED), 'site focus acknowledged', 3)

def command(labels):
    press(0xffc7)
    settle(.3)
    press(0x20)  # Open the focused native menu with the real keyboard.
    action(find(lambda n: n.getRoleName() == 'menu item' and n.name in labels, 'native command ' + str(labels)))

try:
    call('Start')
    state = wait(lambda: (s if (s := latest()).get('ready') in ('interactive', 'complete') and (s.get('text') or s.get('canvases')) else None),
        'external site loaded', 20)
    progress('loaded')
    expected = urllib.parse.urlsplit(args.url).hostname
    result['checks']['https_site_loaded'] = urllib.parse.urlsplit(state['url']).hostname == expected
    find(lambda n: n.getRoleName() == 'document web' and n.getApplication().name == 'alcove-native-chromium', 'website attached to GTK accessibility')
    initial = snapshot('loaded')
    result['checks']['site_document_in_gtk'] = any(n['role'] == 'document web' and n['app'] == 'alcove-native-chromium' for n in initial)
    result['checks']['semantic_site_content'] = sum(n['role'] in ('link', 'heading', 'entry', 'text', 'button', 'paragraph') for n in initial) > 3
    if args.scenario == 'gnome':
        initial_scale = latest()['scale']
        command(('Zoom In', 'Увеличить масштаб'))
        wait(lambda: latest().get('scale', 0) > initial_scale * 1.05, 'native zoom changed website scale')
        result['checks']['native_zoom'] = True
        command(('Reset Zoom', 'Сбросить масштаб'))
        wait(lambda: abs(latest().get('scale', 0) - initial_scale) < .01, 'native zoom reset')
        result['checks']['native_zoom_reset'] = True
    if args.scenario == 'wikipedia':
        progress('search-field')
        entry = find(lambda n: n.getRoleName() in ('entry', 'text') and 'search' in n.name.lower(), 'Wikipedia search field')
        focus(entry)
        for char in 'GNOME':
            press(ord(char))
        wait(lambda: entry.queryText().getText(0, -1) == 'GNOME', 'real keyboard typed search')
        result['checks']['keyboard_search_text'] = True
        press(0xff0d)
        wait(lambda: '/wiki/GNOME' in latest().get('url', '') and 'GNOME' in latest().get('title', ''), 'Wikipedia search navigation')
        result['checks']['search_navigated'] = True
        snapshot('article')
    elif args.scenario == 'gnome':
        progress('principles-link')
        action(find(lambda n: n.getRoleName() == 'link' and n.name == 'Design Principles', 'GNOME principles link'))
        wait(lambda: '/hig/principles.html' in latest().get('url', ''), 'real site link navigation')
        result['checks']['link_navigation'] = True
        snapshot('principles')
    elif args.scenario == 'video':
        progress('video-ready')
        wait(lambda: any(v['ready'] >= 2 for v in latest().get('videos', [])), 'external video ready')
        play = find(lambda n: 'button' in n.getRoleName() and n.name.lower() in ('play', 'воспроизвести'), 'native video play button')
        action(play)
        wait(lambda: any(v['time'] > 1 and v['frames'] > 10 and not v['error'] for v in latest().get('videos', [])), 'actual video decoding and playback')
        result['checks']['video_frames_advance'] = True
        snapshot('playing')
    elif args.scenario == 'webgl':
        result['checks']['webgl_site_reports_support'] = 'Your browser supports WebGL' in state.get('text', '') and 'disabled or unavailable' not in state.get('text', '')
        result['checks']['visible_canvas'] = any(c['visible'] and c['width'] > 0 for c in state['canvases'])
        # A visible canvas is not proof of animation; screenshot comparison is
        # performed separately and no new GL context is created by this audit.
    if args.scenario in ('wikipedia', 'gnome'):
        progress('scroll')
        before = latest().get('scroll', [0, 0])[1]
        press(0xff56)  # PageDown via the private compositor keyboard.
        wait(lambda: latest().get('scroll', [0, 0])[1] > before, 'real keyboard scroll')
        result['checks']['keyboard_scroll'] = True
        press(0xffc7)  # F10 opens the native header without changing page size.
        settle(.3)
        snapshot('native-header')
        back = find(lambda n: 'button' in n.getRoleName() and n.name in ('Назад', 'Back'), 'native Back button')
        action(back)
        wait(lambda: latest().get('url', '').rstrip('/') == args.url.rstrip('/'), 'native Back returned to starting site')
        result['checks']['native_back'] = True
    if args.scenario == 'gnome':
        # The shared WebView shell must drive CEF through actual native actions.
        action(find(lambda n: 'button' in n.getRoleName() and n.name in ('Forward', 'Вперёд'), 'native Forward button'))
        wait(lambda: '/hig/principles.html' in latest().get('url', ''), 'native Forward restored page')
        result['checks']['native_forward'] = True
        command(('Zoom In', 'Увеличить масштаб'))
        wait(lambda: latest().get('scale', 0) > initial_scale * 1.05, 'zoom after history navigation')
        result['checks']['native_zoom_after_history'] = True
        command(('Reset Zoom', 'Сбросить масштаб'))
        command(('Home', 'На главную'))
        wait(lambda: latest().get('url', '').rstrip('/') == args.url.rstrip('/'), 'native Home loaded start URL')
        result['checks']['native_home'] = True
        previous_navigation = latest()['navigationStarted']
        command(('Reload Without Cache', 'Перезагрузить без кэша'))
        wait(lambda: latest().get('navigationStarted', 0) > previous_navigation, 'native reload created a new document')
        result['checks']['native_reload_without_cache'] = True
    result['checks']['no_main_load_error'] = not any(e.get('event') == 'load-error' for e in events())
except Exception:
    result['error'] = traceback.format_exc()
    try:
        snapshot('failure')
    except Exception as error:
        result['snapshot_error'] = str(error)
finally:
    result['observations'] = samples()
    result['load_errors'] = [e for e in events() if e.get('event') == 'load-error']
    result['passed'] = bool(result['checks']) and all(result['checks'].values()) and 'error' not in result
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps({k: v for k, v in result.items() if k not in ('snapshots', 'observations')}, ensure_ascii=False, indent=2))
    call('Stop')
raise SystemExit(0 if result['passed'] else 1)
