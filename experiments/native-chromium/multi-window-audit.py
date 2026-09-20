#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Independent AT-SPI client: two identical URLs/titles, distinct native trees."""
import argparse,json,os,pathlib,time,traceback
import pyatspi
from gi.repository import GLib
parser=argparse.ArgumentParser()
parser.add_argument('--output',type=pathlib.Path,required=True)
args=parser.parse_args()
if os.environ.get('WAYLAND_DISPLAY')!='alcove-probe' or not os.path.basename(os.environ.get('XDG_RUNTIME_DIR','')).startswith('alcove-native-'):
    raise SystemExit('Refusing UI control outside isolated Alcove session')
result={'checks':{}}
pyatspi.Registry.registerEventListener(lambda e: None,'object','window','document')
def settle(seconds=.1):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
        time.sleep(.01)
def walk(node=None):
    found=[]
    def visit(n,depth=0):
        if depth>50 or len(found)>1800:return
        try:
            n.clearCache();found.append(n)
            for child in n:
                if child:visit(child,depth+1)
        except GLib.Error:pass
    visit(node or pyatspi.Registry.getDesktop(0))
    return found
def docs(title='Одинаковая страница'):
    found=[]
    for n in walk():
        try:
            if n.getRoleName()=='document web' and n.name==title and n.getApplication().name=='alcove-native-chromium': found.append(n)
        except GLib.Error:pass # A navigation can remove a node during enumeration.
    return found
def wait(predicate,description):
    end=time.monotonic()+6
    while time.monotonic()<end:
        value=predicate()
        if value:return value
        settle()
    raise AssertionError(description)
def node(document,name):return next(n for n in walk(document) if n.name==name and n.getRoleName() in ('button','push button','link','entry','text'))
def action(document,name):
    target=node(document,name);assert target.queryAction().doAction(0),name;settle(.4);return target
def events():
    p=args.output.parent/'app/events.jsonl'
    return [json.loads(line) for line in p.read_text().splitlines()] if p.exists() else []
def actions(view):return sum(e.get('view')==view and e.get('message')=='ALCOVE_MULTI_ACTION:{"trusted":true}' for e in events())
def defunct(n):
    try:n.clearCache();return n.getState().contains(pyatspi.STATE_DEFUNCT)
    except GLib.Error:return True
try:
    main=wait(lambda:next(iter(docs()),None),'main document')
    settle(5)
    original_action=node(main,'Действие страницы')
    action(main,'Открыть второе окно')
    pair=wait(lambda:docs() if len(docs())==2 else None,'two identical documents')
    child=next(n for n in pair if n!=main)
    result['checks']['identical_titles_distinct_objects']=main!=child and main.name==child.name
    result['checks']['same_document_uri']=main.queryDocument().getAttributeValue('URI')==child.queryDocument().getAttributeValue('URI')
    # Both web roots must belong to distinct real GTK toplevels.
    def frame(n):
        for _ in range(20):
            if n.getRoleName()=='frame':return n
            n=n.parent
        raise AssertionError('No GTK frame ancestor')
    result['checks']['separate_gtk_windows']=frame(main)!=frame(child)
    result['checks']['native_window_titles']=frame(main).name==main.name and frame(child).name==child.name
    action(child,'Действие страницы')
    wait(lambda:actions(2)==1,'child trusted action is routed to view 2')
    result['checks']['child_action_routing']=actions(1)==0
    previous=max((e.get('id',0) for e in events() if e.get('event')=='native-accessibility-focus-result' and e.get('view')==1),default=0)
    assert original_action.queryAction().doAction(0)
    wait(lambda:any(e.get('event')=='native-accessibility-focus-result' and e.get('view')==1 and e.get('id',0)>previous and e.get('focused') is False for e in events()),'inactive parent action is rejected')
    result['checks']['inactive_parent_action_blocked']=actions(1)==0
    # Same text interface exists in each tree, and selections stay independent.
    field=node(child,'Поле страницы');text=field.queryText()
    text.setCaretOffset(3);text.addSelection(1,5);settle()
    result['checks']['child_text_selection']=list(text.getSelection(0))==[1,5]
    result['checks']['parent_text_untouched']=node(main,'Поле страницы').queryText().getNSelections()==0
    action(child,'Действие во фрейме')
    result['checks']['iframe_action_routing']=bool(wait(lambda:any(e.get('view')==2 and e.get('message')=='ALCOVE_FRAME_ACTION:true' for e in events()),'native iframe action'))
    old_child_action=node(child,'Действие страницы')
    action(child,'Перейти дальше')
    other=wait(lambda:next(iter(docs('Другая страница окна')),None),'child navigation')
    result['checks']['only_child_document_retired']=defunct(child) and not defunct(main) and not defunct(original_action)
    try: rejected=not old_child_action.queryAction().doAction(0)
    except (NotImplementedError,GLib.Error): rejected=True
    result['checks']['retired_child_action_rejected']=rejected
    action(other,'Вернуться к одинаковой странице')
    restored=wait(lambda:next((n for n in docs() if n!=main),None),'child history restores correct socket')
    action(restored,'Действие страницы')
    result['checks']['restored_action_routing']=bool(wait(lambda:actions(2)==2,'restored action in child')) and actions(1)==0
    action(restored,'Закрыть это окно')
    wait(lambda:any(e.get('event')=='view-closed' and e.get('view')==2 for e in events()),'child closes')
    settle(.6)
    result['checks']['closed_child_defunct']=defunct(restored)
    action(main,'Действие страницы')
    result['checks']['parent_survives_child_close']=bool(wait(lambda:actions(1)==1,'original parent action remains valid'))
    result['checks']['no_binding_errors']=not any(e.get('event')=='native-accessibility-binding-error' for e in events())
    result['bindings']=[{'view':e['view'],'tree':e['tree']} for e in events() if e.get('event')=='native-accessibility-bound']
except Exception:result['error']=traceback.format_exc()
result['passed']=bool(result['checks']) and all(result['checks'].values()) and 'error' not in result
args.output.write_text(json.dumps(result,ensure_ascii=False,indent=2));print(json.dumps(result,ensure_ascii=False,indent=2))
raise SystemExit(0 if result['passed'] else 1)
