#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Diagnostic entry point: all app state stays in this sandbox's temporary home.
set -eu
export XDG_CONFIG_HOME=/tmp/bastle-probe/config
export XDG_DATA_HOME=/tmp/bastle-probe/data
export XDG_CACHE_HOME=/tmp/bastle-probe/cache
export GSETTINGS_BACKEND=memory
export BASTLE_PROBE_LOCALEDIR=/app/share/locale
mkdir -p "$XDG_CONFIG_HOME/gtk-4.0" "$XDG_DATA_HOME" "$XDG_CACHE_HOME"
printf '[Settings]\ngtk-xft-dpi=98304\n' > "$XDG_CONFIG_HOME/gtk-4.0/settings.ini"
exec /app/bin/bastle-native-chromium "$@"
