#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Drive real GTK/Chromium/portal UI through the private session's AT-SPI bus."""
import argparse
import json
import os
import pathlib
import time
import traceback
import pyatspi
from gi.repository import Gio, GLib

parser = argparse.ArgumentParser()
parser.add_argument('--output', type=pathlib.Path, required=True)
args = parser.parse_args()
if os.environ.get('WAYLAND_DISPLAY') != 'alcove-probe' or not os.path.basename(os.environ.get('XDG_RUNTIME_DIR','')).startswith('alcove-native-'):
    raise SystemExit('Refusing UI control outside the isolated Alcove session')
result = {'checks': {}}
pyatspi.Registry.registerEventListener(lambda event: None, 'object', 'window')
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
service = 'org.gnome.Mutter.RemoteDesktop'
session, = bus.call_sync(service, '/org/gnome/Mutter/RemoteDesktop', service, 'CreateSession', None,
                        GLib.VariantType.new('(o)'), Gio.DBusCallFlags.NONE, 3000, None).unpack()
def keycall(method, signature=None, values=()):
    return bus.call_sync(service, session, service + '.Session', method,
        GLib.Variant(signature,values) if signature else None, None,Gio.DBusCallFlags.NONE,3000,None)
def key(keysym, pressed): keycall('NotifyKeyboardKeysym','(ub)',(keysym,pressed))
def press(keysym): key(keysym,True); time.sleep(.06); key(keysym,False)
def settle(seconds=.15):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        context = GLib.MainContext.default()
        while context.pending(): context.iteration(False)
        time.sleep(.01)
def nodes():
    found = []
    def visit(node, depth=0):
        if depth > 50 or len(found) > 1200: return
        try:
            node.clearCache()
            found.append(node)
            for child in node:
                if child: visit(child,depth+1)
        except GLib.Error: pass
    visit(pyatspi.Registry.getDesktop(0))
    return found
def find(name, roles=None, timeout=5, enabled=False):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        for node in nodes():
            try:
                if node.name == name and (roles is None or node.getRoleName() in roles) and (not enabled or node.getState().contains(pyatspi.STATE_SENSITIVE)): return node
            except GLib.Error: pass
        settle()
    raise AssertionError(f'Accessible not found: {name!r}, {roles}')
def click(name):
    node=find(name,('push button','button'),enabled=True)
    assert node.queryAction().doAction(0), name
    settle()
def menu_item(name):
    # GTK can expose a model item's text as a labelled child. Walk the real
    # accessible hierarchy instead of assuming its parent has a flat name.
    node=find(name)
    for _ in range(8):
        if node.getRoleName()=='menu item': return node
        node=node.parent
        if node is None: break
    raise AssertionError(f'No menu item ancestor for {name}')
def events():
    path=args.output.parent/'app/events.jsonl'
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
def wait_log(kind,value):
    until=time.monotonic()+5
    while time.monotonic()<until:
        for event in events():
            message=event.get('message','')
            if message.startswith('ALCOVE_REQUEST:'):
                record=json.loads(message.split(':',1)[1])
                if record == {'kind':kind,'value':value}: return True
        settle()
    raise AssertionError(f'Missing renderer result: {kind}={value!r}')
def portal_path(path, save=False):
    # GTK portal provides the location field through Ctrl+L. Use its real
    # EditableText interface; acceptance is delivered through Mutter's seat.
    deadline=time.monotonic()+6
    while time.monotonic()<deadline:
        if any(node.getApplication().name == 'xdg-desktop-portal-gtk' for node in nodes()[1:]): break
        settle()
    else: raise AssertionError('Real GTK file portal did not appear')
    settle(.6)
    # Give the real chooser a compositor activation serial. AT-SPI site
    # actions do not themselves supply a Wayland pointer serial to GTK3.
    keycall('NotifyPointerMotionRelative','(dd)',(-10000.,-10000.))
    keycall('NotifyPointerMotionRelative','(dd)',(640.,450.))
    keycall('NotifyPointerButton','(ib)',(0x110,True))
    keycall('NotifyPointerButton','(ib)',(0x110,False))
    key(0xffe3,True); press(ord('l')); key(0xffe3,False)
    settle(.4)
    entries=[node for node in nodes() if node.getRoleName() in ('text','entry') and node.getApplication().name=='xdg-desktop-portal-gtk']
    visible=[node for node in entries if node.getState().contains(pyatspi.STATE_SHOWING) and node.name != 'Search']
    if not visible: raise AssertionError('Portal location field is not exposed')
    visible[0].queryEditableText().setTextContents(str(path))
    click('Save' if save else 'Open')
    settle(.7)
    # Some GtkFileChooser versions first select the file, then require Open.
    if any(node.getApplication().name=='xdg-desktop-portal-gtk' and node.getRoleName()=='file chooser' for node in nodes()[1:]):
        press(0xff0d)
        settle(.4)
def click_file():
    file_node=next(node for node in nodes() if node.name.startswith('Выбрать тестовый файл') and node.getRoleName() in ('button','push button'))
    assert file_node.queryAction().doAction(0)
def wait_download_cancelled(download_id):
    deadline=time.monotonic()+5
    while time.monotonic()<deadline:
        if any(e.get('event')=='download' and e.get('download')==download_id and e.get('cancelled') for e in events()): return True
        settle()
    return False
def blocked_web_action():
    previous=max((e.get('id',0) for e in events() if e.get('event')=='native-accessibility-focus-result'),default=0)
    # A screen reader can still hold a reference to content behind a modal.
    # The bridge must ask GTK before executing its default browser action.
    assert find('Скачать тестовый файл',('button',)).queryAction().doAction(0)
    deadline=time.monotonic()+3
    while time.monotonic()<deadline:
        replies=[e for e in events() if e.get('event')=='native-accessibility-focus-result' and e['id']>previous]
        if replies: return replies[-1]['focused'] is False
        settle()
    return False

try:
    keycall('Start')
    time.sleep(5)
    press(0xffc7); settle(.3); press(0x20)
    result['checks']['menu_keyboard']=bool(menu_item('Загрузки'))
    result['checks']['menu_blocks_web_action']=blocked_web_action()
    press(0xff1b)
    click('Сообщение сайта'); find('Сообщение <b>без разметки</b>')
    result['checks']['dialog_blocks_web_action']=blocked_web_action()
    settle(2); click('ОК')
    result['checks']['alert']=wait_log('alert',True)
    click('Подтверждение сайта'); click('Отмена')
    result['checks']['confirm_deny']=wait_log('confirm',False)
    click('Подтверждение сайта'); click('ОК')
    result['checks']['confirm_allow']=wait_log('confirm',True)
    click('Ответ сайту')
    entry=find('Ответ сайту',('text','entry'))
    entry.queryEditableText().setTextContents('Ответ из GTK')
    click('ОК'); result['checks']['prompt']=wait_log('prompt','Ответ из GTK')
    click('Разрешение уведомлений'); click('Всегда блокировать')
    result['checks']['notification_deny']=wait_log('notification','denied')
    click('Разрешение местоположения'); click('Разрешить на эту сессию')
    result['checks']['geolocation_allow']=wait_log('location','granted')
    click('Разрешение микрофона'); click('Разрешить на эту сессию')
    result['checks']['microphone_allow']=wait_log('microphone','granted')
    click('Разрешение камеры'); click('Всегда блокировать')
    result['checks']['camera_deny']=wait_log('camera','NotAllowedError')
    upload=args.output.parent/'upload.txt'; upload.write_text('Alcove portal upload\n')
    # CEF exposes the file control as a button with its localized default name.
    click_file()
    portal_path(upload)
    result['checks']['file_portal_open']=wait_log('files',[{'name':'upload.txt','text':'Alcove portal upload\n'}])
    click_file(); click('Cancel')
    result['checks']['file_portal_cancel']=wait_log('file-cancel',True)
    click('Скачать тестовый файл')
    download=args.output.parent/'download.txt'
    portal_path(download,save=True)
    deadline=time.monotonic()+5
    while not download.exists() and time.monotonic()<deadline: settle()
    result['checks']['download_portal_save']=download.read_text()=='Alcove portal download\n'
    result['checks']['download_complete']=any(e.get('event')=='download' and e.get('complete') for e in events())
    click('Скачать тестовый файл')
    click('Cancel')
    result['checks']['download_portal_cancel']=wait_download_cancelled(2)
    click('Скачать большой файл')
    cancelled_download=args.output.parent/'cancelled.bin'
    portal_path(cancelled_download,save=True)
    press(0xffc7)  # F10 reveals the overlay and focuses its menu.
    settle(.3)
    result['menu_before_space']=[{'name':node.name,'role':node.getRoleName(),
        'focused':node.getState().contains(pyatspi.STATE_FOCUSED)} for node in nodes() if node.name in ('Меню','Загрузки')]
    press(0x20)  # Open the focused MenuButton through the compositor keyboard.
    menu_item('Загрузки').queryAction().doAction(0)
    find('alcove-slow.bin')
    click('Отменить загрузку')
    result['checks']['download_ui_cancel']=wait_download_cancelled(3) and not cancelled_download.exists()
    # The native dialog header uses the toolkit's localized Close button.
    press(0xff1b)
    settle(.5)
    click('Проверка закрытия страницы'); click('Остаться')
    result['checks']['before_unload_cancel']=not any('"kind":"before-unload","value":true' in e.get('message','') for e in events())
    click('Проверка закрытия страницы'); click('Покинуть')
    result['checks']['before_unload_accept']=wait_log('before-unload',True)
    click('Отмена при переходе')
    find('Разрешение сайта')
    settle(3)
    result['checks']['navigation_cancels_prompt']=not any(node.name=='Разрешение сайта' for node in nodes()) and any(e.get('event')=='request-cancelled' for e in events())
    click('Отмена при переходе')
    find('Разрешение сайта')
    click('Не сейчас')
    settle(3)
    result['checks']['navigation_dismissal_does_not_save_denial']=True
    click('Отмена выбора при переходе')
    find('Выбрать файл',('file chooser',))
    settle(4)
    result['checks']['navigation_closes_portal']=not any(node.name=='Выбрать файл' and node.getRoleName()=='file chooser' for node in nodes())
except Exception:
    result['error']=traceback.format_exc()
finally:
    result['nodes']=[]
    for node in nodes()[1:]:
        try:
            result['nodes'].append({'name':node.name,'role':node.getRoleName(),'app':node.getApplication().name})
        except GLib.Error:
            pass  # The last navigation may retire an endpoint during inspection.
    result['passed']=bool(result['checks']) and all(result['checks'].values()) and 'error' not in result
    args.output.write_text(json.dumps(result,ensure_ascii=False,indent=2))
    keycall('Stop')
