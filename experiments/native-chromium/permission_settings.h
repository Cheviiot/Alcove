// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <map>
#include <string>
#include <nlohmann/json.hpp>
#include "include/cef_request_context.h"

// CEF 152 maps ACCEPT/DENY to Chrome's persistent permission decisions, and
// does not expose AcceptThisTime. Alcove owns the durable policy. Reset only
// the corresponding Chrome permission exceptions before the first page loads;
// cookies, site storage and unrelated preferences remain intact.
// Names are pinned to Chromium 152 website_settings_registry/content_settings.
inline bool ApplyPermissionSettings(CefRefPtr<CefRequestContext> context,
    const nlohmann::json& policy, std::string& failure) {
  for (const auto* kind : {"notifications", "geolocation", "geolocation_with_options",
        "media_stream_mic", "media_stream_camera", "clipboard", "pointer_lock",
        "storage_access", "top_level_storage_access", "permission_autoblocking_data"}) {
    const std::string name = std::string("profile.content_settings.exceptions.") + kind;
    CefString error;
    if (!context->HasPreference(name) || !context->CanSetPreference(name) ||
        !context->SetPreference(name, nullptr, error)) {
      failure = "Could not synchronize Alcove permissions: " + name + ": " + error.ToString();
      return false;
    }
  }
  const std::map<std::string, cef_content_setting_types_t> types = {
    {"notifications", CEF_CONTENT_SETTING_TYPE_NOTIFICATIONS},
    {"geolocation", CEF_CONTENT_SETTING_TYPE_GEOLOCATION},
    {"microphone", CEF_CONTENT_SETTING_TYPE_MEDIASTREAM_MIC},
    {"camera", CEF_CONTENT_SETTING_TYPE_MEDIASTREAM_CAMERA},
    {"clipboard", CEF_CONTENT_SETTING_TYPE_CLIPBOARD_READ_WRITE},
    {"pointer_lock", CEF_CONTENT_SETTING_TYPE_POINTER_LOCK},
    {"third_party_storage", CEF_CONTENT_SETTING_TYPE_STORAGE_ACCESS},
  };
  const auto permissions = policy.value("permissions", nlohmann::json::object());
  for (const auto& [origin, decisions] : permissions.items()) {
    for (const auto& [kind, decision] : decisions.items()) {
      const auto found = types.find(kind);
      if (found == types.end() || !decision.is_string()) { failure = "Unknown Alcove permission"; return false; }
      const auto value = decision == "allow" ? CEF_CONTENT_SETTING_VALUE_ALLOW :
          decision == "block" ? CEF_CONTENT_SETTING_VALUE_BLOCK : CEF_CONTENT_SETTING_VALUE_ASK;
      context->SetContentSetting(origin, "", found->second, value);
      if (kind == "third_party_storage")
        context->SetContentSetting(origin, "", CEF_CONTENT_SETTING_TYPE_TOP_LEVEL_STORAGE_ACCESS, value);
    }
  }
  return true;
}
