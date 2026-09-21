#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Render the real library under disposable Xvfb, including accessibility styles."""
import argparse
import datetime
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

ROOT=Path(__file__).resolve().parents[2]


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--screen',choices=('library','creation','settings','policy','utilities'),default='library')
    parser.add_argument('--high-contrast',action='store_true')
    parser.add_argument('--large-text',action='store_true')
    args=parser.parse_args()
    variant=(args.screen+'-' if args.screen!='library' else '')+('high-contrast' if args.high_contrast else 'normal')+('-large-text' if args.large_text else '')
    output=ROOT/'build/ui-renders'/(datetime.datetime.now().strftime('%Y%m%d-%H%M%S-')+variant)
    output.mkdir(parents=True)
    with tempfile.TemporaryDirectory(prefix='alcove-ui-') as runtime:
        env=os.environ.copy()
        for key in ('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY','AT_SPI_BUS_ADDRESS','DBUS_SESSION_BUS_ADDRESS',
                    'DBUS_STARTER_ADDRESS','DBUS_STARTER_BUS_TYPE','GTK_THEME','ADW_DEBUG_HIGH_CONTRAST'):
            env.pop(key,None)
        env.update(XDG_RUNTIME_DIR=runtime,XDG_CONFIG_HOME=runtime+'/config',XDG_DATA_HOME=runtime+'/data',
            XDG_CACHE_HOME=runtime+'/cache',XDG_DATA_DIRS='/usr/local/share:/usr/share',
            GDK_BACKEND='x11',GSK_RENDERER='cairo',GSETTINGS_BACKEND='memory',
            LANGUAGE='ru',LC_ALL='ru_RU.UTF-8',ALCOVE_TEST_LOCALEDIR=str(ROOT/'build/meson/data/po'),
            ALCOVE_TEST_RESOURCE=str(ROOT/'build/meson/src/alcove.gresource'),
            ALCOVE_UI_SCREENSHOTS=str(output),GSETTINGS_SCHEMA_DIR=str(ROOT/'build/meson/data'))
        settings=Path(runtime,'config/gtk-4.0/settings.ini')
        settings.parent.mkdir(parents=True)
        settings.write_text('[Settings]\ngtk-xft-dpi=98304\n'+('gtk-font-name=Adwaita Sans 20\n' if args.large_text else ''))
        if args.high_contrast: env['ADW_DEBUG_HIGH_CONTRAST']='1'
        process=None
        timed_out=False
        try:
            with (output/'render.log').open('w') as log:
                process=subprocess.Popen(['xvfb-run','-a','-s','-screen 0 1600x1200x24','dbus-run-session','--',
                    str(ROOT/'build/meson/src/alcove'),'--ui-test-'+args.screen],env=env,stdout=log,
                    stderr=subprocess.STDOUT,start_new_session=True)
                try: process.wait(timeout=75)
                except subprocess.TimeoutExpired: timed_out=True
        finally:
            # Own a separate process group so a failed test cannot leave its
            # X server, session bus or application running in the background.
            if process is not None:
                try: os.killpg(process.pid,signal.SIGTERM)
                except ProcessLookupError: pass
                try: process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid,signal.SIGKILL)
                    process.wait()
            mount=Path(runtime,'doc')
            deadline=time.monotonic()+5
            while mount.is_mount() and time.monotonic()<deadline: time.sleep(.05)
            if mount.is_mount(): subprocess.run(['fusermount3','-u',str(mount)],check=True,timeout=5)
    images=sorted(p.name for p in output.glob('*.png'))
    expected=({'library.png','library-dark.png','library-narrow.png','library-large-end.png'}
        if args.screen=='library' else {'backup-narrow.png','backup-dark.png','restore-narrow.png',
            'addons-missing-narrow.png','diagnostics-narrow.png','capabilities-narrow.png',
            'shortcuts-manager-narrow.png','about-narrow.png','downloads-empty-narrow.png',
            'downloads-narrow.png','backup-progress-narrow.png','restore-passphrase-narrow.png'}
        if args.screen=='utilities' else {'permissions.png','permissions-narrow.png','permissions-dark.png',
            'permissions-empty-narrow.png','privacy.png','privacy-narrow.png','privacy-dark.png',
            'privacy-proxy-narrow.png','privacy-background-narrow.png','privacy-filters-narrow.png'}
        if args.screen=='policy' else {'application.png','application-narrow.png','application-edit.png'}
        if args.screen=='settings' else {'create-address.png','create-review.png','create-review-narrow.png','create-review-dark.png'})
    passed=not timed_out and process.returncode==0 and expected<=set(images)
    (output/'report.json').write_text(json.dumps(dict(variant=variant,passed=passed,
        timed_out=timed_out,exit_code=process.returncode,images=images),indent=2))
    print(output)
    return 0 if passed else 1


if __name__=='__main__': raise SystemExit(main())
