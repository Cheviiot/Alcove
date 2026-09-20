// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "atspi_export.h"
#include <deque>
#include <map>
#include <memory>

// CEF 152 calls its per-browser accessibility observer synchronously before
// applying renderer updates to native ATK objects. Root loadStart/loadComplete
// events then emit public ATK busy notifications in the same UI-thread stack.
// A one-batch ticket associates that root with the browser, without URL/title
// matching or private Chromium object layouts. The runtime version is pinned.
class NativeAccessibility {
 public:
  static void Initialize() {
    object_class_ = g_type_class_ref(ATK_TYPE_OBJECT);
    document_interface_ = g_type_default_interface_ref(ATK_TYPE_DOCUMENT);
    text_interface_ = g_type_default_interface_ref(ATK_TYPE_TEXT);
    for (const char* name : {"state-change", "children-changed", "property-change", "focus-event", "active-descendant-changed", "visible-data-changed"})
      Hook(ATK_TYPE_OBJECT, name);
    Hook(ATK_TYPE_DOCUMENT, "load-complete");
    for (const char* name : {"text-changed", "text-insert", "text-remove", "text-caret-moved", "text-selection-changed", "text-attributes-changed"})
      Hook(ATK_TYPE_TEXT, name);
  }
  static void Register(uint64_t view) { states_.try_emplace(view); }
  static void TreeChanged(uint64_t view, const nlohmann::json& batch) {
    ticket_ = {};
    if (!states_.contains(view)) return;
    states_[view].focus_dirty = true;
    const auto tree = batch.value("ax_tree_id", "");
    if (tree.empty()) return;
    auto& info = trees_[tree];
    info.view = view;
    for (const auto& update : batch.value("updates", nlohmann::json::array())) {
      if (update.contains("root_id")) info.root = update["root_id"].get<int>();
      if (update.contains("tree_data")) {
        const auto& data = update["tree_data"];
        info.top_level = !data.contains("parent_tree_id") || data["parent_tree_id"] == "";
      }
    }
    if (!info.top_level || !info.root) return;
    ticket_.view = view;
    ticket_.tree = tree;
    for (const auto& event : batch.value("events", nlohmann::json::array())) {
      if (event.value("id", 0) != info.root) continue;
      const auto type = event.value("event_type", "");
      if (type == "loadStart") ticket_.busy.push_back(true);
      if (type == "loadComplete") ticket_.busy.push_back(false);
    }
  }
  static void SetHostFocus(uint64_t view, bool focused) {
    if (auto found = states_.find(view); found != states_.end()) {
      found->second.host_focused = focused;
      found->second.focus_dirty = true;
      atspi_export::views[view].host_focused = focused;
    }
  }
  static void Pump() {
    ticket_ = {}; // No ticket may escape a CEF message-loop iteration.
    for (auto& [view, state] : states_) {
      if (state.publish_pending) {
        state.publish_pending = false;
        atspi_export::Publish(view, state.root);
        if (state.load_complete) {
          if (auto* proxy = atspi_export::Wrap(state.root)) g_signal_emit_by_name(proxy, "load-complete");
          state.load_complete = false;
        }
      }
      if (state.focus_dirty && state.root) {
        state.focus_dirty = false;
        unsigned visited = 0;
        auto* native = state.host_focused ? FindFocused(state.root, visited) : nullptr;
        auto* next = atspi_export::Wrap(native);
        if (next != state.focus_proxy) {
          const char* name = next ? atk_object_get_name(next) : "";
          std::cout << nlohmann::json({{"event", "native-accessibility-focus"}, {"view", view},
              {"name", name ? name : ""}, {"host_focused", state.host_focused}}).dump() << std::endl;
          if (state.focus_proxy) {
            atk_object_notify_state_change(state.focus_proxy, ATK_STATE_FOCUSED, FALSE);
            g_object_unref(state.focus_proxy);
          }
          state.focus_proxy = next ? ATK_OBJECT(g_object_ref(next)) : nullptr;
          if (state.focus_proxy) atk_object_notify_state_change(state.focus_proxy, ATK_STATE_FOCUSED, TRUE);
        }
        if (native) g_object_unref(native);
      }
    }
    auto signals = std::move(pending_text_);
    pending_text_.clear();
    for (auto& signal : signals)
      atspi_export::ForwardSignal(&signal->hint, signal->count, signal->values.data());
  }
  static void Inspect(uint64_t view) {
    nlohmann::json nodes = nlohmann::json::array();
    if (auto state = states_.find(view); state != states_.end()) Visit(state->second.root, nodes, 0);
    std::cout << nlohmann::json({{"event", "native-accessibility"}, {"view", view},
        {"signals", signals_}, {"nodes", nodes}}).dump() << std::endl;
  }
  static void Navigated(uint64_t view) {
    auto found = states_.find(view);
    if (found == states_.end()) return;
    auto& state = found->second;
    // Chromium can detach old ATK sources before the new load event arrives.
    // Retire those proxies at committed main-frame load start, retaining the
    // per-window socket while the next document is being constructed.
    if (state.focus_proxy) {
      atk_object_notify_state_change(state.focus_proxy, ATK_STATE_FOCUSED, FALSE);
      g_object_unref(state.focus_proxy);
      state.focus_proxy = nullptr;
    }
    atspi_export::Publish(view, nullptr);
    if (state.root) { g_object_unref(state.root); state.root = nullptr; }
    state.publish_pending = state.load_complete = false;
    if (ticket_.view == view) ticket_ = {};
  }
  static void Close(uint64_t view) {
    atspi_export::CloseView(view);
    states_.erase(view);
    std::erase_if(trees_, [view](const auto& item) { return item.second.view == view; });
    if (ticket_.view == view) ticket_ = {};
  }
  static void Shutdown() {
    for (const auto& [signal, hook] : hooks_) g_signal_remove_emission_hook(signal, hook);
    hooks_.clear();
    pending_text_.clear();
    atspi_export::Shutdown();
    states_.clear();
    trees_.clear();
    ticket_ = {};
    if (text_interface_) g_type_default_interface_unref(text_interface_);
    if (document_interface_) g_type_default_interface_unref(document_interface_);
    if (object_class_) g_type_class_unref(object_class_);
    text_interface_ = document_interface_ = object_class_ = nullptr;
  }
 private:
  struct State {
    AtkObject* root = nullptr;
    AtkObject* focus_proxy = nullptr;
    bool host_focused = false, focus_dirty = false, publish_pending = false, load_complete = false;
    ~State() { if (root) g_object_unref(root); if (focus_proxy) g_object_unref(focus_proxy); }
  };
  struct Tree { uint64_t view = 0; int root = 0; bool top_level = false; };
  struct Ticket { uint64_t view; std::string tree; std::deque<bool> busy; Ticket() : view(0) {} };
  struct Binding { uint64_t view; std::string tree; };
  static GQuark BindingKey() { return g_quark_from_static_string("bastle-native-atk-browser-binding-v3"); }
  struct PendingSignal {
    GSignalInvocationHint hint;
    guint count;
    std::array<GValue, 8> values{};
    PendingSignal(GSignalInvocationHint* source_hint, guint size, const GValue* source)
        : hint(*source_hint), count(size) {
      for (guint i = 0; i < count; ++i) {
        g_value_init(&values[i], G_VALUE_TYPE(&source[i])); g_value_copy(&source[i], &values[i]);
      }
    }
    ~PendingSignal() { for (guint i = 0; i < count; ++i) g_value_unset(&values[i]); }
  };
  static AtkObject* FindFocused(AtkObject* object, unsigned& visited) {
    if (!object || ++visited > 2048) return nullptr;
    for (int i = 0, count = atk_object_get_n_accessible_children(object); i < count && visited < 2048; ++i) {
      auto* child = atk_object_ref_accessible_child(object, i);
      auto* found = FindFocused(child, visited);
      if (child) g_object_unref(child);
      if (found) return found;
    }
    auto* states = atk_object_ref_state_set(object);
    if (!states) return nullptr; // Detached native ATK source during navigation.
    const bool focused = atk_state_set_contains_state(states, ATK_STATE_FOCUSED)
        && !atk_state_set_contains_state(states, ATK_STATE_DEFUNCT);
    g_object_unref(states);
    return focused ? ATK_OBJECT(g_object_ref(object)) : nullptr;
  }
  static void Visit(AtkObject* object, nlohmann::json& nodes, int depth) {
    if (!object || depth > 30 || nodes.size() >= 500) return;
    const auto* name = atk_object_get_name(object);
    const auto* role = atk_role_get_name(atk_object_get_role(object));
    nodes.push_back({{"name", name ? name : ""}, {"role", role ? role : ""},
        {"type", G_OBJECT_TYPE_NAME(object)}, {"depth", depth},
        {"text", bool(ATK_IS_TEXT(object))}, {"editable", bool(ATK_IS_EDITABLE_TEXT(object))},
        {"action", bool(ATK_IS_ACTION(object))}});
    const int count = std::min(atk_object_get_n_accessible_children(object), 500);
    for (int i = 0; i < count; ++i) {
      if (auto* child = atk_object_ref_accessible_child(object, i)) {
        Visit(child, nodes, depth + 1);
        g_object_unref(child);
      }
    }
  }
  static void Bind(AtkObject* object, bool busy) {
    if (!ticket_.view || ticket_.busy.empty() || ticket_.busy.front() != busy) return;
    ticket_.busy.pop_front();
    auto* binding = static_cast<Binding*>(g_object_get_qdata(G_OBJECT(object), BindingKey()));
    if (binding && (binding->view != ticket_.view || binding->tree != ticket_.tree)) {
      std::cout << nlohmann::json({{"event", "native-accessibility-binding-error"},
          {"view", ticket_.view}, {"message", "native root belongs to another CEF tree"}}).dump() << std::endl;
      ticket_ = {};
      return;
    }
    if (!binding) {
      binding = new Binding{ticket_.view, ticket_.tree};
      g_object_set_qdata_full(G_OBJECT(object), BindingKey(), binding,
          [](gpointer data) { delete static_cast<Binding*>(data); });
      std::cout << nlohmann::json({{"event", "native-accessibility-bound"},
          {"view", binding->view}, {"tree", binding->tree}}).dump() << std::endl;
    }
    auto& state = states_.at(binding->view);
    if (state.root != object) {
      if (state.root) g_object_unref(state.root);
      state.root = ATK_OBJECT(g_object_ref(object));
      state.publish_pending = true;
    }
  }
  static gboolean OnSignal(GSignalInvocationHint* hint, guint count, const GValue* values, gpointer) {
    ++signals_;
    if (count == 0 || !G_VALUE_HOLDS_OBJECT(&values[0])) return TRUE;
    auto* object = ATK_OBJECT(g_value_get_object(&values[0]));
    if (!object || g_type_is_a(G_OBJECT_TYPE(object), atspi_export::BaseType()) || ATK_IS_PLUG(object)) return TRUE;
    for (auto& [view, state] : states_) state.focus_dirty = true;
    const std::string_view signal(g_signal_name(hint->signal_id));
    const bool state_change = signal == "state-change" && count > 2 && G_VALUE_HOLDS_STRING(&values[1]);
    const bool native_focus = signal == "focus-event" ||
        (state_change && std::string_view(g_value_get_string(&values[1])) == "focused");
    if (atk_object_get_role(object) == ATK_ROLE_DOCUMENT_WEB && !atk_object_get_parent(object)) {
      if (state_change && std::string_view(g_value_get_string(&values[1])) == "busy" && G_VALUE_HOLDS_BOOLEAN(&values[2]))
        Bind(object, g_value_get_boolean(&values[2]));
      auto* binding = static_cast<Binding*>(g_object_get_qdata(G_OBJECT(object), BindingKey()));
      if (binding && signal == "load-complete") {
        if (auto state = states_.find(binding->view); state != states_.end() && state->second.publish_pending)
          state->second.load_complete = true;
      }
    }
    // Focus precedes caret/text events so Orca announces the correct source.
    if (signal.starts_with("text-") && count <= 8) {
      if (pending_text_.size() < 1024)
        pending_text_.push_back(std::make_unique<PendingSignal>(hint, count, values));
      else
        std::cout << nlohmann::json({{"event", "protocol-error"},
            {"message", "accessibility text queue overflow"}}).dump() << std::endl;
    } else if (!native_focus) atspi_export::ForwardSignal(hint, count, values);
    return TRUE;
  }
  static void Hook(GType type, const char* name) {
    const auto id = g_signal_lookup(name, type);
    if (id) hooks_.emplace_back(id, g_signal_add_emission_hook(id, 0, OnSignal, nullptr, nullptr));
  }
  inline static std::vector<std::pair<guint, gulong>> hooks_;
  inline static std::vector<std::unique_ptr<PendingSignal>> pending_text_;
  inline static std::map<uint64_t, State> states_;
  inline static std::map<std::string, Tree> trees_;
  inline static Ticket ticket_;
  inline static gpointer object_class_ = nullptr, document_interface_ = nullptr, text_interface_ = nullptr;
  inline static uint64_t signals_ = 0;
};
