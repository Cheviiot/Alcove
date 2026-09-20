#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
set -eu
native_root=/app/extensions/chromium-native
export ZYPAK_CEF_LIBRARY_PATH="$native_root/worker/libcef.so"
exec "$native_root/bin/zypak-wrapper" "$native_root/worker/alcove-cef-worker" "$@"
