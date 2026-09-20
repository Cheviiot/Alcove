#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Read the isolated session's real AT-SPI tree, independently of CEF events."""
import argparse
import json
import pathlib
import time
import traceback

parser = argparse.ArgumentParser()
parser.add_argument("--output", type=pathlib.Path, required=True)
parser.add_argument("--delay", type=float, default=6)
parser.add_argument("--navigate", action="store_true")
args = parser.parse_args()
time.sleep(args.delay)

result = {}
interactions = {}
try:
    import pyatspi
    from gi.repository import GLib
    events = []
    def event_received(event):
        if len(events) >= 500:
            return
        record = {"type": event.type, "detail1": event.detail1, "detail2": event.detail2}
        # Defunct events are delivered after the object's remote endpoint can
        # disappear. Querying that endpoint can recursively generate more events.
        if event.type == "object:state-changed:defunct":
            events.append(record)
            return
        try:
            if event.source.getApplication().name in ("Bastle Chromium content", "bastle-native-chromium"):
                events.append(record | {"name": event.source.name})
        except GLib.Error:
            events.append(record | {"source_removed": True})
    pyatspi.Registry.registerEventListener(event_received, "object", "focus", "document")
    def settle(predicate=lambda: False, timeout=.5):
        until = time.monotonic() + timeout
        context = GLib.MainContext.default()
        while time.monotonic() < until:
            for _ in range(100):
                if not context.pending():
                    break
                context.iteration(False)
            if predicate():
                return True
            time.sleep(.02)
        return bool(predicate())
    nodes = []
    objects = {}
    def removed(error):
        return any(text in str(error) for text in ("does not exist", "object no longer exists"))
    def visit(node, depth=0):
        try:
            visit_live(node, depth)
        except GLib.Error as error:
            if not removed(error):
                raise
    def visit_live(node, depth=0):
        if depth > 30 or len(nodes) >= 500:
            return
        nodes.append({"name": node.name, "role": node.getRoleName(), "depth": depth,
                      "application": node.getApplication().name if depth else ""})
        objects[(node.name, node.getRoleName())] = node
        for child in node:
            if child is not None:
                visit(child, depth + 1)
    visit(pyatspi.Registry.getDesktop(0))
    result = {"nodes": list(nodes),
              "document_embedded_in_gtk": any(n["role"] == "document web" and n["application"] == "bastle-native-chromium" for n in nodes),
              "web_document_exposed": any(n["role"] == "document web" for n in nodes),
              "web_input_exposed": any(n["name"] == "Проверка ввода" and n["role"] in ("entry", "text") for n in nodes),
              "web_button_exposed": any(n["name"] == "Проверить нажатие" and "button" in n["role"] for n in nodes)}
    interactions = {}
    for (name, role), node in objects.items():
        if name == "Проверка ввода" and role in ("entry", "text"):
            text = node.queryText()
            interactions["text"] = text.getText(0, -1)
            interactions["focus_accepted"] = bool(node.queryComponent().grabFocus())
            settle(lambda: node.getState().contains(pyatspi.STATE_FOCUSED))
            interactions["focused"] = bool(node.getState().contains(pyatspi.STATE_FOCUSED))
            interactions["caret_accepted"] = bool(text.setCaretOffset(2))
            settle(lambda: text.caretOffset == 2)
            interactions["caret_offset"] = text.caretOffset
            interactions["selection_accepted"] = bool(text.addSelection(1, 4))
            settle(lambda: text.getNSelections() > 0)
            interactions["selection"] = list(text.getSelection(0)) if text.getNSelections() else None
        if name == "Проверить переход" and role == "link":
            link = node.queryHyperlink()
            interactions["link_uri"] = link.getURI(0)
            interactions["link_target_matches"] = link.getObject(0) == node
        if name == "Проверить нажатие" and "button" in role:
            action = node.queryAction()
            interactions["action_names"] = [action.getName(i) for i in range(action.nActions)]
            interactions["button_action_accepted"] = bool(action.doAction(0)) if action.nActions else False
    settle()
    if args.navigate:
        old_document = next(node for (name, role), node in objects.items() if role == "document web")
        old_button = next(node for (name, role), node in objects.items() if name == "Проверить нажатие" and role == "button")
        link = objects[("Проверить переход", "link")]
        interactions["navigation_action_accepted"] = bool(link.queryAction().doAction(0))
        def find_document():
            nodes.clear()
            objects.clear()
            visit(pyatspi.Registry.getDesktop(0))
            return ("Переход выполнен", "document web") in objects
        interactions["navigation_document_replaced"] = settle(find_document, timeout=3)
        def previous_defunct():
            try:
                return bool(old_document.getState().contains(pyatspi.STATE_DEFUNCT))
            except GLib.Error as error:
                if removed(error):
                    return True
                raise
        interactions["previous_document_defunct"] = settle(previous_defunct)
        try:
            interactions["previous_button_action_rejected"] = not bool(old_button.queryAction().doAction(0))
        except GLib.Error as error:
            interactions["previous_button_action_rejected"] = removed(error)
            interactions["previous_button_endpoint_removed"] = str(error)
        back = objects.get(("Вернуться", "link"))
        if back:
            interactions["return_action_accepted"] = bool(back.queryAction().doAction(0))
            def returned():
                nodes.clear()
                objects.clear()
                visit(pyatspi.Registry.getDesktop(0))
                return ("Chromium внутри GTK", "document web") in objects
            interactions["return_document_restored"] = settle(returned, timeout=3)
    result["interactions"] = interactions
    result["events"] = events
    pyatspi.Registry.deregisterEventListener(event_received, "object", "focus", "document")
except Exception as error:
    result.update(error=str(error), traceback=traceback.format_exc(), interactions=interactions)
args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2))
