#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Run inside bastle-dev; see README.md for the project-local prerequisites.
set -euo pipefail
cd "$(dirname "$0")/../.."
cef_probe_root=$(python3 experiments/native-chromium/fetch-cef.py)
cmake -S experiments/native-chromium -B build/native-chromium/worker -G Ninja \
  -DCEF_ROOT="$cef_probe_root" -DCMAKE_BUILD_TYPE=Release
cmake --build build/native-chromium/worker --parallel 6
mkdir -p build/native-chromium/locale/ru/LC_MESSAGES
msgfmt po/ru.po -o build/native-chromium/locale/ru/LC_MESSAGES/bastle.mo
cargo build --locked --features native-chromium-probe --bin bastle-native-chromium

python3 - "$cef_probe_root" <<'PY_MANIFEST'
import json, pathlib, sys
root = pathlib.Path("build/native-chromium").resolve()
(root / "addon.json").write_text(json.dumps({
    "schema_version": 1, "worker_protocol": 4,
    "cef_version": "152.0.7+g83ffcba+chromium-152.0.7977.83",
    "worker": "worker/bastle-cef-worker",
    "cef_root": str(pathlib.Path(sys.argv[1]).resolve().relative_to(root)),
}, indent=2) + "\n")
PY_MANIFEST
