<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Alcove Flatpak repository

Tagged releases publish a small OSTree repository to GitHub Pages without a
`gh-pages` branch. Both repository metadata and application commits are signed
by the dedicated Alcove release key:

```text
FA64 0607 BEBF D82E 61EF  72EF 33AD DA41 5AF7 FE09
```

The public key is tracked as `alcove-repository.gpg`. The private key is kept
only in the `ALCOVE_FLATPAK_GPG_PRIVATE_KEY` GitHub Actions secret and the
owner's protected local release-key directory. Never commit or print it.

GitHub Pages receives an Actions deployment artifact, so publishing does not
create a persistent deployment branch. The repository contains the Alcove app
and its `io.github.cheviiot.alcove.ChromiumNative` add-on for both supported
architectures. The add-on is mounted by Flatpak at
`/app/extensions/chromium-native`; it is not a second application.

GNOME Platform is intentionally not mirrored: `alcove.flatpakref` points
Flatpak to the upstream runtime repository, while Alcove and its Chromium
add-on come only from the project-owned remote.
