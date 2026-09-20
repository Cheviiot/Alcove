#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Real select dropdown input and independent AT-SPI in private Mutter."""
import argparse, json, os, pathlib, time, traceback
import pyatspi
from gi.repository import Gio, GLib

parser=argparse.ArgumentParser()
parser.add_argument('--output',type=pathlib.Path,required=True)
args=parser.parse_args()
if os.environ.get('WAYLAND_DISPLAY')!='bastle-probe' or not os.path.basename(os.environ.get('XDG_RUNTIME_DIR','')).startswith('bastle-native-'):
    raise SystemExit('Refusing UI control outside isolated Bastle session')
result={'checks':{}}
pyatspi.Registry.registerEventListener(lambda e:None,'object','window')
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
service='org.gnome.Mutter.RemoteDesktop'
session,=bus.call_sync(service,'/org/gnome/Mutter/RemoteDesktop',service,'CreateSession',None,GLib.VariantType.new('(o)'),Gio.DBusCallFlags.NONE,3000,None).unpack()
def call(method,signature=None,values=()):
    return bus.call_sync(service,session,service+'.Session',method,GLib.Variant(signature,values) if signature else None,None,Gio.DBusCallFlags.NONE,3000,None)
def key(sym,pressed):call('NotifyKeyboardKeysym','(ub)',(sym,pressed))
def press(sym):key(sym,True);time.sleep(.06);key(sym,False)
def settle(seconds=.15):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
        time.sleep(.02)
def wait(predicate,description):
    end=time.monotonic()+6
    while time.monotonic()<end:
        value=predicate()
        if value:return value
        settle()
    raise AssertionError(description)
def events():
    p=args.output.parent/'app/events.jsonl'
    return [json.loads(l) for l in p.read_text().splitlines()] if p.exists() else []
def messages(kind):
    records=[]
    for e in events():
        if e.get('message','').startswith('BASTLE_SELECT:'):
            value=json.loads(e['message'].split(':',1)[1])
            if value['kind']==kind:records.append(value['value'])
    return records
def nodes():
    found=[]
    def visit(n,depth=0):
        if depth>40 or len(found)>1000:return
        try:
            n.clearCache();found.append(n)
            for c in n:
                if c:visit(c,depth+1)
        except GLib.Error:pass
    visit(pyatspi.Registry.getDesktop(0));return found
def find(name,roles):
    return wait(lambda:next((n for n in nodes() if n.name==name and n.getRoleName() in roles),None),'accessible '+name)
def state():
    return next((e for e in reversed(events()) if e.get('event')=='popup-surface'),{})
def opened(previous=0):
    return wait(lambda:(e if (e:=state()).get('visible') and e.get('generation',0)>previous and e.get('rect',[0,0,0,0])[2]>0 else None),'popup shown')
def painted(e):
    return wait(lambda:(p if (p:=args.output.parent/f'app/dropdown-{e["generation"]}.png').exists() else None),'popup actually presented and captured')
def pointer(x,y):
    call('NotifyPointerMotionRelative','(dd)',(-10000.,-10000.));call('NotifyPointerMotionRelative','(dd)',(float(x),float(y)))
def click():
    call('NotifyPointerButton','(ib)',(0x110,True));call('NotifyPointerButton','(ib)',(0x110,False))
try:
    call('Start');settle(5)
    combo=find('Цвет интерфейса',('combo box',))
    initial=wait(lambda:messages('geometry'),'fixture geometry')[-1]
    scale=initial['scale'];cx,cy=640,450
    result['checks']['select_accessible']=combo.getApplication().name=='bastle-native-chromium'
    # The private Mutter opens the sole 800x640 window at the monitor center.
    # The first select is centered in the viewport: use real pointer input.
    pointer(cx,cy);click()
    first=opened();painted(first)
    result['checks']['pointer_opens_rendered_popup']=True
    result['popup_nodes']=[{'name':n.name,'role':n.getRoleName()} for n in nodes() if n.name in ('Красный','Зелёный','Синий')]
    press(0xff54);press(0xff0d) # Down, Enter.
    wait(lambda:{'value':'green','trusted':True} in messages('change'),'trusted keyboard selection')
    wait(lambda:state().get('visible') is False,'popup hidden after selection')
    result['checks']['keyboard_selection']=True
    pointer(cx,cy);click();second=opened(first['generation']);painted(second)
    result['checks']['reopen_new_generation']=second['generation']>first['generation']
    # Three options fill the popup. Choose the bottom option using its actual
    # CEF bounds, translated from viewport to centered compositor coordinates.
    x,y,w,h=second['rect'];vw,vh=initial['viewport']
    pointer(cx-vw/2+x+w/2,cy-vh/2+y+h*5/6);click()
    wait(lambda:{'value':'blue','trusted':True} in messages('change'),'trusted pointer selection')
    wait(lambda:state().get('visible') is False,'popup hidden after pointer selection')
    result['checks']['pointer_selection']=True
    # Test popup placement by the lower/right edge without resizing the page.
    find('Переместить список к краю',('button','push button')).queryAction().doAction(0)
    moved=wait(lambda:messages('geometry') if len(messages('geometry'))>1 else None,'edge geometry')[-1]
    combo=find('Цвет интерфейса',('combo box',));assert combo.queryComponent().grabFocus();settle(.3)
    key(0xffe9,True);press(0xff54);key(0xffe9,False) # Alt+Down.
    third=opened(second['generation']);painted(third)
    x,y,w,h=third['rect'];vw,vh=moved['viewport']
    result['checks']['edge_popup_inside_viewport']=x>=0 and y>=0 and x+w<=vw and y+h<=vh
    press(0xff1b);wait(lambda:state().get('visible') is False,'Escape dismisses popup')
    result['checks']['escape_preserves_selection']=messages('change')[-1]=={'value':'blue','trusted':True}
    assert combo.queryComponent().grabFocus();settle(.3)
    key(0xffe9,True);press(0xff54);key(0xffe9,False)
    fourth=opened(third['generation']);painted(fourth)
    option=find('Зелёный',('menu item',));assert option.queryAction().doAction(0)
    wait(lambda:len(messages('change'))==3 and messages('change')[-1]=={'value':'green','trusted':True},'native option action')
    result['checks']['native_option_action']=True
    # Chromium's native action selects an option without necessarily dismissing
    # its menu. Preserve that behavior; Escape must close the rendered surface.
    result['native_action_kept_menu_open']=state().get('visible')
    press(0xff1b);wait(lambda:state().get('visible') is False,'Escape after native action')
    result['checks']['native_action_escape_preserves_selection']=messages('change')[-1]=={'value':'green','trusted':True}
    result['checks']['page_size_unchanged']=all(g['viewport']==initial['viewport'] for g in messages('geometry'))
    result['checks']['native_option_names']=all(any(n['name']==name for n in result['popup_nodes']) for name in ('Красный','Зелёный','Синий'))
except Exception:result['error']=traceback.format_exc()
finally:
    call('Stop');result['passed']=bool(result['checks']) and all(result['checks'].values()) and 'error' not in result
    args.output.write_text(json.dumps(result,ensure_ascii=False,indent=2));print(json.dumps(result,ensure_ascii=False,indent=2))
raise SystemExit(0 if result['passed'] else 1)
