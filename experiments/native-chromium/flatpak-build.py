#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Build the optional CEF extension; install only into a private test installation.

Run inside bastle-dev. The production manifest and ordinary installation stay
unchanged. The prototype is currently pinned and verified for x86_64 only.
"""
import argparse
import hashlib
import json
import os
import pathlib
import platform
import shutil
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUILD = ROOT / 'build/native-chromium/flatpak'
APP = 'io.github.cheviiot.bastle'
EXTENSION = APP + '.ChromiumNative'
PREFIX = '/app/extensions/chromium-native'
CEF_VERSION = '152.0.7+g83ffcba+chromium-152.0.7977.83'
CEF_ARCHIVE = f'cef_binary_{CEF_VERSION}_linux64_minimal.tar.bz2'
CEF_SHA256 = 'a75d8956901e1f91bad4f7151af1dc31aaa2d46888cd6af51adb0ecaa77f860f'
JSON_SHA256 = '42f6e95cad6ec532fd372391373363b62a14af6d771056dbfc86160e6dfff7aa'


def seed_archive(path, checksum, name):
    if not path.is_file():
        return
    with path.open('rb') as stream:
        if hashlib.file_digest(stream, 'sha256').hexdigest() != checksum:
            raise RuntimeError(f'cached source checksum mismatch: {path.name}')
    target = ROOT / '.flatpak-builder/downloads' / checksum / name
    if not target.exists():
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_suffix('.seed')
        shutil.copyfile(path, temporary)
        temporary.replace(target)


def manifest():
    base = json.loads((ROOT / 'build-aux' / (APP + '.Devel.json')).read_text())
    modules = {module['name']: module for module in base['modules']}
    zypak = modules['zypak']
    zypak['build-commands'][-1] = f'make FLATPAK_DEST={PREFIX} install'
    # The header-only nickle dependency needs no doctest checkout to build Zypak.
    # Pin both source commits instead of following their recursive test modules.
    zypak['sources'][0]['disable-submodules'] = True
    zypak['sources'].append({
        'type': 'git', 'url': 'https://github.com/refi64/nickle',
        'commit': '0ac7f5dbf659caa8d1d45cb57e942f2bc565da1e',
        'disable-submodules': True, 'dest': 'nickle',
    })
    native = {
        'name': 'bastle-native-chromium', 'buildsystem': 'simple',
        'build-commands': [
            'cmake -S . -B _build -G Ninja -DCEF_ROOT="$PWD/cef" -DCMAKE_BUILD_TYPE=Release',
            'cmake --build _build --parallel ${FLATPAK_BUILDER_N_JOBS}',
            f'install -Dm755 _build/bastle-cef-worker {PREFIX}/worker/bastle-cef-worker',
            f'install -d {PREFIX}/cef',
            f'cp -a cef/Release cef/Resources {PREFIX}/cef/',
            f'for library in {PREFIX}/cef/Release/*.so*; do ln -s ../cef/Release/"$(basename "$library")" {PREFIX}/worker/; done',
            f'ln -s ../cef/Release/v8_context_snapshot.bin {PREFIX}/worker/',
            f'ln -s ../cef/Release/chrome-sandbox {PREFIX}/worker/',
            f'ln -s ../cef/Resources/icudtl.dat ../cef/Resources/locales {PREFIX}/worker/',
            f'for resource in {PREFIX}/cef/Resources/*.pak; do ln -s ../cef/Resources/"$(basename "$resource")" {PREFIX}/worker/; done',
            f'install -Dm755 flatpak-worker.sh {PREFIX}/bin/bastle-cef-launch',
            f'install -Dm644 addon.json {PREFIX}/addon.json',
            f'install -Dm644 cef/LICENSE.txt {PREFIX}/share/licenses/cef/LICENSE.txt',
            f'install -d {PREFIX}/share/licenses',
            f'mv /app/share/licenses/{APP}/zypak {PREFIX}/share/licenses/',
        ],
        'sources': [
            *[{'type': 'file', 'path': str(ROOT / 'experiments/native-chromium' / name)}
              for name in ('CMakeLists.txt', 'worker.cc', 'atspi_export.h', 'gpu_bridge.h',
                  'native_accessibility.h', 'navigation_gate.h', 'permission_settings.h',
                  'site_requests.h', 'flatpak-worker.sh')],
            {'type': 'archive', 'url': 'https://cef-builds.spotifycdn.com/' + CEF_ARCHIVE,
             'sha256': CEF_SHA256, 'dest': 'cef'},
            {'type': 'inline', 'dest-filename': 'addon.json', 'contents': json.dumps({
                'schema_version': 1, 'worker_protocol': 4, 'cef_version': CEF_VERSION,
                'worker': 'bin/bastle-cef-launch', 'cef_root': 'cef',
            })},
        ],
    }
    app = modules['bastle']
    app['config-opts'].append('-Dnative_chromium=true')
    # A diagnostic executable is bundled only with this experimental build.
    app['post-install'] = [
        f'install -d {PREFIX}',
        'cargo build --offline --locked --release --features native-chromium-probe --bin bastle-native-chromium --manifest-path=/run/build/bastle/Cargo.toml --target-dir=/run/build/bastle/_flatpak_build/src',
        'install -Dm755 /run/build/bastle/_flatpak_build/src/release/bastle-native-chromium /app/bin/bastle-native-chromium',
        'install -Dm755 /run/build/bastle/experiments/native-chromium/flatpak-probe.sh /app/bin/bastle-native-probe',
    ]
    for module in (modules['bastle-chromium-broker'], app):
        for i, source in enumerate(module['sources']):
            if isinstance(source, str):
                module['sources'][i] = str(ROOT / 'build-aux' / source)
            elif 'path' in source:
                source['path'] = str((ROOT / 'build-aux' / source['path']).resolve())
    base['add-extensions'] = {EXTENSION: {
        'version': 'master', 'directory': 'extensions/chromium-native',
        'bundle': True, 'no-autodownload': True, 'autodelete': True,
    }}
    base['finish-args'].append(f'--env=BASTLE_NATIVE_CHROMIUM_ADDON={PREFIX}/addon.json')
    base['modules'] = [modules['blueprint-compiler'], modules['bastle-chromium-broker'],
        zypak, {
            'name': 'nlohmann-json', 'buildsystem': 'cmake-ninja',
            'config-opts': ['-DJSON_BuildTests=OFF', '-DJSON_Install=ON'],
            'cleanup': ['*'], 'sources': [{'type': 'archive',
                'url': 'https://github.com/nlohmann/json/releases/download/v3.12.0/json.tar.xz',
                'sha256': JSON_SHA256}],
        }, native, app]
    return base


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--install', action='store_true', help='install into build/native-chromium/flatpak/installation only')
    parser.add_argument('--manifest-only', action='store_true')
    parser.add_argument('--install-only', action='store_true', help='install the already built packages without rebuilding')
    parser.add_argument('--host-only', action='store_true', help='rebuild the host while retaining the already built CEF extension')
    args = parser.parse_args()
    if platform.machine() != 'x86_64':
        parser.error('this experimental Flatpak recipe currently supports x86_64 only')
    BUILD.mkdir(parents=True, exist_ok=True)
    if args.install_only:
        install()
        return
    recipe = BUILD / ('host-manifest.json' if args.host_only else 'manifest.json')
    content = manifest()
    if args.host_only:
        subprocess.run(['ostree', '--repo=' + str(BUILD / 'repo'), 'rev-parse',
            'runtime/' + EXTENSION + '/x86_64/master'], check=True, stdout=subprocess.DEVNULL)
        content['modules'] = [m for m in content['modules'] if m['name'] in
            ('blueprint-compiler', 'bastle-chromium-broker', 'bastle')]
        content['add-extensions'][EXTENSION].pop('bundle')
    recipe.write_text(json.dumps(content, indent=2) + '\n')
    if args.manifest_only:
        print(recipe)
        return
    seed_archive(ROOT / 'build/native-chromium' / CEF_ARCHIVE, CEF_SHA256, CEF_ARCHIVE)
    seed_archive(ROOT / 'build/native-chromium/flatpak-sources/json-3.12.0.tar.xz', JSON_SHA256, 'json.tar.xz')
    subprocess.run(['flatpak-builder', '--disable-rofiles-fuse', '--disable-updates',
        '--user', '--force-clean', '--jobs=4', '--repo=' + str(BUILD / 'repo'),
        str(BUILD / ('app-host' if args.host_only else 'app')), str(recipe)], cwd=ROOT, check=True)
    if args.install:
        install()
    print(BUILD)


def install():
    env = dict(os.environ, FLATPAK_USER_DIR=str(BUILD / 'installation'))
    # Distrobox exposes host system runtimes here. Reuse them read-only;
    # installing our app still targets only the project-local user installation.
    if pathlib.Path('/run/host/var/lib/flatpak').is_dir():
        env['FLATPAK_SYSTEM_DIR'] = '/run/host/var/lib/flatpak'
    subprocess.run(['flatpak', 'install', '--user', '--noninteractive', '--assumeyes',
        '--reinstall', str(BUILD / 'repo'), APP + '//master', EXTENSION + '//master'],
        env=env, check=True)


if __name__ == '__main__':
    main()
