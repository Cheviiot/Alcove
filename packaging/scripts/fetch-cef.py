#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Fetch the pinned upstream CEF SDK into ignored project build artifacts."""
import hashlib
import fcntl
import pathlib
import platform
import tarfile
import tempfile
import urllib.request

VERSION = "152.0.7+g83ffcba+chromium-152.0.7977.83"
ARCHIVES = {
    "x86_64": ("linux64", "0f64d01e1a5fe811a59f585176b45fe8d907aac8"),
    "aarch64": ("linuxarm64", "2dade4cfdfd55833e46e9efe1e95a2a052c3b821"),
}

def main():
    target, expected = ARCHIVES[platform.machine()]
    root = pathlib.Path(__file__).resolve().parents[2] / "build/native-chromium"
    root.mkdir(parents=True, exist_ok=True)
    # Build and diagnostic runs may ask for the same SDK concurrently.
    lock = (root / ".sdk.lock").open("w")
    fcntl.flock(lock, fcntl.LOCK_EX)
    name = f"cef_binary_{VERSION}_{target}_minimal"
    archive = root / f"{name}.tar.bz2"
    if not archive.exists():
        temporary = archive.with_suffix(".partial")
        urllib.request.urlretrieve(f"https://cef-builds.spotifycdn.com/{archive.name}", temporary)
        temporary.rename(archive)
    with archive.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha1").hexdigest()
    if actual != expected:
        raise RuntimeError("CEF archive does not match the pinned upstream checksum")
    destination = root / name
    if not destination.exists():
        with tempfile.TemporaryDirectory(prefix=".extract-", dir=root) as temporary:
            with tarfile.open(archive) as bundle:
                bundle.extractall(temporary, filter="data")
            pathlib.Path(temporary, name).rename(destination)
    print(destination)

if __name__ == "__main__":
    main()
