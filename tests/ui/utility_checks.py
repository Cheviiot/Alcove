#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Exercise utility dialogs and real backup/restore portals in disposable data."""
import json
import time


def run(nodes,find,wait,press,key,result,runtime):
    import pyatspi
    checks=result['checks']
    apps=runtime/'data/alcove/apps'
    def focus(node):
        for _ in range(90):
            node.clear_cache_single()
            if node.getState().contains(pyatspi.STATE_FOCUSED): return
            press(0xff09);time.sleep(.03)
        raise AssertionError('cannot focus '+node.name)
    def activate(node):
        actions=node.queryAction()
        if actions.nActions: assert actions.doAction(0)
        else: focus(node);press(0x20)
    def click(name,role='button'): activate(find(name,role))
    def menu(name):
        # GTK exports menu-item text through labels/relations rather than the
        # focused object's name. Exercise the actual ordered keyboard menu;
        # each caller verifies the resulting dialog and Orca reads its labels.
        position=['Сортировка','Дополнения','Резервная копия…','Восстановить…',
            'Системные возможности','Комбинации клавиш','О Alcove'].index(name)
        press(0xffc7);time.sleep(.25)
        press(0xff51);press(0xff50)
        for _ in range(position): press(0xff54)
        press(0xff0d);time.sleep(.3)
    def portal(node):
        app=node.getApplication()
        return app is not None and app.name=='xdg-desktop-portal-gtk'
    def chooser():
        return next((n for n in nodes() if portal(n) and n.getRoleName()=='file chooser'),None)
    def choose_path(path):
        wait(chooser,'actual file portal')
        time.sleep(.4)
        key(0xffe3,True);press(ord('l'));key(0xffe3,False)
        entry=wait(lambda: next((n for n in nodes() if portal(n) and
            n.getRoleName() in ('entry','text') and n.getState().contains(pyatspi.STATE_FOCUSED)),None),'portal location field')
        assert entry.queryEditableText().setTextContents(str(path))
        press(0xff0d);time.sleep(.5)
        if chooser(): press(0xff0d)
        wait(lambda: not chooser(),'file portal closes')
    def close():
        press(0xff1b);time.sleep(.4)
    def text_field(name):
        return wait(lambda: next((n for n in nodes() if n.name==name and n.getRoleName() in
            ('text','password text')),None),'text field '+name)
    def write(name,value):
        entry=text_field(name);focus(entry)
        assert entry.queryEditableText().setTextContents(value)
        return entry
    def toggle(name):
        node=wait(lambda: next((n for n in nodes() if n.name==name and n.getRoleName()=='switch' and
            n.getState().contains(pyatspi.STATE_FOCUSABLE)),None),'switch '+name)
        focus(node);press(0x20)
    def sensitive(name):
        node=find(name,'button');node.clear_cache_single()
        return node.getState().contains(pyatspi.STATE_SENSITIVE)
    def records():
        return [json.loads(p.read_text()) for p in apps.glob('*/app.json')
            if p.parent.name!='invalid00000' and p.parent.name==json.loads(p.read_text()).get('id')]
    menu('Дополнения')
    find('WebKitGTK');find('Установить','button')
    assert not any(n.name=='Удалить' for n in nodes())
    checks['optional_addon_has_native_install_action']=True
    close()
    menu('Системные возможности');find('Dynamic Launcher')
    old_retry=find('Повторить','button')
    click('Повторить')
    wait(lambda: sensitive('Повторить'),'capabilities refresh finished')
    assert find('Повторить','button')==old_retry
    find('Dynamic Launcher');find('Выбор файлов')
    checks['capabilities_load_and_refresh']=True
    close()
    menu('Комбинации клавиш');find('Добавить новое приложение')
    find('Поиск приложений')
    checks['manager_help_exposes_its_shortcuts']=True
    close()
    menu('О Alcove');find('Alcove');find('Cheviiot')
    checks['native_about_dialog_opens']=True
    close()
    click('Подробности')
    find('Диагностика данных приложений')
    wait(lambda: any('invalid00000' in n.name and 'app.json' in n.name for n in nodes()),
        'diagnostics exposes the invalid file')
    checks['diagnostics_exposes_corrupt_application_without_hiding_valid_apps']=True
    close()
    menu('Резервная копия…')
    find('Включить данные сайтов')
    toggle('Включить данные сайтов')
    assert not sensitive('Создать копию')
    write('Парольная фраза','utility audit passphrase')
    write('Подтвердите парольную фразу','mismatch')
    find('Пароли не совпадают.')
    assert not sensitive('Создать копию')
    write('Подтвердите парольную фразу','utility audit passphrase')
    wait(lambda: sensitive('Создать копию'),'matching password permits backup')
    checks['backup_encryption_validation_is_inline']=True
    click('Создать копию')
    encrypted=runtime/'encrypted.alcove-backup';choose_path(encrypted)
    wait(lambda: encrypted.exists(),'encrypted archive saved',30)
    assert encrypted.read_bytes().startswith(b'age-encryption.org/v1')
    menu('Восстановить…');choose_path(encrypted)
    find('Зашифрованная резервная копия')
    assert not sensitive('Открыть')
    write('Парольная фраза','incorrect audit passphrase');click('Открыть')
    find('Не удалось открыть копию. Проверьте парольную фразу и попробуйте снова.')
    write('Парольная фраза','utility audit passphrase');click('Открыть')
    find('Восстановление приложений')
    find('Восстановить отдельной копией')
    assert len(records())==3
    checks['encrypted_backup_password_retry_keeps_selected_archive']=True
    close()
    # Cancel before choosing a destination: no archive or record is written.
    menu('Резервная копия…');close();assert len(records())==3
    menu('Резервная копия…')
    click('Создать копию')
    wait(chooser,'backup save portal');press(0xff1b)
    wait(lambda: not chooser(),'backup save cancelled')
    assert len(records())==3
    checks['backup_portal_cancellation_preserves_apps']=True
    menu('Резервная копия…');click('Создать копию')
    archive=runtime/'roundtrip.alcove-backup';choose_path(archive)
    wait(lambda: archive.exists() and archive.stat().st_size>0,'real backup archive saved',20)
    checks['backup_uses_actual_file_portal']=True
    menu('Восстановить…');choose_path(archive)
    find('Такое же приложение уже существует')
    assert not sensitive('Восстановить')
    checks['identical_restore_has_no_selectable_apps']=True
    close()
    # Keep one absent application, one ID conflict, and one identical record.
    (apps/'library00000').rename(runtime/'removed-alpha')
    profile=runtime/'data/alcove/profiles/library00000'
    if profile.exists(): profile.rename(runtime/'removed-alpha-profile')
    beta_path=apps/'library00001/app.json';beta=json.loads(beta_path.read_text())
    beta['title']='Изменённая Бета';beta_path.write_text(json.dumps(beta))
    menu('Восстановить…');choose_path(archive)
    find('Восстановить приложение');find('Восстановить отдельной копией')
    check=find('Бета','check box');activate(check)
    check=find('Альфа','check box');activate(check)
    wait(lambda: not sensitive('Восстановить'),'empty selection disables restore')
    activate(check)
    checks['restore_selection_and_conflicts_are_accessible']=True
    click('Восстановить')
    wait(lambda: next((n for n in nodes() if portal(n) and n.name=='Create' and n.getRoleName()=='button'),None),
        'actual launcher confirmation')
    time.sleep(.3);press(0xff0d)
    wait(lambda: (apps/'library00000/app.json').exists(),'restored app committed',20)
    restored=json.loads((apps/'library00000/app.json').read_text())
    result['restored_app']=restored
    assert restored['title']=='Альфа' and restored['start_url'] in (
        'https://alpha.example','https://alpha.example/'), restored
    assert len(records())==3
    assert json.loads(beta_path.read_text())['title']=='Изменённая Бета'
    assert any('library00000' in p.name for p in (runtime/'data').rglob('*.desktop'))
    find('alpha.example')
    checks['restore_roundtrip_installs_launcher_and_keeps_unselected_app']=True
