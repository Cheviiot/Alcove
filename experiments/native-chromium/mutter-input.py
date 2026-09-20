#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Provide virtual input only to the harness's private Mutter compositor."""
import argparse
import os
import signal
import time
from gi.repository import Gio, GLib

if os.environ.get("WAYLAND_DISPLAY") != "bastle-probe" or not os.path.basename(
    os.environ.get("XDG_RUNTIME_DIR", "")
).startswith("bastle-native-"):
    raise SystemExit("Refusing input outside the isolated Bastle session")

parser = argparse.ArgumentParser()
parser.add_argument("--keyboard-check", action="store_true")
args = parser.parse_args()
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
service = "org.gnome.Mutter.RemoteDesktop"
path = "/org/gnome/Mutter/RemoteDesktop"
session, = bus.call_sync(service, path, service, "CreateSession", None,
                        GLib.VariantType.new("(o)"), Gio.DBusCallFlags.NONE, 3000, None).unpack()

def call(method, signature=None, values=()):
    return bus.call_sync(service, session, service + ".Session", method,
                         GLib.Variant(signature, values) if signature else None,
                         None, Gio.DBusCallFlags.NONE, 3000, None)

def stop_on_signal(*_args):
    raise SystemExit(0)
signal.signal(signal.SIGTERM, stop_on_signal)

try:
    call("Start")
    time.sleep(1.5)
    call("NotifyPointerMotionRelative", "(dd)", (-10000., -10000.))
    call("NotifyPointerMotionRelative", "(dd)", (640., 450.))
    call("NotifyPointerButton", "(ib)", (0x110, True))
    call("NotifyPointerButton", "(ib)", (0x110, False))
    # Create a virtual keyboard so GTK/Orca receive a real Wayland seat.
    call("NotifyKeyboardKeysym", "(ub)", (0xFFE1, True))
    call("NotifyKeyboardKeysym", "(ub)", (0xFFE1, False))
    print("Private Mutter pointer and keyboard ready", flush=True)
    if args.keyboard_check:
        time.sleep(10.5)
        # The preceding external AT-SPI action focused the button. Go back to
        # the input, then forward to the button using compositor keyboard events.
        call("NotifyKeyboardKeysym", "(ub)", (0xFFE1, True))
        call("NotifyKeyboardKeysym", "(ub)", (0xFF09, True))
        call("NotifyKeyboardKeysym", "(ub)", (0xFF09, False))
        call("NotifyKeyboardKeysym", "(ub)", (0xFFE1, False))
        time.sleep(3)
        call("NotifyKeyboardKeysym", "(ub)", (0xFF09, True))  # Tab: input -> button
        call("NotifyKeyboardKeysym", "(ub)", (0xFF09, False))
        time.sleep(3)
        call("NotifyKeyboardKeysym", "(ub)", (0xFF0D, True))  # Enter: activate button
        call("NotifyKeyboardKeysym", "(ub)", (0xFF0D, False))
        print("Orca keyboard check: Shift+Tab, Tab, Enter delivered through Mutter", flush=True)
    while True:
        time.sleep(1)
finally:
    call("Stop")
