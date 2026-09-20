#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Independent creation checks used by the private native Wayland UI audit."""
import http.server
import json
import struct
import threading
import time
import zlib


def run(nodes, find, wait, press, key, result, runtime):
    import pyatspi

    requests=[]
    def chunk(kind,data):
        return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data))
    icon=(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!2I5B',32,32,8,2,0,0,0))+
        chunk(b'IDAT',zlib.compress((b'\0'+bytes([0,180,90])*32)*32))+chunk(b'IEND',b''))

    class Site(http.server.BaseHTTPRequestHandler):
        def log_message(self,*args): pass
        def do_GET(self):
            requests.append(self.path)
            if self.path.startswith('/slow'): time.sleep(3)
            if self.path=='/icon.png': status,kind,body=200,'image/png',icon
            elif self.path in ('/site','/slow'):
                status,kind,body=200,'text/html; charset=utf-8',(
                    '<title>Проверочный сайт Bastle</title><link rel="icon" href="/icon.png">').encode()
            else: status,kind,body=503,'text/plain',b'Offline fixture'
            try:
                self.send_response(status)
                self.send_header('Content-Type',kind)
                self.send_header('Content-Length',str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            except (BrokenPipeError,ConnectionResetError): pass

    server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Site)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    base='http://127.0.0.1:'+str(server.server_port)
    checks=result['checks']
    def field(name):
        return wait(lambda: next((n for n in nodes() if n.name==name and
            n.getRoleName() in ('entry','text')),None),'field: '+name)
    def read(node): return node.queryText().getText(0,-1)
    def app_records():
        # Transaction staging directories also contain app.json. Only the
        # final directory named after its ID proves the create committed.
        return [p for p in (runtime/'data/bastle/apps').glob('*/app.json')
            if p.parent.name==json.loads(p.read_text())['id']]
    def write(node,value): assert node.queryEditableText().setTextContents(value)
    def click(name):
        button=find(name,'button')
        assert button.getState().contains(pyatspi.STATE_SENSITIVE), 'disabled: '+name
        assert button.queryAction().doAction(0)
    def back():
        key(0xffe9,True);press(0xff51);key(0xffe9,False)
        field('Адрес сайта')
        time.sleep(.3)
    def next_address(value):
        write(field('Адрес сайта'),value)
        click('Далее')
    def review():
        return wait(lambda: next((n for n in nodes() if n.name=='Название' and
            n.getRoleName() in ('entry','text')),None),'review appears',22)

    try:
        key(0xffe3,True);press(ord('f'));key(0xffe3,False)
        write(field('Поиск по названию или домену'),'beta.example')
        wait(lambda: not any(n.name=='Альфа' for n in nodes()),'filter before creation')
        key(0xffe3,True);press(ord('n'));key(0xffe3,False)
        address=field('Адрес сайта')
        assert address.getState().contains(pyatspi.STATE_FOCUSED)
        assert not any(n.name=='Название' and n.getRoleName() in ('entry','text') for n in nodes())
        checks['creation_address_only_first_step']=True
        write(address,'file:///tmp/test')
        find('Введите правильный адрес HTTP или HTTPS.')
        assert not find('Далее','button').getState().contains(pyatspi.STATE_SENSITIVE)
        checks['creation_invalid_address_inline']=True
        write(address,base+'/site')
        press(0xff0d)  # Enter on the first step must advance, not create.
        title=review()
        wait(lambda: read(title)=='Проверочный сайт Bastle','site title appears')
        assert '/icon.png' in requests
        assert len(app_records())==3
        checks['creation_fetches_title_and_icon_before_create']=True
        write(title,'Моё новое приложение')
        back()
        assert read(field('Адрес сайта'))==base+'/site'
        click('Далее')
        title=review()
        assert read(title)=='Моё новое приложение' and requests.count('/site')==1
        checks['creation_back_preserves_edited_details']=True
        back()
        next_address(base+'/slow')
        click('Ввести вручную')
        title=review()
        write(title,'Ручное название')
        time.sleep(3.5)
        assert read(title)=='Ручное название'
        checks['creation_skip_discards_late_metadata']=True
        back()
        next_address(base+'/offline')
        title=review()
        find('Сайт недоступен. Приложение всё равно можно создать.')
        assert read(title)=='127.0.0.1'
        assert find('Создать','button').getState().contains(pyatspi.STATE_SENSITIVE)
        checks['creation_offline_can_continue']=True
        write(title,'   ')
        find('Введите название приложения.')
        assert not find('Создать','button').getState().contains(pyatspi.STATE_SENSITIVE)
        checks['creation_empty_name_inline']=True

        # Exercise the same HTTP/metadata code against actual public websites.
        real_sites=[]
        for url in ('https://www.gnome.org/','https://ru.wikipedia.org/wiki/GNOME'):
            back()
            next_address(url)
            title=review()
            actual=read(title)
            offline=any(n.name=='Сайт недоступен. Приложение всё равно можно создать.' for n in nodes())
            real_sites.append(dict(url=url,title=actual,metadata_loaded=not offline))
            assert actual and find('Создать','button').getState().contains(pyatspi.STATE_SENSITIVE)
        result['creation_real_sites']=real_sites
        assert any(site['metadata_loaded'] for site in real_sites), real_sites
        checks['creation_real_website_review']=True

        back()
        next_address(base+'/site')
        title=review()
        write(title,'Создано в проверке')
        click('Создать')
        # The real Dynamic Launcher portal is confined to the disposable bus
        # and XDG data. Inspect its confirmation or the native inline error.
        outcome=wait(lambda: next((n for n in nodes() if
            n.getApplication() and n.getApplication().name=='xdg-desktop-portal-gtk'
            and n.getRoleName()=='dialog'),None) or next((n for n in nodes() if
            n.name.startswith('Не удалось создать приложение.')),None),
            'launcher confirmation or error',20)
        result['creation_launcher_outcome']=dict(name=outcome.name,role=outcome.getRoleName())
        if outcome.name.startswith('Не удалось создать приложение.'):
            assert read(field('Название'))=='Создано в проверке'
            assert find('Создать','button').getState().contains(pyatspi.STATE_SENSITIVE)
            assert len(app_records())==3
            checks['creation_install_failure_preserves_draft']=True
            press(0xff1b);time.sleep(.3);press(0xff1b)
        else:
            def portal_button(name):
                return wait(lambda: next((n for n in nodes() if n.name==name and
                    n.getRoleName()=='button' and n.getApplication().name=='xdg-desktop-portal-gtk'),None),
                    'portal button: '+name)
            portal_button('Cancel')
            press(0xff1b)
            wait(lambda: find('Создать','button').getState().contains(pyatspi.STATE_SENSITIVE),
                 'create recovers after portal cancellation')
            assert read(field('Название'))=='Создано в проверке'
            assert len(app_records())==3
            checks['creation_portal_cancel_preserves_draft']=True
            click('Создать')
            portal_button('Create')
            time.sleep(.3)
            press(0xff0d)
            # The development binary is not installed in the container. GLib
            # rejects Exec=bastle until it is on the portal session's PATH.
            # Exercise this real failure, then provide the ordinary command
            # only inside the disposable environment and retry the same draft.
            wait(lambda: any(n.name.startswith('Не удалось создать приложение.') for n in nodes()),
                'missing launcher command reports an inline error',20)
            assert len(app_records())==3 and read(field('Название'))=='Создано в проверке'
            checks['creation_install_failure_preserves_draft']=True
            from pathlib import Path
            (runtime/'bin/bastle').symlink_to(Path(__file__).resolve().parents[2]/'build/src/bastle')
            click('Создать')
            portal_button('Create')
            time.sleep(.3)
            press(0xff0d)
            records=wait(lambda: app_records() if len(app_records())==4 else None,
                'real portal creation commits one app',20)
            created=next(json.loads(path.read_text()) for path in records
                if json.loads(path.read_text())['title']=='Создано в проверке')
            assert created['start_url']==base+'/site' and created['engine']=='webkit'
            assert created['sort_order']==21, 'newest order reused an existing position after deletions'
            wait(lambda: any(n.getRoleName()=='list item' and n.name.startswith('Создано в проверке\n')
                for n in nodes()),'new app appears in library')
            wait(lambda: len([n for n in nodes() if n.getRoleName()=='list item'])==4,
                'creation clears the previous library filter')
            wait(lambda: any(n.getRoleName()=='list item' and n.name.startswith('Создано в проверке\n')
                and n.getState().contains(pyatspi.STATE_FOCUSED) for n in nodes()),
                'created row receives keyboard focus')
            assert not any(n.getRoleName()=='dialog' and n.name=='Новое приложение' for n in nodes())
            launchers=list((runtime/'data').rglob('*.desktop'))
            installed=[p for p in launchers if created['id'] in p.name]
            assert installed, launchers
            desktop=installed[0].read_text()
            assert 'bastle '+created['id'] in desktop and 'Создано в проверке' in desktop
            icon_path=runtime/'data/bastle/apps'/created['id']/'icon.png'
            assert icon_path.exists() and icon_path.read_bytes().startswith(b'\x89PNG')
            checks['creation_real_portal_installs_launcher']=True
            checks['creation_returns_to_library_with_new_app']=True
            result['created_app']=dict(id=created['id'],title=created['title'],url=created['start_url'],
                launcher=str(installed[0].relative_to(runtime)))
    finally:
        result['creation_http_requests']=requests
        server.shutdown()
        server.server_close()
