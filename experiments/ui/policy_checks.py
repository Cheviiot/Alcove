#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Native policy settings checks, confined to the audit's disposable application."""
import json
import time


def run(nodes,find,wait,press,key,result,runtime):
    import pyatspi
    path=runtime/'data/bastle/apps/library00000/policy.json'
    def policy(): return json.loads(path.read_text())
    def focus(node):
        for _ in range(80):
            node.clear_cache_single()
            if node.getState().contains(pyatspi.STATE_FOCUSED): return
            press(0xff09);time.sleep(.04)
        raise AssertionError('cannot focus '+node.name)
    def activate(node):
        action=node.queryAction()
        if action.nActions: assert action.doAction(0)
        else: focus(node);press(0x20)
    def field(name): return find(name,'text')
    def write(name,value):
        node=field(name)
        assert node.queryEditableText().setTextContents(value)
        return node
    def choose(name,index):
        row=find(name,'combo box')
        focus(row);press(0x20);time.sleep(.2)
        press(0xff50)
        for _ in range(index): press(0xff54)
        press(0xff0d);time.sleep(.2)
    def toggle(name):
        row=wait(lambda: next((n for n in nodes() if n.name==name and n.getRoleName()=='switch'
            and n.getState().contains(pyatspi.STATE_FOCUSABLE)),None),'switch '+name)
        activate(row)
    def close_dialog(): press(0xff1b);time.sleep(.4)
    def is_portal(node):
        app=node.getApplication()
        return app is not None and app.name=='xdg-desktop-portal-gtk'
    def open_filter_chooser():
        focus(find('Импортировать список фильтров…','list item'));press(0xff0d)
        wait(lambda: any(is_portal(n) and
            n.getRoleName()=='file chooser' for n in nodes()),'real filter file portal')
        time.sleep(.4)
    def select_filter(path):
        open_filter_chooser()
        key(0xffe3,True);press(ord('l'));key(0xffe3,False)
        location=wait(lambda: next((n for n in nodes() if is_portal(n)
            and n.getRoleName() in ('text','entry') and n.getState().contains(pyatspi.STATE_FOCUSED)),None),
            'filter portal location')
        assert location.queryEditableText().setTextContents(str(path))
        press(0xff0d);time.sleep(.6)
        if any(is_portal(n) and n.getRoleName()=='file chooser' for n in nodes()):
            press(0xff0d)
        wait(lambda: not any(is_portal(n) and
            n.getRoleName()=='file chooser' for n in nodes()),'filter portal closed')
    checks=result['checks']
    row=wait(lambda: next((n for n in nodes() if n.getRoleName()=='list item' and n.name.startswith('Альфа\n')),None),'Alpha row')
    focus(row);press(0xff0d)
    activate(find('Разрешения','list item'))
    find('Камера','combo box')
    assert not any(n.name=='Сохранить' and n.getRoleName()=='button' for n in nodes())
    choose('Камера',1)
    wait(lambda: policy()['permissions']['https://alpha.example']['camera']=='allow','camera permission saved without Save')
    checks['permission_choice_saves_immediately']=True
    # Simulate another window granting microphone access while this dialog is open.
    current=policy();current['permissions']['https://alpha.example']['microphone']='block';path.write_text(json.dumps(current))
    choose('Уведомления',2)
    wait(lambda: policy()['permissions']['https://alpha.example']['notifications']=='block','notification permission saved')
    assert policy()['permissions']['https://alpha.example']['microphone']=='block'
    checks['permission_edit_preserves_concurrent_grant']=True
    original=policy()
    path.rename(path.with_suffix('.saved'));path.mkdir()
    try:
        choose('Камера',2)
        wait(lambda: any(n.name.startswith('Не удалось сохранить изменения.') for n in nodes()),'permission write failure inline')
    finally:
        path.rmdir();path.with_suffix('.saved').rename(path)
    assert policy()==original
    checks['permission_failed_write_keeps_saved_policy']=True
    activate(find('Сбросить всё','button'))
    wait(lambda: not policy()['permissions'],'reset permissions persisted')
    assert policy()['content_filters'] and policy()['navigation']==original['navigation']
    assert not any(n.name.startswith('Не удалось сохранить изменения.') for n in nodes())
    choose('Камера',1)
    wait(lambda: policy()['permissions'].get('https://alpha.example',{}).get('camera')=='allow','choice after reset persists')
    checks['permission_reset_and_new_choice_preserve_other_policy']=True
    close_dialog()
    activate(find('Приватность и питание','list item'))
    find('Ограничить навигацию')
    assert not any(n.name=='Сохранить' and n.getRoleName()=='button' for n in nodes())
    toggle('Ограничить навигацию')
    wait(lambda: policy()['navigation']['enabled'],'navigation switch saved')
    assert 'https://alpha.example' in policy()['navigation']['allowed_origins']
    checks['navigation_switch_preserves_start_origin']=True
    origins=write('Разрешённые сайты','https://www.gnome.org, https://www.gnome.org/path')
    focus(origins);press(0xff0d)
    wait(lambda: set(policy()['navigation']['allowed_origins'])=={'https://alpha.example','https://www.gnome.org'},'origins save and normalize')
    checks['navigation_origins_save_on_enter']=True
    origins=write('Разрешённые сайты','https://www.gnome.org, https://developer.mozilla.org')
    focus(origins)
    toggle('Ограничить навигацию')
    wait(lambda: not policy()['navigation']['enabled'] and
        'https://developer.mozilla.org' in policy()['navigation']['allowed_origins'],
        'disabling navigation saves its valid text first')
    toggle('Ограничить навигацию')
    wait(lambda: policy()['navigation']['enabled'],'navigation restored')
    checks['disabling_navigation_keeps_valid_draft']=True
    write('Разрешённые сайты','invalid origin')
    find('Введите адреса сайтов HTTP или HTTPS через запятую.')
    choose('Режим прокси',1)
    wait(lambda: policy()['proxy']['mode']=='no_proxy','proxy saves despite invalid origin field')
    checks['invalid_origins_do_not_block_other_settings']=True
    # Discard invalid text through the actual close-attempt handler.
    close_dialog()
    find('Отменить несохранённые изменения?')
    activate(find('Отменить изменения','button'));time.sleep(.4)
    activate(find('Приватность и питание','list item'))
    assert field('Разрешённые сайты').queryText().getText(0,-1).find('www.gnome.org')>=0
    assert policy()['proxy']['mode']=='no_proxy'
    checks['policy_discard_retains_saved_changes']=True
    choose('Режим прокси',2)
    uri=write('Адрес прокси','http://127.0.0.1:8080')
    focus(uri);press(0xff0d)
    wait(lambda: policy()['proxy']==dict(mode='custom',uri='http://127.0.0.1:8080'),'custom proxy saved')
    uri=write('Адрес прокси','socks5://127.0.0.1:1080')
    focus(uri);press(0xff09)
    wait(lambda: policy()['proxy']['uri']=='socks5://127.0.0.1:1080','proxy saves on blur')
    checks['proxy_saves_on_enter_and_blur']=True
    choose('Режим прокси',0)
    wait(lambda: policy()['proxy']==dict(mode='system',uri=None),'system proxy resets URI')
    toggle('Проверочный фильтр')
    wait(lambda: not policy()['content_filters']['policyfilter']['enabled'],'filter switch saved')
    activate(find('Удалить фильтр','button'))
    wait(lambda: not policy()['content_filters'],'filter removal saved')
    checks['content_filter_toggle_and_removal_save']=True
    open_filter_chooser();press(0xff1b)
    wait(lambda: not any(n.getRoleName()=='file chooser' for n in nodes()),'filter import cancelled')
    time.sleep(.3)
    assert not policy()['content_filters']
    assert not any(n.name.startswith('Не удалось сохранить изменения.') for n in nodes())
    checks['filter_import_cancellation_keeps_policy']=True
    invalid=runtime/'invalid-filter.json';invalid.write_text('not JSON')
    select_filter(invalid)
    wait(lambda: any(n.name.startswith('Не удалось сохранить изменения.') for n in nodes()),'invalid filter inline error')
    assert not policy()['content_filters']
    valid=runtime/'imported-filter.json'
    valid.write_text(json.dumps([{'trigger':{'url-filter':'.*tracker.*'},'action':{'type':'block'}}]))
    select_filter(valid)
    wait(lambda: len(policy()['content_filters'])==1,'validated imported filter saved immediately')
    saved_filter=next(iter(policy()['content_filters'].values()))
    assert saved_filter['name']=='imported-filter' and saved_filter['enabled']
    assert not any(n.name.startswith('Не удалось сохранить изменения.') for n in nodes())
    checks['real_portal_filter_validation_and_import']=True
    toggle('Продолжать работу в фоне')
    # Test the actual desktop portal outcome; no host desktop bus is used.
    wait(lambda: policy()['background']['enabled'] or any(n.name.startswith('Не удалось сохранить изменения.') for n in nodes()),'background portal response',25)
    result['background_portal_enabled']=policy()['background']['enabled']
    result['background_portal_errors']=[n.name for n in nodes() if n.name.startswith('Не удалось сохранить изменения.')]
    if policy()['background']['enabled']:
        toggle('Продолжать работу в фоне')
        wait(lambda: not policy()['background']['enabled'],'background disabled again')
    assert not policy()['background']['autostart']
    background=find('Продолжать работу в фоне','switch')
    assert not background.getState().contains(pyatspi.STATE_CHECKED)
    checks['background_portal_result_matches_saved_state']=True
    write('Разрешённые сайты','https://www.gnome.org, https://developer.mozilla.org, https://en.wikipedia.org')
    key(0xffe3,True);press(ord('q'));key(0xffe3,False)
    wait(lambda: not any(n.name=='Режим прокси' for n in nodes()),'Ctrl+Q closes policy dialog first')
    find('Название','text')
    assert 'https://en.wikipedia.org' in policy()['navigation']['allowed_origins']
    checks['closing_policy_saves_valid_text_without_enter']=True
    checks['manager_quit_closes_policy_dialog_before_window']=True
    result['saved_policy']=policy()
