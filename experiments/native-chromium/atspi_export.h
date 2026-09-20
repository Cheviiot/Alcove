// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <atk/atk.h>
#include <atk-bridge.h>
#include <array>
#include <functional>
#include <memory>
#include <cstdint>
#include <iostream>
#include <type_traits>
#include <utility>
#include <nlohmann/json.hpp>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

// An experimental public-ATK adapter. Web roles/text/actions remain Chromium's;
// proxies supply the missing OSR parent hierarchy. No Chromium GType vtable or
// private symbol is modified. A private toolkit application owns the objects;
// an AtkPlug connects this hierarchy to the GTK host via GtkAtSpiSocket.
namespace atspi_export {
struct Node { AtkObject parent; AtkObject* source; int kind; bool retired; bool defunct_notified; uint64_t view; };
struct NodeClass { AtkObjectClass parent; };
inline AtkObject* application = nullptr;
struct View {
  AtkObject* frame = nullptr;
  AtkObject* document = nullptr;
  bool host_focused = false;
  uint64_t generation = 0;
  std::function<bool(std::function<void(bool)>)> request_host_focus;
};
struct Plug { AtkPlug parent; uint64_t view; };
inline std::unordered_map<uint64_t, View> views;
inline bool initialized = false;
inline View* ViewFor(uint64_t id) {
  auto found = views.find(id);
  return found == views.end() ? nullptr : &found->second;
}
inline View* Owner(gpointer object) { return ViewFor(reinterpret_cast<Node*>(object)->view); }
inline View* PlugOwner(gpointer object) { return ViewFor(reinterpret_cast<Plug*>(object)->view); }
inline std::unordered_map<AtkObject*, AtkObject*> proxies;
inline std::unordered_map<unsigned, GType> types;
inline GObjectClass* parent_class = nullptr;
inline AtkObject* Wrap(AtkObject* source);
inline AtkObject* Source(gpointer object) { return reinterpret_cast<Node*>(object)->source; }
inline bool Retired(gpointer object) { return reinterpret_cast<Node*>(object)->retired; }
inline void NotifyDefunct(AtkObject* object) {
  auto* node = reinterpret_cast<Node*>(object);
  node->retired = true;
  // Chromium may already have invalidated the source before main-frame load
  // start. AT-SPI deregisters its weak reference on this signal, so forwarding
  // that event and later retiring the document must not emit it twice.
  if (std::exchange(node->defunct_notified, true)) return;
  atk_object_notify_state_change(object, ATK_STATE_DEFUNCT, TRUE);
}
inline bool BelongsTo(AtkObject* object, AtkObject* root) {
  for (unsigned depth = 0; object && depth < 512; ++depth) {
    if (object == root) return true;
    object = atk_object_get_parent(object);
  }
  return false;
}

// Forward scalar/string/text operations to the native ATK implementation. The
// proxy implements an interface only when its native object implements it.
// Member-pointer traits preserve the exact ABI, including void-returning ATK
// methods. Object-returning APIs need explicit wrapping and are handled below.
template<typename T> struct MemberType;
template<typename T, typename I> struct MemberType<T I::*> { using Interface = I; };
// Scalar pointer parameters in the forwarded ATK interfaces are outputs.
// Clear them on a defunct/unsupported call, never return uninitialized extents
// or text offsets to the AT-SPI adaptor.
template<typename T> inline void ClearOutput(T value) {
  if constexpr (std::is_pointer_v<T>) {
    using Pointee = std::remove_pointer_t<T>;
    if constexpr (std::is_arithmetic_v<Pointee> && !std::is_const_v<Pointee>)
      if (value) *value = Pointee{};
  }
}
template<auto GetType, auto Member, typename Result, typename Object, typename... Args>
inline Result Invoke(Object* proxy, Args... args) {
  auto* source = Source(proxy);
  if (!source || Retired(proxy)) { (ClearOutput(args), ...); return Result(); }
  using Interface = typename MemberType<decltype(Member)>::Interface;
  auto* iface = static_cast<Interface*>(g_type_interface_peek(G_OBJECT_GET_CLASS(source), GetType()));
  auto function = iface->*Member;
  if (function) return function(reinterpret_cast<Object*>(source), args...);
  (ClearOutput(args), ...);
  return Result();
}

// Helper specialization avoids type-erased casts on callback entrypoints.
template<auto GetType, auto Member, typename Signature> struct Delegate;
template<auto GetType, auto Member, typename R, typename O, typename... A>
struct Delegate<GetType, Member, R (*)(O*, A...)> {
  static R Call(O* object, A... args) { return Invoke<GetType, Member, R>(object, args...); }
};
#define AX_FORWARD(I, TYPE, METHOD) iface->METHOD = &Delegate<TYPE, &I::METHOD, decltype(I::METHOD)>::Call

inline void ActionInit(gpointer value, gpointer) {
  auto* iface = static_cast<AtkActionIface*>(value);
  iface->do_action = [](AtkAction* object, gint index) -> gboolean {
    auto* owner = Owner(object);
    if (!Source(object) || Retired(object) || !owner || !owner->request_host_focus) return FALSE;
    if (index < 0 || index >= atk_action_get_n_actions(ATK_ACTION(Source(object)))) return FALSE;
    auto held = std::shared_ptr<AtkObject>(ATK_OBJECT(g_object_ref(object)), [](AtkObject* o) { g_object_unref(o); });
    return owner->request_host_focus([held, index](bool focused) {
      if (focused) Invoke<atk_action_get_type, &AtkActionIface::do_action, gboolean>(ATK_ACTION(held.get()), index);
    });
  };
  AX_FORWARD(AtkActionIface, atk_action_get_type, get_n_actions);
  AX_FORWARD(AtkActionIface, atk_action_get_type, get_description);
  AX_FORWARD(AtkActionIface, atk_action_get_type, get_name);
  AX_FORWARD(AtkActionIface, atk_action_get_type, get_keybinding);
  AX_FORWARD(AtkActionIface, atk_action_get_type, get_localized_name);
}
inline void ComponentInit(gpointer value, gpointer) {
  auto* iface = static_cast<AtkComponentIface*>(value);
  AX_FORWARD(AtkComponentIface, atk_component_get_type, contains);
  AX_FORWARD(AtkComponentIface, atk_component_get_type, get_extents);
  iface->grab_focus = [](AtkComponent* object) -> gboolean {
    auto* owner = Owner(object);
    if (!Source(object) || Retired(object) || !owner || !owner->request_host_focus) return FALSE;
    auto held = std::shared_ptr<AtkObject>(ATK_OBJECT(g_object_ref(object)), [](AtkObject* o) { g_object_unref(o); });
    return owner->request_host_focus([held](bool focused) {
      if (focused) Invoke<atk_component_get_type, &AtkComponentIface::grab_focus, gboolean>(ATK_COMPONENT(held.get()));
    });
  };
  AX_FORWARD(AtkComponentIface, atk_component_get_type, get_layer);
  AX_FORWARD(AtkComponentIface, atk_component_get_type, get_mdi_zorder);
  AX_FORWARD(AtkComponentIface, atk_component_get_type, get_alpha);
  AX_FORWARD(AtkComponentIface, atk_component_get_type, scroll_to);
  AX_FORWARD(AtkComponentIface, atk_component_get_type, scroll_to_point);
  iface->ref_accessible_at_point = [](AtkComponent* object, gint x, gint y, AtkCoordType coords) {
    auto* native = atk_component_ref_accessible_at_point(ATK_COMPONENT(Source(object)), x, y, coords);
    auto* proxy = Wrap(native);
    if (proxy) g_object_ref(proxy);
    if (native) g_object_unref(native);
    return proxy;
  };
}
inline void TextInit(gpointer value, gpointer) {
  auto* iface = static_cast<AtkTextIface*>(value);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_text);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_text_after_offset);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_text_at_offset);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_text_before_offset);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_character_at_offset);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_caret_offset);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_run_attributes);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_default_attributes);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_character_extents);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_character_count);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_offset_at_point);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_n_selections);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_selection);
  AX_FORWARD(AtkTextIface, atk_text_get_type, add_selection);
  AX_FORWARD(AtkTextIface, atk_text_get_type, remove_selection);
  AX_FORWARD(AtkTextIface, atk_text_get_type, set_selection);
  AX_FORWARD(AtkTextIface, atk_text_get_type, set_caret_offset);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_range_extents);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_bounded_ranges);
  AX_FORWARD(AtkTextIface, atk_text_get_type, get_string_at_offset);
  AX_FORWARD(AtkTextIface, atk_text_get_type, scroll_substring_to);
  AX_FORWARD(AtkTextIface, atk_text_get_type, scroll_substring_to_point);
}
struct Link { AtkHyperlink parent; AtkHyperlink* source; uint64_t view; bool retired; };
inline std::unordered_map<AtkHyperlink*, AtkHyperlink*> links;
inline AtkHyperlink* LinkSource(AtkHyperlink* object) { return reinterpret_cast<Link*>(object)->source; }
inline GObjectClass* link_parent_class = nullptr;
inline GType LinkType() {
  static GType type = g_type_register_static_simple(ATK_TYPE_HYPERLINK, "BastleNativeAxLink",
      sizeof(AtkHyperlinkClass), [](gpointer value, gpointer) {
        auto* klass = static_cast<AtkHyperlinkClass*>(value);
        link_parent_class = G_OBJECT_CLASS(g_type_class_peek_parent(klass));
        G_OBJECT_CLASS(klass)->finalize = [](GObject* object) {
          if (auto* source = LinkSource(ATK_HYPERLINK(object))) g_object_unref(source);
          link_parent_class->finalize(object);
        };
        klass->get_uri = [](AtkHyperlink* object, gint index) { return atk_hyperlink_get_uri(LinkSource(object), index); };
        klass->get_object = [](AtkHyperlink* object, gint index) -> AtkObject* {
          if (reinterpret_cast<Link*>(object)->retired) return nullptr;
          return Wrap(atk_hyperlink_get_object(LinkSource(object), index));
        };
        klass->get_start_index = [](AtkHyperlink* object) { return atk_hyperlink_get_start_index(LinkSource(object)); };
        klass->get_end_index = [](AtkHyperlink* object) { return atk_hyperlink_get_end_index(LinkSource(object)); };
        klass->get_n_anchors = [](AtkHyperlink* object) { return atk_hyperlink_get_n_anchors(LinkSource(object)); };
        klass->is_valid = [](AtkHyperlink* object) -> gboolean { return !reinterpret_cast<Link*>(object)->retired && atk_hyperlink_is_valid(LinkSource(object)); };
        klass->is_selected_link = [](AtkHyperlink* object) -> gboolean {
          auto* native = LinkSource(object);
          auto function = ATK_HYPERLINK_GET_CLASS(native)->is_selected_link;
          return function ? function(native) : FALSE;
        };
        klass->link_state = [](AtkHyperlink* object) -> guint {
          return atk_hyperlink_is_inline(LinkSource(object)) ? ATK_HYPERLINK_IS_INLINE : 0;
        };
      }, sizeof(Link), nullptr, GTypeFlags(0));
  return type;
}
inline AtkHyperlink* WrapLink(AtkHyperlink* source, uint64_t view) {
  if (!source) return nullptr;
  if (auto found = links.find(source); found != links.end()) return found->second;
  auto* link = reinterpret_cast<Link*>(g_object_new(LinkType(), nullptr));
  link->source = ATK_HYPERLINK(g_object_ref(source));
  link->view = view;
  links.emplace(source, ATK_HYPERLINK(link));
  return ATK_HYPERLINK(link);
}
inline void HypertextInit(gpointer value, gpointer) {
  auto* iface = static_cast<AtkHypertextIface*>(value);
  AX_FORWARD(AtkHypertextIface, atk_hypertext_get_type, get_n_links);
  AX_FORWARD(AtkHypertextIface, atk_hypertext_get_type, get_link_index);
  iface->get_link = [](AtkHypertext* object, gint index) {
    if (Retired(object) || !Source(object)) return static_cast<AtkHyperlink*>(nullptr);
    return WrapLink(atk_hypertext_get_link(ATK_HYPERTEXT(Source(object)), index), reinterpret_cast<Node*>(object)->view);
  };
}
inline void HyperlinkInit(gpointer value, gpointer) {
  auto* iface = static_cast<AtkHyperlinkImplIface*>(value);
  iface->get_hyperlink = [](AtkHyperlinkImpl* object) -> AtkHyperlink* {
    if (Retired(object) || !Source(object)) return nullptr;
    auto* native = atk_hyperlink_impl_get_hyperlink(ATK_HYPERLINK_IMPL(Source(object)));
    auto* result = WrapLink(native, reinterpret_cast<Node*>(object)->view);
    if (result) g_object_ref(result);
    if (native) g_object_unref(native);
    return result;
  };
}
inline void DocumentInit(gpointer value, gpointer) {
  auto* iface = static_cast<AtkDocumentIface*>(value);
  AX_FORWARD(AtkDocumentIface, atk_document_get_type, get_document_locale);
  AX_FORWARD(AtkDocumentIface, atk_document_get_type, get_document_attributes);
  AX_FORWARD(AtkDocumentIface, atk_document_get_type, get_document_attribute_value);
  AX_FORWARD(AtkDocumentIface, atk_document_get_type, get_current_page_number);
  AX_FORWARD(AtkDocumentIface, atk_document_get_type, get_page_count);
}
#undef AX_FORWARD

inline void ClassInit(gpointer value, gpointer) {
  auto* klass = static_cast<AtkObjectClass*>(value);
  parent_class = G_OBJECT_CLASS(g_type_class_peek_parent(klass));
  G_OBJECT_CLASS(klass)->finalize = [](GObject* object) {
    if (auto* native = Source(object)) g_object_unref(native);
    parent_class->finalize(object);
  };
  klass->get_name = [](AtkObject* object) -> const gchar* {
    auto* node = reinterpret_cast<Node*>(object);
    if (node->kind == 1) return "Bastle Chromium content";
    if (node->kind == 2) return "Chromium внутри Bastle";
    return node->source ? atk_object_get_name(node->source) : "";
  };
  klass->get_description = [](AtkObject* object) -> const gchar* {
    return Source(object) ? atk_object_get_description(Source(object)) : "";
  };
  klass->get_role = [](AtkObject* object) -> AtkRole {
    auto* node = reinterpret_cast<Node*>(object);
    if (node->kind == 1) return ATK_ROLE_APPLICATION;
    if (node->kind == 2) return ATK_ROLE_FRAME;
    return node->source ? atk_object_get_role(node->source) : ATK_ROLE_UNKNOWN;
  };
  klass->get_parent = [](AtkObject* object) -> AtkObject* {
    if (Retired(object) || object == application) return nullptr;
    auto* owner = Owner(object);
    if (owner && Source(object) == owner->document) return owner->frame;
    return Wrap(atk_object_get_parent(Source(object)));
  };
  klass->get_n_children = [](AtkObject* object) -> gint {
    if (Retired(object) || object == application) return 0;
    return atk_object_get_n_accessible_children(Source(object));
  };
  klass->ref_child = [](AtkObject* object, gint index) -> AtkObject* {
    if (Retired(object) || index < 0) return nullptr;
    if (object == application) return nullptr;
    auto* native = atk_object_ref_accessible_child(Source(object), index);
    auto* result = Wrap(native);
    if (result) g_object_ref(result);
    if (native) g_object_unref(native);
    return result;
  };
  klass->get_index_in_parent = [](AtkObject* object) -> gint {
    if (Retired(object) || object == application) return -1;
    auto* owner = Owner(object);
    if (owner && Source(object) == owner->document) return 0;
    return atk_object_get_index_in_parent(Source(object));
  };
  klass->ref_state_set = [](AtkObject* object) {
    if (!Retired(object) && Source(object)) {
      auto* original = atk_object_ref_state_set(Source(object));
      if (!original) {
        auto* states = atk_state_set_new();
        atk_state_set_add_state(states, ATK_STATE_DEFUNCT);
        return states;
      }
      auto* states = atk_state_set_or_sets(original, original);
      g_object_unref(original);
      auto* owner = Owner(object);
      if (!owner || !owner->host_focused) atk_state_set_remove_state(states, ATK_STATE_FOCUSED);
      return states;
    }
    auto* states = atk_state_set_new();
    if (Retired(object)) { atk_state_set_add_state(states, ATK_STATE_DEFUNCT); return states; }
    for (auto state : {ATK_STATE_VISIBLE, ATK_STATE_SHOWING, ATK_STATE_ENABLED, ATK_STATE_SENSITIVE})
      atk_state_set_add_state(states, state);
    return states;
  };
  klass->get_attributes = [](AtkObject* object) -> AtkAttributeSet* {
    if (!Source(object)) return nullptr;
    auto* attributes = atk_object_get_attributes(Source(object));
    // Orca uses this per-object hint for a web engine embedded in a GTK app.
    auto* toolkit = g_new0(AtkAttribute, 1);
    toolkit->name = g_strdup("toolkit"); toolkit->value = g_strdup("Chromium");
    return g_slist_prepend(attributes, toolkit);
  };
  klass->get_object_locale = [](AtkObject* object) -> const gchar* {
    return Source(object) ? atk_object_get_object_locale(Source(object)) : "ru";
  };
  klass->ref_relation_set = [](AtkObject* object) -> AtkRelationSet* {
    auto* result = atk_relation_set_new();
    if (Retired(object) || !Source(object)) return result;
    auto* native = atk_object_ref_relation_set(Source(object));
    for (int i = 0; native && i < atk_relation_set_get_n_relations(native); ++i) {
      auto* relation = atk_relation_set_get_relation(native, i);
      auto* targets = atk_relation_get_target(relation);
      std::vector<AtkObject*> mapped;
      for (guint j = 0; targets && j < targets->len; ++j)
        if (auto* target = Wrap(ATK_OBJECT(targets->pdata[j]))) mapped.push_back(target);
      if (mapped.empty()) continue;
      auto* copy = atk_relation_new(mapped.data(), mapped.size(), atk_relation_get_relation_type(relation));
      atk_relation_set_add(result, copy);
      g_object_unref(copy);
    }
    if (native) g_object_unref(native);
    return result;
  };
}
inline GType BaseType() {
  static GType type = g_type_register_static_simple(ATK_TYPE_OBJECT, "BastleNativeAxProxy",
      sizeof(NodeClass), ClassInit, sizeof(Node), nullptr, GTypeFlags(0));
  return type;
}
inline AtkObject* Wrap(AtkObject* source) {
  if (!source) return nullptr;
  if (g_type_is_a(G_OBJECT_TYPE(source), BaseType()) || ATK_IS_PLUG(source)) return source;
  if (auto found = proxies.find(source); found != proxies.end()) return found->second;
  uint64_t owner = 0;
  for (const auto& [id, view] : views) if (BelongsTo(source, view.document)) { owner = id; break; }
  if (!owner) return nullptr;
  const unsigned mask = (ATK_IS_ACTION(source) ? 1 : 0) | (ATK_IS_COMPONENT(source) ? 2 : 0)
      | (ATK_IS_TEXT(source) ? 4 : 0) | (ATK_IS_DOCUMENT(source) ? 8 : 0)
      | (ATK_IS_HYPERTEXT(source) ? 16 : 0) | (ATK_IS_HYPERLINK_IMPL(source) ? 32 : 0);
  auto& type = types[mask];
  if (!type) {
    const auto name = "BastleNativeAxProxy" + std::to_string(mask);
    type = g_type_register_static_simple(BaseType(), name.c_str(), sizeof(NodeClass), nullptr,
        sizeof(Node), nullptr, GTypeFlags(0));
    const std::array<GType, 6> interfaces{ATK_TYPE_ACTION, ATK_TYPE_COMPONENT, ATK_TYPE_TEXT, ATK_TYPE_DOCUMENT, ATK_TYPE_HYPERTEXT, ATK_TYPE_HYPERLINK_IMPL};
    const std::array<GInterfaceInitFunc, 6> initializers{ActionInit, ComponentInit, TextInit, DocumentInit, HypertextInit, HyperlinkInit};
    for (unsigned i = 0; i < interfaces.size(); ++i) if (mask & (1u << i)) {
      const GInterfaceInfo info{initializers[i], nullptr, nullptr};
      g_type_add_interface_static(type, interfaces[i], &info);
    }
  }
  auto* node = reinterpret_cast<Node*>(g_object_new(type, nullptr));
  node->source = ATK_OBJECT(g_object_ref(source));
  node->view = owner;
  auto* object = ATK_OBJECT(node);
  proxies.emplace(source, object);
  return object;
}
// ATK's bridge installs global signal listeners. Export only the proxy side;
// allowing original objects through would duplicate events and expose an
// unparented second tree. These are public toolkit hooks, not Chromium hooks.
struct Listener { GSignalEmissionHook callback; std::string event; guint signal; gulong hook; };
inline std::unordered_map<guint, Listener*> listeners;
inline guint next_listener = 1;
inline guint AddListener(GSignalEmissionHook callback, const gchar* event) {
  gchar** parts = g_strsplit(event, ":", 0);
  const auto length = g_strv_length(parts);
  const auto type = length >= 3 ? g_type_from_name(parts[1]) : GType(0);
  const auto signal = type ? g_signal_lookup(parts[2], type) : 0;
  if (!signal || length > 4) { g_strfreev(parts); return 0; }
  auto* listener = new Listener{callback, event, signal, 0};
  listener->hook = g_signal_add_emission_hook(signal,
      length == 4 ? g_quark_from_string(parts[3]) : 0,
      [](GSignalInvocationHint* hint, guint count, const GValue* values, gpointer data) -> gboolean {
        auto* listener = static_cast<Listener*>(data);
        if (count && G_VALUE_HOLDS_OBJECT(&values[0])) {
          auto* object = g_value_get_object(&values[0]);
          if (object && (g_type_is_a(G_OBJECT_TYPE(object), BaseType()) || ATK_IS_PLUG(object)))
            listener->callback(hint, count, values, const_cast<char*>(listener->event.c_str()));
        }
        return TRUE;
      }, listener, [](gpointer data) { delete static_cast<Listener*>(data); });
  g_strfreev(parts);
  if (!listener->hook) { delete listener; return 0; }
  const auto id = next_listener++;
  listeners.emplace(id, listener);
  return id;
}
inline void RemoveListener(guint id) {
  auto found = listeners.find(id);
  if (found == listeners.end()) return;
  auto* listener = found->second;
  listeners.erase(found);
  g_signal_remove_emission_hook(listener->signal, listener->hook);
}
inline GType PlugType() {
  static GType type = g_type_register_static_simple(ATK_TYPE_PLUG, "BastleNativeAxPlug",
      sizeof(AtkPlugClass), [](gpointer value, gpointer) {
        auto* klass = static_cast<AtkObjectClass*>(value);
        klass->get_name = [](AtkObject*) -> const gchar* { return "Содержимое сайта"; };
        klass->get_role = [](AtkObject*) { return ATK_ROLE_PANEL; };
        // AtkPlug's default reads its private child, which this per-view
        // subclass does not use. Return a valid set even between documents;
        // the AT-SPI bridge queries it during children-changed notifications.
        klass->ref_state_set = [](AtkObject* object) {
          auto* states = atk_state_set_new();
          if (!PlugOwner(object)) {
            atk_state_set_add_state(states, ATK_STATE_DEFUNCT);
          } else {
            for (auto state : {ATK_STATE_VISIBLE, ATK_STATE_SHOWING, ATK_STATE_ENABLED, ATK_STATE_SENSITIVE})
              atk_state_set_add_state(states, state);
          }
          return states;
        };
        klass->get_n_children = [](AtkObject* object) { auto* owner = PlugOwner(object); return owner && owner->document ? 1 : 0; };
        klass->ref_child = [](AtkObject* object, gint index) -> AtkObject* {
          auto* owner = PlugOwner(object);
          return index == 0 && owner && owner->document ? ATK_OBJECT(g_object_ref(Wrap(owner->document))) : nullptr;
        };
      }, sizeof(Plug), nullptr, GTypeFlags(0));
  return type;
}
inline void Publish(uint64_t id, AtkObject* root) {
  auto& view = views[id];
  if (root == view.document) return;
  std::cout << nlohmann::json({{"event", "native-accessibility-document"}, {"view", id},
      {"generation", ++view.generation}}).dump() << std::endl;
  if (view.document && initialized)
    g_signal_emit_by_name(view.frame, "children-changed::remove", 0, Wrap(view.document), nullptr);
  auto* previous = view.document;
  view.document = root ? ATK_OBJECT(g_object_ref(root)) : nullptr;
  // Retire only this window's proxies. Other sockets retain valid objects.
  std::vector<AtkObject*> retired;
  for (auto it = proxies.begin(); it != proxies.end();) {
    if (reinterpret_cast<Node*>(it->second)->view == id) {
      auto* proxy = it->second;
      reinterpret_cast<Node*>(proxy)->retired = true;
      retired.push_back(proxy);
      it = proxies.erase(it);
    } else ++it;
  }
  for (auto* proxy : retired) {
    NotifyDefunct(proxy);
    g_object_unref(proxy);
  }
  for (auto it = links.begin(); it != links.end();) {
    auto* link = reinterpret_cast<Link*>(it->second);
    if (link->view == id) {
      link->retired = true;
      g_object_unref(link);
      it = links.erase(it);
    } else ++it;
  }
  if (previous) g_object_unref(previous);
  if (!initialized) {
    auto* app = reinterpret_cast<Node*>(g_object_new(BaseType(), nullptr)); app->kind = 1;
    application = ATK_OBJECT(app);
    atk_bridge_adaptor_cleanup();
    auto* util = ATK_UTIL_CLASS(g_type_class_ref(ATK_TYPE_UTIL));
    util->add_global_event_listener = AddListener;
    util->remove_global_event_listener = RemoveListener;
    util->get_root = [] { return application; };
    util->get_toolkit_name = [] { return "Chromium"; };
    util->get_toolkit_version = [] { return "152 Bastle OSR probe"; };
    g_type_class_unref(util);
    g_unsetenv("NO_AT_BRIDGE");
    atk_bridge_adaptor_init(nullptr, nullptr);
    initialized = true;
  }
  if (!view.frame && root) {
    auto* plug = reinterpret_cast<Plug*>(g_object_new(PlugType(), nullptr));
    plug->view = id;
    view.frame = ATK_OBJECT(plug);
    if (auto* address = atk_plug_get_id(ATK_PLUG(view.frame))) {
      std::cout << nlohmann::json({{"event", "native-accessibility-ready"}, {"view", id}, {"plug", address}}).dump() << std::endl;
      g_free(address);
    }
  }
  if (view.document) g_signal_emit_by_name(view.frame, "children-changed::add", 0, Wrap(view.document), nullptr);
}
inline void CloseView(uint64_t id) {
  auto* view = ViewFor(id);
  if (!view) return;
  Publish(id, nullptr);
  if (view->frame) g_object_unref(view->frame);
  views.erase(id);
}
inline void ForwardSignal(GSignalInvocationHint* hint, guint count, const GValue* values) {
  if (!initialized || count == 0 || count > 8 || !G_VALUE_HOLDS_OBJECT(&values[0])) return;
  auto* native = ATK_OBJECT(g_value_get_object(&values[0]));
  if (!native || g_type_is_a(G_OBJECT_TYPE(native), BaseType()) || ATK_IS_PLUG(native)) return;
  auto* proxy = Wrap(native);
  if (!proxy) return;
  const char* name = g_signal_name(hint->signal_id);
  if (std::string_view(name) == "state-change" && count > 2 && G_VALUE_HOLDS_STRING(&values[1])
      && g_strcmp0(g_value_get_string(&values[1]), "defunct") == 0
      && G_VALUE_HOLDS_BOOLEAN(&values[2]) && g_value_get_boolean(&values[2])) {
    NotifyDefunct(proxy);
    return;
  }
  if (Retired(proxy)) return;
  if (std::string_view(name) == "property-change") {
    if (count != 2 || !G_VALUE_HOLDS_POINTER(&values[1])) return;
    auto* source = static_cast<AtkPropertyValues*>(g_value_get_pointer(&values[1]));
    if (!source) return;
    AtkPropertyValues property{};
    property.property_name = source->property_name;
    auto copy = [](const GValue& from, GValue& to) {
      if (!G_VALUE_TYPE(&from)) return;
      g_value_init(&to, G_VALUE_TYPE(&from)); g_value_copy(&from, &to);
      if (G_VALUE_HOLDS_OBJECT(&from) && ATK_IS_OBJECT(g_value_get_object(&from)))
        g_value_set_object(&to, Wrap(ATK_OBJECT(g_value_get_object(&from))));
    };
    copy(source->old_value, property.old_value); copy(source->new_value, property.new_value);
    g_signal_emit(proxy, hint->signal_id, hint->detail, &property);
    if (G_VALUE_TYPE(&property.old_value)) g_value_unset(&property.old_value);
    if (G_VALUE_TYPE(&property.new_value)) g_value_unset(&property.new_value);
    return;
  }
  GValue mapped[8]{};
  g_value_init(&mapped[0], ATK_TYPE_OBJECT); g_value_set_object(&mapped[0], proxy);
  for (guint i = 1; i < count; ++i) {
    g_value_init(&mapped[i], G_VALUE_TYPE(&values[i]));
    g_value_copy(&values[i], &mapped[i]);
    if (G_VALUE_HOLDS_OBJECT(&values[i]) && ATK_IS_OBJECT(g_value_get_object(&values[i])))
      g_value_set_object(&mapped[i], Wrap(ATK_OBJECT(g_value_get_object(&values[i]))));
    if (((std::string_view(name) == "children-changed" && i == 2) ||
         (std::string_view(name) == "active-descendant-changed" && i == 1)) && G_VALUE_HOLDS_POINTER(&values[i]))
      g_value_set_pointer(&mapped[i], Wrap(ATK_OBJECT(g_value_get_pointer(&values[i]))));
  }
  g_signal_emitv(mapped, hint->signal_id, hint->detail, nullptr);
  for (guint i = 0; i < count; ++i) g_value_unset(&mapped[i]);
}
inline void Shutdown() {
  while (!views.empty()) CloseView(views.begin()->first);
  if (initialized) atk_bridge_adaptor_cleanup();
  initialized = false;
  if (application) g_object_unref(application);
  application = nullptr;
}
}  // namespace atspi_export
