<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Chromium add-on for Alcove

This directory contains the Chromium engine published as the
`io.github.cheviiot.alcove.Chromium` Flatpak add-on. It is built from the same
repository and release as Alcove, but its large Electron payload is not stored
inside the main application ref.

The engine is activated on demand through the private
`io.github.cheviiot.alcove.Chromium` D-Bus service exported by Alcove. Flatpak
mounts the add-on at `/app/extensions/chromium`. WebKitGTK remains the default,
and Alcove uses Chromium only after an explicit user choice.

The C broker validates every request, serializes profile operations, and starts
one sandboxed Electron process per Alcove application ID. The JavaScript layer
uses `nodeIntegration=false`, `contextIsolation=true`, `sandbox=true`, and
`webSecurity=true`.

Run the non-rendered engine tests inside `alcove-dev`:

```sh
node chromium/tests/validate.test.js
node chromium/tests/navigation-policy.test.js
node chromium/tests/proxy.test.js
```

Rendered checks must use Xvfb with the Wayland socket disabled.
