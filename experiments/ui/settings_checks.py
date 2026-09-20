#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Exercise automatic saving in the real manager and inspect committed data."""
import json
import time


def run(nodes,find,wait,press,key,result,runtime):
    import pyatspi
    created=result['created_app']
    path=runtime/'data/bastle/apps'/created['id']/'app.json'
    def config(): return json.loads(path.read_text())
    def field(name):
        return wait(lambda: next((n for n in nodes() if n.name==name and
            n.getRoleName() in ('entry','text')),None),'settings field: '+name)
    def read(node): return node.queryText().getText(0,-1)
    def write(node,value): assert node.queryEditableText().setTextContents(value)
    def focus(node):
        # GTK4 does not implement Component.grabFocus for every entry. Use
        # actual compositor Tab events and verify the focused accessible node.
        for _ in range(60):
            node.clear_cache_single()
            if node.getState().contains(pyatspi.STATE_FOCUSED): return
            press(0xff09)
            time.sleep(.06)
        raise AssertionError('cannot focus '+node.name)
    def activate(node):
        action=node.queryAction()
        if action.nActions:
            assert action.doAction(0), (node.name,node.getRoleName())
        else:
            focus(node);press(0x20)
    def portal():
        return next((n for n in nodes() if n.getApplication() and
            n.getApplication().name=='xdg-desktop-portal-gtk' and n.getRoleName()=='dialog'),None)
    def portal_action(name):
        button=wait(lambda: next((n for n in nodes() if n.name==name and n.getRoleName()=='button'
            and n.getApplication().name=='xdg-desktop-portal-gtk'),None),'portal button: '+name)
        activate(button)
    def press_enter(node): focus(node);press(0xff0d)
    def toggle(name):
        row=wait(lambda: next((n for n in nodes() if n.name==name and n.getRoleName() in ('switch','toggle button') and n.getState().contains(pyatspi.STATE_FOCUSABLE)),None),'switch: '+name)
        activate(row)
    checks=result['checks']
    press(0xff0d)  # The new library row has keyboard focus after creation.
    title=field('Название')
    address=field('Адрес сайта')
    assert read(title)==created['title']
    assert not any(n.name in ('Сохранить','Изменить приложение') for n in nodes())
    checks['settings_fields_available_without_edit_mode']=True

    write(address,'https://www.gnome.org/')
    press_enter(address)
    wait(lambda: config()['start_url']=='https://www.gnome.org/','address saved on Enter')
    assert not portal(), 'runtime setting unnecessarily opened launcher portal'
    find('Изменения вступят в силу при следующем запуске приложения.')
    checks['settings_enter_saves_and_marks_restart']=True

    write(address,'https://ru.wikipedia.org/wiki/GNOME')
    focus(address)
    focus(title)
    wait(lambda: config()['start_url']=='https://ru.wikipedia.org/wiki/GNOME','address saved on blur')
    checks['settings_blur_saves_text']=True

    write(title,'')
    find('Введите название приложения.')
    toggle('Цвет заголовка')
    wait(lambda: not config()['use_theme_color'],'switch saved despite invalid name draft')
    assert config()['title']==created['title'] and read(title)==''
    assert not portal()
    checks['settings_invalid_field_does_not_block_switch']=True
    # A second switch change must also persist, not merely alter the widget.
    toggle('Цвет заголовка')
    wait(lambda: config()['use_theme_color'],'second switch save')

    write(title,'Переименовано в проверке')
    press_enter(title)
    wait(portal,'rename uses desktop confirmation')
    portal_action('Cancel')
    wait(lambda: any(n.name.startswith('Не удалось сохранить изменения.') for n in nodes()),'cancelled rename remains inline')
    assert config()['title']==created['title'] and read(title)=='Переименовано в проверке'
    checks['settings_cancelled_rename_keeps_draft']=True
    key(0xffe9,True);press(0xff51);key(0xffe9,False)
    find('Отменить несохранённые изменения?')
    assert not portal(), 'Back retried a failed rename instead of offering to leave'
    press(0xff1b)
    wait(lambda: not any(n.name=='Отменить несохранённые изменения?' for n in nodes()),'keep editing after cancelled rename')
    time.sleep(.35)
    checks['settings_failed_save_offers_discard_on_back']=True
    press_enter(title)
    wait(portal,'retry rename confirmation')
    portal_action('Create')
    wait(lambda: config()['title']=='Переименовано в проверке','rename commits after portal')
    desktop=(runtime/created['launcher']).read_text()
    assert 'Переименовано в проверке' in desktop
    checks['settings_rename_updates_launcher']=True

    # Native expander and its controls remain keyboard/AT-SPI reachable.
    advanced=wait(lambda: next((n for n in nodes() if n.name=='Дополнительно' and n.getRoleName() not in ('label','static')),None),'advanced expander')
    activate(advanced)
    toggle('Собственный User-Agent')
    agent=field('User-Agent')
    write(agent,'Bastle Settings Audit')
    press_enter(agent)
    wait(lambda: config()['user_agent']=='Bastle Settings Audit','custom user agent saved')
    toggle('Собственный User-Agent')
    wait(lambda: config()['user_agent'] is None,'custom agent disabled immediately')
    checks['settings_advanced_user_agent_saves']=True

    # Closing the manager is not allowed to silently lose an invalid draft.
    write(address,'file:///tmp/test')
    focus(address)
    key(0xffe3,True);press(ord('q'));key(0xffe3,False)
    find('Отменить несохранённые изменения?')
    press(0xff1b)
    wait(lambda: not any(n.name=='Отменить несохранённые изменения?' for n in nodes()),'discard dialog dismissed')
    time.sleep(.35)
    assert read(field('Адрес сайта'))=='file:///tmp/test'
    checks['settings_quit_protects_invalid_draft']=True
    # Correcting it and using Back commits on the way out, without a Save mode.
    write(address,'https://www.gnome.org/')
    key(0xffe9,True);press(0xff51);key(0xffe9,False)
    wait(lambda: any(n.getRoleName()=='list item' and n.name.startswith('Переименовано в проверке\n') for n in nodes()),'settings return to updated library')
    assert config()['start_url']=='https://www.gnome.org/'
    checks['settings_back_commits_and_updates_library']=True
    saved=config()
    row=wait(lambda: next((n for n in nodes() if n.getRoleName()=='list item'
        and n.name.startswith('Переименовано в проверке\n')),None),'renamed app row')
    focus(row);press(0xff0d)
    write(field('Название'),'')
    key(0xffe9,True);press(0xff51);key(0xffe9,False)
    find('Отменить несохранённые изменения?')
    activate(find('Отменить изменения','button'))
    wait(lambda: any(n.getRoleName()=='list item' and n.name.startswith('Переименовано в проверке\n')
        for n in nodes()),'discard returns to library')
    assert config()==saved, 'discard undid already saved settings'
    checks['settings_discard_preserves_saved_changes']=True
    result['saved_settings']=config()
