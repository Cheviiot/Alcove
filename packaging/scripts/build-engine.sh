#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Builds the CEF worker and the diagnostic binary. Run inside alcove-dev; see
# tests/engine/README.md for the prerequisites.
set -euo pipefail
cd "$(dirname "$0")/../.."
cef_probe_root=$(python3 packaging/scripts/fetch-cef.py)
cmake -S src/cpp -B build/native-chromium/worker -G Ninja \
  -DCEF_ROOT="$cef_probe_root" -DCMAKE_BUILD_TYPE=Release
cmake --build build/native-chromium/worker --parallel 6
mkdir -p build/native-chromium/locale/ru/LC_MESSAGES
msgfmt data/po/ru.po -o build/native-chromium/locale/ru/LC_MESSAGES/alcove.mo
cargo build --locked --features native-chromium-probe --bin alcove-native-chromium

python3 - "$cef_probe_root" <<'PY_MANIFEST'
import json, pathlib, sys
root = pathlib.Path("build/native-chromium").resolve()
(root / "addon.json").write_text(json.dumps({
    "schema_version": 1, "worker_protocol": 4,
    "cef_version": "152.0.7+g83ffcba+chromium-152.0.7977.83",
    "worker": "worker/alcove-cef-worker",
    "cef_root": str(pathlib.Path(sys.argv[1]).resolve().relative_to(root)),
}, indent=2) + "\n")
PY_MANIFEST
