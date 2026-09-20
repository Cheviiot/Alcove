// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <functional>
#include <map>
#include "include/cef_dialog_handler.h"
#include "include/cef_download_handler.h"
#include "include/cef_jsdialog_handler.h"
#include "include/cef_permission_handler.h"
#include "include/cef_callback.h"

// Browser UI callbacks stay on CEF's UI thread. Only a request identifier and
// display data cross the private parent pipe; the renderer cannot answer them.
class SiteRequests final : public CefDialogHandler,
                           public CefDownloadHandler,
                           public CefJSDialogHandler,
                           public CefPermissionHandler {
 public:
  using Json = nlohmann::json;
  using Send = std::function<void(Json)>;
  explicit SiteRequests(Send send) : send_(std::move(send)) {}

  // Called on the UI thread by the resource handler. The network request is
  // paused at its original CEF callback, preserving POST data and redirects.
  void Navigation(uint64_t request, const std::string& url, CefRefPtr<CefCallback> callback) {
    // Do not queue navigation behind UI belonging to the departing document:
    // it cannot reach OnLoadStart (which normally cancels that UI) while this
    // resource callback is paused. before-unload remains CEF's own gate.
    CancelKinds({"permission", "file-dialog", "js-dialog"});
    const auto id = Add("navigation", {{"url", url}}, [callback](const Json& answer) {
      if (answer.value("allow", false)) callback->Continue(); else callback->Cancel();
    });
    if (id) navigations_[request] = id;
  }
  void NavigationComplete(uint64_t request) {
    auto entry = navigations_.extract(request);
    if (entry.empty()) return;
    auto pending = pending_.extract(entry.mapped());
    if (!pending.empty()) {
      pending.mapped().reply(Json::object());
      send_({{"event", "request-cancelled"}, {"id", entry.mapped()}});
    }
  }

  bool OnJSDialog(CefRefPtr<CefBrowser>, const CefString& origin,
      JSDialogType type, const CefString& message, const CefString& initial,
      CefRefPtr<CefJSDialogCallback> callback, bool& suppress) override {
    if (pending_.size() >= 32) { suppress = true; return false; }
    Add("js-dialog", {{"origin", origin.ToString()}, {"type", type},
        {"message", message.ToString()}, {"initial", initial.ToString()}},
        [callback](const Json& answer) {
          callback->Continue(answer.value("allow", false), answer.value("text", ""));
        });
    return true;
  }
  bool OnBeforeUnloadDialog(CefRefPtr<CefBrowser> browser, const CefString&,
      bool reload, CefRefPtr<CefJSDialogCallback> callback) override {
    Add("before-unload", {{"origin", browser->GetMainFrame()->GetURL().ToString()},
        {"reload", reload}}, [callback](const Json& answer) {
          callback->Continue(answer.value("allow", false), CefString());
        });
    return true;
  }
  void OnResetDialogState(CefRefPtr<CefBrowser>) override {
    CancelKinds({"js-dialog", "before-unload"});
  }
  bool OnFileDialog(CefRefPtr<CefBrowser> browser, FileDialogMode mode,
      const CefString& title, const CefString& default_path,
      const std::vector<CefString>& filters, const std::vector<CefString>&,
      const std::vector<CefString>&, CefRefPtr<CefFileDialogCallback> callback) override {
    Json accept = Json::array();
    for (const auto& filter : filters) accept.push_back(filter.ToString());
    Add("file-dialog", {{"origin", browser->GetMainFrame()->GetURL().ToString()},
        {"mode", mode}, {"title", title.ToString()},
        {"name", std::filesystem::path(default_path.ToString()).filename().string()},
        {"filters", accept}}, [callback, mode](const Json& answer) {
          std::vector<CefString> paths;
          if (answer.value("allow", false) && answer.contains("paths") && answer["paths"].is_array()) {
            for (const auto& path : answer["paths"]) {
              if (!path.is_string()) { paths.clear(); break; }
              const auto value = path.get<std::string>();
              if (value.empty() || value[0] != '/' || value.find('\0') != std::string::npos) {
                paths.clear(); break;
              }
              paths.emplace_back(value);
            }
          }
          if (paths.empty() || (mode != FILE_DIALOG_OPEN_MULTIPLE && paths.size() != 1)) callback->Cancel();
          else callback->Continue(paths);
        });
    return true;
  }
  bool OnRequestMediaAccessPermission(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame>,
      const CefString& origin, uint32_t permissions, CefRefPtr<CefMediaAccessCallback> callback) override {
    Add("permission", {{"origin", origin.ToString()}, {"media", true}, {"permissions", permissions}},
        [callback, permissions](const Json& answer) {
          // Desktop capture needs a portal source picker, which is a separate
          // capability. Never grant it with a generic permission confirmation.
          const uint32_t supported = CEF_MEDIA_PERMISSION_DEVICE_AUDIO_CAPTURE | CEF_MEDIA_PERMISSION_DEVICE_VIDEO_CAPTURE;
          callback->Continue(answer.value("allow", false) && !(permissions & ~supported) ? permissions : 0);
        });
    return true;
  }
  bool OnShowPermissionPrompt(CefRefPtr<CefBrowser>, uint64_t prompt,
      const CefString& origin, uint32_t permissions, CefRefPtr<CefPermissionPromptCallback> callback) override {
    // Every download still requires Alcove's explicit destination portal.
    // Match WebKit: that per-file confirmation also authorizes multiple files.
    if (permissions == CEF_PERMISSION_TYPE_MULTIPLE_DOWNLOADS) {
      callback->Continue(CEF_PERMISSION_RESULT_ACCEPT);
      return true;
    }
    const auto id = Add("permission", {{"origin", origin.ToString()}, {"media", false}, {"permissions", permissions}},
        [callback](const Json& answer) {
          callback->Continue(answer.value("allow", false) ? CEF_PERMISSION_RESULT_ACCEPT :
              answer.value("dismiss", false) ? CEF_PERMISSION_RESULT_DISMISS : CEF_PERMISSION_RESULT_DENY);
        });
    if (id) prompts_[prompt] = id;
    return true;
  }
  void OnDismissPermissionPrompt(CefRefPtr<CefBrowser>, uint64_t prompt,
      cef_permission_request_result_t) override {
    auto it = prompts_.find(prompt);
    if (it == prompts_.end()) return;
    const auto id = it->second;
    prompts_.erase(it);
    // Chromium has already dismissed this callback. Do not continue it again.
    if (pending_.erase(id)) send_({{"event", "request-cancelled"}, {"id", id}});
  }
  bool OnBeforeDownload(CefRefPtr<CefBrowser>, CefRefPtr<CefDownloadItem> item,
      const CefString& name, CefRefPtr<CefBeforeDownloadCallback> callback) override {
    const auto download = item->GetId();
    download_names_[download] = name.ToString();
    Add("download-request", {{"download", download}, {"url", item->GetURL().ToString()},
        {"name", std::filesystem::path(name.ToString()).filename().string()}},
        [this, callback, download](const Json& answer) {
          const auto path = answer.contains("path") && answer["path"].is_string() ? answer["path"].get<std::string>() : std::string();
          if (answer.value("allow", false) && !path.empty() && path[0] == '/' && path.find('\0') == std::string::npos)
            callback->Continue(path, false);
          else if (auto it = downloads_.find(download); it != downloads_.end()) {
            auto update = it->second;
            update->Cancel();
          }
          // Releasing an uncontinued BeforeDownloadCallback also cancels it.
        });
    return true;
  }
  void OnDownloadUpdated(CefRefPtr<CefBrowser>, CefRefPtr<CefDownloadItem> item,
      CefRefPtr<CefDownloadItemCallback> callback) override {
    if (!item->IsValid()) return;
    const auto id = item->GetId();
    if (item->IsInProgress()) downloads_[id] = callback;
    else downloads_.erase(id);
    auto name = item->GetSuggestedFileName().ToString();
    if (name.empty() && download_names_.contains(id)) name = download_names_[id];
    send_({{"event", "download"}, {"download", id}, {"name", name},
        {"received", item->GetReceivedBytes()}, {"total", item->GetTotalBytes()},
        {"complete", item->IsComplete()}, {"cancelled", item->IsCanceled()},
        {"progress", item->IsInProgress()}, {"paused", item->IsPaused()},
        {"path", item->GetFullPath().ToString()}});
    if (!item->IsInProgress()) download_names_.erase(id);
  }
  void DownloadCommand(uint32_t id, const std::string& action) {
    const auto it = downloads_.find(id);
    if (it == downloads_.end()) return;
    auto callback = it->second;
    if (action == "cancel") callback->Cancel();
    else if (action == "pause") callback->Pause();
    else if (action == "resume") callback->Resume();
  }
  void Reply(const Json& answer) {
    const auto id = answer.at("id").get<uint64_t>();
    auto entry = pending_.extract(id);
    if (entry.empty()) return;  // Stale/repeated responses cannot affect another request.
    entry.mapped().reply(answer);
    send_({{"event", "request-resolved"}, {"id", id}});
  }
  void Navigated() { CancelKinds({"permission", "file-dialog", "js-dialog", "before-unload"}); }
  void Close() {
    closed_ = true;
    CancelKinds({"permission", "file-dialog", "js-dialog", "before-unload", "download-request", "navigation"});
    navigations_.clear();
    auto downloads = std::move(downloads_);
    downloads_.clear();
    for (auto& [id, callback] : downloads) callback->Cancel();
  }
 private:
  struct Pending { std::string kind; std::function<void(const Json&)> reply; };
  uint64_t Add(const std::string& kind, Json data, std::function<void(const Json&)> reply) {
    if (closed_ || pending_.size() >= 32) { reply(Json::object()); return 0; }
    const auto id = ++serial_;
    pending_.emplace(id, Pending{kind, std::move(reply)});
    data["event"] = "site-request";
    data["kind"] = kind;
    data["id"] = id;
    send_(std::move(data));
    return id;
  }
  void CancelKinds(std::initializer_list<std::string> kinds) {
    std::vector<uint64_t> ids;
    for (const auto& [id, pending] : pending_)
      if (std::find(kinds.begin(), kinds.end(), pending.kind) != kinds.end()) ids.push_back(id);
    for (auto id : ids) {
      auto entry = pending_.extract(id);
      if (!entry.empty()) {
        // Navigating away is a dismissal, never a saved Chrome denial.
        const auto answer = entry.mapped().kind == "permission" ? Json{{"dismiss", true}} : Json::object();
        entry.mapped().reply(answer);
      }
      send_({{"event", "request-cancelled"}, {"id", id}});
    }
  }
  Send send_;
  bool closed_ = false;
  uint64_t serial_ = 0;
  std::map<uint64_t, Pending> pending_;
  std::map<uint64_t, uint64_t> prompts_;
  std::map<uint64_t, uint64_t> navigations_;
  std::map<uint32_t, CefRefPtr<CefDownloadItemCallback>> downloads_;
  std::map<uint32_t, std::string> download_names_;
  IMPLEMENT_REFCOUNTING(SiteRequests);
};
