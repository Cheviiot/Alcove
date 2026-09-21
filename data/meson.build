# SPDX-License-Identifier: GPL-3.0-only

desktop_file = i18n.merge_file(
        input: 'io.github.cheviiot.alcove.desktop.in',
       output: 'io.github.cheviiot.alcove.desktop',
         type: 'desktop',
       po_dir: 'po',
      install: true,
  install_dir: get_option('datadir') / 'applications'
)

desktop_utils = find_program('desktop-file-validate', required: false)
if desktop_utils.found()
  test('Validate desktop file', desktop_utils, args: [desktop_file])
endif

appstream_file = i18n.merge_file(
        input: 'io.github.cheviiot.alcove.metainfo.xml.in',
       output: 'io.github.cheviiot.alcove.metainfo.xml',
       po_dir: 'po',
      install: true,
  install_dir: get_option('datadir') / 'metainfo'
)

appstreamcli = find_program('appstreamcli', required: false, disabler: true)
test('Validate appstream file', appstreamcli,
     args: ['validate', '--no-net', '--explain', appstream_file])

# The add-on is listed by software centres through its own AppStream data, so a
# mistake there is invisible in the application's own metadata.
test('Validate add-on appstream file', appstreamcli,
     args: ['validate', '--no-net', '--explain',
            meson.current_source_dir()
              / 'io.github.cheviiot.alcove.ChromiumNative.metainfo.xml'])

install_data('io.github.cheviiot.alcove.gschema.xml',
  install_dir: get_option('datadir') / 'glib-2.0' / 'schemas'
)

compile_schemas = find_program('glib-compile-schemas')
test('Validate schema file',
     compile_schemas,
     args: ['--strict', '--dry-run', meson.current_source_dir()])

test_schema = custom_target(
  'compile test schema',
  input: 'io.github.cheviiot.alcove.gschema.xml',
  output: 'gschemas.compiled',
  command: [
    compile_schemas,
    '--strict',
    '--targetdir', meson.current_build_dir(),
    meson.current_source_dir(),
  ],
  build_by_default: get_option('ui_tests'),
)

subdir('icons')
subdir('po')
