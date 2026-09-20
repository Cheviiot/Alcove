#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Exercise real window.open and local OAuth using the private compositor seat.

GTK shells/dialogs are inspected via AT-SPI, and page actions use actual Enter
keys. Optional native/Orca modes also inspect website objects and allow the
screen reader to process focus events before navigating away from the page.
"""
import argparse
import json
import os
import pathlib
import time
import traceback
import pyatspi
from gi.repository import Gio, GLib

parser=argparse.ArgumentParser()
parser.add_argument('--native',action='store_true')
parser.add_argument('--orca',action='store_true')
parser.add_argument('--output',type=pathlib.Path,required=True)
args=parser.parse_args()
if os.environ.get('WAYLAND_DISPLAY')!='bastle-probe' or not os.path.basename(os.environ.get('XDG_RUNTIME_DIR','')).startswith('bastle-native-'):
    raise SystemExit('Refusing UI control outside the isolated Bastle session')
result={'scope':'popup transport and local OAuth' + (' with native website AT-SPI' if args.native else '; website screenreader support is not accepted'),'checks':{}}
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
service='org.gnome.Mutter.RemoteDesktop'
session,=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,GLib.VariantType.new('(o)'),Gio.DBusCallFlags.NONE,3000,None).unpack()
def call(method,signature=None,values=()):
    return bus.call_sync(service,session,service+'.Session',method,GLib.Variant(signature,values) if signature else None,None,Gio.DBusCallFlags.NONE,3000,None)
def key(sym,down):call('NotifyKeyboardKeysym','(ub)',(sym,down))
def press(sym):key(sym,True);time.sleep(.06);key(sym,False)
def events():
    path=args.output.parent/'app/events.jsonl'
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
def wait(predicate,description,timeout=6):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        value=predicate()
        if value:return value
        while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
        time.sleep(.1)
    raise AssertionError(description)
def message(kind):
    for event in events():
        text=event.get('message','')
        if text.startswith('BASTLE_POPUP:'):
            value=json.loads(text.split(':',1)[1])
            if value['kind']==kind:return value['value']
    return None
def nodes():
    found=[]
    def visit(node,depth=0):
        if depth>40 or len(found)>1000:return
        try:
            node.clearCache();found.append(node)
            for child in node:
                if child:visit(child,depth+1)
        except GLib.Error:pass
    visit(pyatspi.Registry.getDesktop(0))
    return found
def click(name):
    def find():
        return next((n for n in nodes() if n.name==name and n.getRoleName() in ('button','push button')),None)
    node=wait(find,'Native GTK dialog button: '+name)
    assert node.queryAction().doAction(0)
    time.sleep(.5)
def native_close():
    key(0xffe9,True);press(0xffc1);key(0xffe9,False) # Alt+F4

def native_document(title):
    return any(n.name==title and n.getRoleName()=='document web' and n.getApplication().name=='bastle-native-chromium' for n in nodes())
def native_window(title):
    return any(n.name==title and n.getRoleName()=='frame' and n.getApplication().name=='bastle-native-chromium' for n in nodes())
def title_loaded(title):return any(e.get('event')=='title' and e.get('title')==title for e in events())
def allow_speech():
    if not args.orca:return
    # Orca queues events and buffers its debug file. Navigating immediately
    # makes queued controls defunct before speech generation. Inspect actual
    # speech in the completed run, not a partially flushed live log.
    time.sleep(2.5)
try:
    call('Start');time.sleep(6)
    if args.native: result['checks']['native_window_titles']=bool(wait(lambda:native_window('Bastle popup fixture'),'main window title'))
    # Autofocused button in the fixture receives a real keyboard activation.
    allow_speech()
    press(0xff0d)
    result['checks']['trusted_open']=wait(lambda:message('trusted-open'),'trusted main action') is True
    result['checks']['window_returned']=wait(lambda:message('window-returned'),'window.open returned a WindowProxy') is True
    child=wait(lambda:message('child-session'),'child inherits opener and cookie')
    result['checks']['shared_session_and_opener']=child=={'opener':True,'cookie':True}
    wait(lambda:any(e.get('event')=='view-created' for e in events()),'native child created')
    if args.native: result['checks']['native_child_document']=bool(wait(lambda:native_document('Bastle OAuth step'),'child website in GTK AT-SPI tree'))
    if args.native: result['checks']['native_window_titles'] &= bool(wait(lambda:native_window('Bastle OAuth step'),'child window title'))
    allow_speech()
    time.sleep(1);press(0xff0d)
    wait(lambda:title_loaded('Bastle local provider'),'cross-origin provider loaded')
    if args.native: result['checks']['native_provider_document']=bool(wait(lambda:native_document('Bastle local provider'),'cross-origin website in GTK AT-SPI tree'))
    if args.native: result['checks']['native_window_titles'] &= bool(wait(lambda:native_window('Bastle local provider'),'provider window title'))
    result['checks']['cross_origin_security']=wait(lambda:message('cross-origin-opener-isolated'),'cross-origin access blocked') is True
    allow_speech()
    time.sleep(1);press(0xff0d)
    value=wait(lambda:message('oauth'),'callback delivered to original opener')
    result['checks']['oauth_callback']=value=={'state':'bastle-local-state','code':'local-test-code','cookie':True,'opener':True}
    wait(lambda:any(e.get('event')=='view-closed' and e.get('view')==2 for e in events()),'window.close closes native child')
    result['checks']['script_close']=True
    time.sleep(1);press(0xff0d)
    wait(lambda:message('close-window-returned'),'second native window created')
    wait(lambda:title_loaded('Bastle close confirmation'),'close fixture loaded')
    time.sleep(1);press(0xff0d)
    wait(lambda:message('beforeunload-armed'),'beforeunload user activation')
    native_close();click('Остаться')
    result['checks']['close_cancel']=not any(e.get('event')=='view-closed' and e.get('view')==3 for e in events())
    native_close();click('Покинуть')
    wait(lambda:any(e.get('event')=='view-closed' and e.get('view')==3 for e in events()),'native close confirmed')
    result['checks']['close_confirm']=True
    reports=[]
    for view in (2,3):
        path=args.output.parent/f'app/views/{view}/report.json'
        wait(path.exists,'child report')
        reports.append(json.loads(path.read_text()))
    result['checks']['separate_frames']=all(r['gpu_presented']+r['cpu_frames']>0 and not r['errors'] for r in reports)
    result['children']=reports
    result['checks']['single_worker']=len({e['pid'] for e in events() if e.get('event')=='ready'})==1
    result['checks']['all_native_windows_closed']=sum(e.get('event')=='view-created' for e in events())==2 and sum(e.get('event')=='view-closed' for e in events())==2
except Exception:
    result['error']=traceback.format_exc()
finally:
    call('Stop')
    result['passed']=bool(result['checks']) and all(result['checks'].values()) and 'error' not in result
    args.output.write_text(json.dumps(result,ensure_ascii=False,indent=2))
    print(json.dumps(result,ensure_ascii=False,indent=2))
raise SystemExit(0 if result['passed'] else 1)
