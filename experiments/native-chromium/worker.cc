// SPDX-License-Identifier: GPL-3.0-only
// Optional OSR worker. Alcove supplies a separate CEF profile and private IPC;
// Chromium's renderer sandbox remains enabled.
#include <atomic>
#include <chrono>
#include <csignal>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <poll.h>
#include <sys/prctl.h>
#include <unistd.h>
#include <nlohmann/json.hpp>
#include "include/cef_app.h"
#include "include/cef_browser.h"
#include "include/cef_client.h"
#include "include/cef_parser.h"
#include "include/cef_request_context.h"
#include "include/cef_version.h"
#include "include/cef_version_info.h"
#include "gpu_bridge.h"
#include "native_accessibility.h"
#include "site_requests.h"
#include "navigation_gate.h"
#include "permission_settings.h"

using Json = nlohmann::json;
namespace fs = std::filesystem;
constexpr int kProtocol = 4;
constexpr int kMaximumDimension = 4096;

void Emit(Json value) {
  std::cout << value.dump() << std::endl;
}

class Client final : public CefClient,
                     public CefRenderHandler,
                     public CefLifeSpanHandler,
                     public CefLoadHandler,
                     public CefDisplayHandler,
                     public CefRequestHandler,
                     public CefAccessibilityHandler {
 public:
  static inline Json initial_policy = Json::object();
  static inline bool diagnostics = false;
  explicit Client(fs::path directory, std::shared_ptr<GpuBridge> bridge, bool native_accessibility,
                  uint64_t view = 1, uint64_t opener = 0)
      : directory_(std::move(directory)), view_(view), opener_(opener),
        gpu_(bool(bridge)), native_accessibility_(native_accessibility), gpu_bridge_(std::move(bridge)) {
    fs::create_directories(FrameDirectory());
    // CEF may retain a request handler beyond its client's close callback.
    // Capture the immutable route, never a raw Client pointer.
    requests_ = new SiteRequests([view](Json event) { event["view"] = view; Emit(std::move(event)); });
  }
  void Send(Json event) const { event["view"] = view_; Emit(std::move(event)); }
  fs::path FrameDirectory() const { return view_ == 1 ? directory_ : directory_ / "views" / std::to_string(view_); }
  static void Dispatch(const Json& request) {
    if (request.value("protocol", 0) != kProtocol) throw std::runtime_error("unsupported protocol");
    if (request.value("command", "") == "quit") {
      shutting_down_ = true;
      for (const auto& [id, client] : clients_) client->Command(request);
      return;
    }
    const auto view = request.at("view").get<uint64_t>();
    if (auto found = clients_.find(view); found != clients_.end()) found->second->Command(request);
    // Late commands from a closing GTK window cannot target any other view.
  }
  static void Add(CefRefPtr<Client> client) { clients_.emplace(client->view_, client); }
  static bool Empty() { return clients_.empty(); }
  static void Reap() { std::erase_if(clients_, [](const auto& item) { return item.second->closed_; }); }

  CefRefPtr<CefRenderHandler> GetRenderHandler() override { return this; }
  CefRefPtr<CefLifeSpanHandler> GetLifeSpanHandler() override { return this; }
  CefRefPtr<CefLoadHandler> GetLoadHandler() override { return this; }
  CefRefPtr<CefDisplayHandler> GetDisplayHandler() override { return this; }
  CefRefPtr<CefRequestHandler> GetRequestHandler() override { return this; }
  CefRefPtr<CefAccessibilityHandler> GetAccessibilityHandler() override { return this; }
  CefRefPtr<CefDialogHandler> GetDialogHandler() override { return requests_; }
  CefRefPtr<CefDownloadHandler> GetDownloadHandler() override { return requests_; }
  CefRefPtr<CefJSDialogHandler> GetJSDialogHandler() override { return requests_; }
  CefRefPtr<CefPermissionHandler> GetPermissionHandler() override { return requests_; }
  CefRefPtr<CefResourceRequestHandler> GetResourceRequestHandler(
      CefRefPtr<CefBrowser>, CefRefPtr<CefFrame> frame, CefRefPtr<CefRequest>,
      bool is_navigation, bool, const CefString&, bool&) override {
    if (is_navigation && frame && frame->IsMain()) return new NavigationGate(requests_);
    return nullptr;
  }
  void GetViewRect(CefRefPtr<CefBrowser>, CefRect& rect) override {
    rect = CefRect(0, 0, width_, height_);
  }
  bool GetScreenInfo(CefRefPtr<CefBrowser>, CefScreenInfo& info) override {
    info.device_scale_factor = scale_;
    info.rect = info.available_rect = CefRect(0, 0, width_, height_);
    Send({{"event", "screen-info"}, {"scale", scale_}, {"width", width_}, {"height", height_}});
    return true;
  }
  void OnAfterCreated(CefRefPtr<CefBrowser> browser) override {
    if (view_ == 1) {
      std::string failure;
      if (!ApplyPermissionSettings(browser->GetHost()->GetRequestContext(), initial_policy, failure)) {
        Send({{"event", "protocol-error"}, {"message", failure}});
        browser->GetHost()->CloseBrowser(true);
        return;
      }
    }
    browser_ = browser;
    if (opener_) {
      if (auto found = clients_.find(opener_); found != clients_.end())
        std::erase_if(found->second->pending_popups_, [this](const auto& item) { return item.second == view_; });
      Send({{"event", "view-created"}, {"opener", opener_}, {"width", width_}, {"height", height_}});
    }
    if (native_accessibility_) NativeAccessibility::Register(view_);
    if (native_accessibility_) atspi_export::views[view_].request_host_focus = [this](std::function<void(bool)> action) {
      if (!browser_ || accessibility_actions_.size() >= 32) return false;
      const auto id = ++accessibility_action_id_;
      accessibility_actions_.emplace(id, std::move(action));
      Send({{"event", "native-accessibility-focus-request"}, {"id", id}});
      return true;
    };
    browser->GetHost()->SetAccessibilityState(native_accessibility_ ? STATE_DEFAULT : STATE_ENABLED);
    Send({{"event", "ready"}, {"protocol", kProtocol}, {"cef", CEF_VERSION},
          {"transport", gpu_ ? "gpu-dmabuf" : "cpu-diagnostic"},
          {"pid", getpid()}, {"native_atspi", native_accessibility_ ? 3 : 0}, {"site_requests", 1}, {"multi_view", 1}, {"surfaces", 1}, {"shell_actions", 1}, {"app_launch", 1}, {"policy_ui", 1}, {"window_lifecycle", 1}});
    if (close_requested_ || shutting_down_) browser->GetHost()->CloseBrowser(true);
  }
  void OnBeforeClose(CefRefPtr<CefBrowser>) override {
    if (native_accessibility_) NativeAccessibility::Close(view_);
    accessibility_actions_.clear();
    requests_->Close();
    browser_ = nullptr;
    closed_ = true;
    Send({{"event", "view-closed"}});
    for (const auto& [popup_id, view] : pending_popups_) {
      auto found = clients_.find(view);
      if (found != clients_.end() && !found->second->browser_) found->second->closed_ = true;
    }
    pending_popups_.clear();
  }
  bool OnBeforePopup(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame>, int popup_id,
      const CefString& url, const CefString&, cef_window_open_disposition_t, bool user_gesture,
      const CefPopupFeatures& features, CefWindowInfo& window, CefRefPtr<CefClient>& client,
      CefBrowserSettings&, CefRefPtr<CefDictionaryValue>&, bool*) override {
    if (shutting_down_ || clients_.size() >= 9) {
      Send({{"event", "popup-blocked"}, {"url", url.ToString()},
            {"reason", "view-limit-or-shutdown"}});
      return true;
    }
    const auto view = ++next_view_;
    CefRefPtr<Client> popup = new Client(directory_, gpu_bridge_, native_accessibility_, view, view_);
    popup->width_ = features.widthSet ? std::clamp(features.width, 320, 1600) : 640;
    popup->height_ = features.heightSet ? std::clamp(features.height, 240, 1200) : 640;
    popup->scale_ = scale_;
    client = popup;
    window.SetAsWindowless(0);
    window.shared_texture_enabled = gpu_;
    Add(popup);
    pending_popups_[popup_id] = view;
    // CEF creates the actual popup, preserving opener, target name and request
    // context. Never replace window.open with a new independent URL load.
    return false;
  }
  void OnBeforePopupAborted(CefRefPtr<CefBrowser>, int popup_id) override {
    auto entry = pending_popups_.extract(popup_id);
    if (entry.empty()) return;
    if (auto found = clients_.find(entry.mapped()); found != clients_.end()) {
      found->second->closed_ = true;
      found->second->Send({{"event", "popup-aborted"}});
    }
  }
  void OnPaint(CefRefPtr<CefBrowser>, PaintElementType type,
      const RectList&, const void* buffer, int width, int height) override {
    if ((type == PET_POPUP && !popup_visible_) || width < 1 || height < 1 ||
        width > kMaximumDimension || height > kMaximumDimension) return;
    const bool popup = type == PET_POPUP;
    const auto stem = popup ? "popup-" + std::to_string(popup_generation_) : "frame";
    const size_t bytes = static_cast<size_t>(width) * height * 4;
    // Atomic replacement prevents torn frames. This deliberately measures a
    // diagnostic CPU path; it is not the proposed production GPU transport.
    const auto next = FrameDirectory() / (stem + ".next");
    std::ofstream output(next, std::ios::binary | std::ios::trunc);
    const uint32_t header[] = {0x42535446, kProtocol,
        static_cast<uint32_t>(width), static_cast<uint32_t>(height)};
    output.write(reinterpret_cast<const char*>(header), sizeof(header));
    output.write(static_cast<const char*>(buffer), bytes);
    output.close();
    if (!output) return;
    fs::rename(next, FrameDirectory() / (stem + ".bin"));
    Send({{"event", "frame"}, {"sequence", ++frames_},
          {"surface", popup ? "popup" : "view"}, {"generation", popup ? popup_generation_ : 0},
          {"width", width}, {"height", height}, {"bytes", bytes}});
  }
  void OnPopupShow(CefRefPtr<CefBrowser>, bool show) override {
    popup_visible_ = show;
    PopupChanged();
  }
  void OnPopupSize(CefRefPtr<CefBrowser>, const CefRect& rect) override {
    popup_rect_ = rect;
    PopupChanged();
  }
  void PopupChanged() {
    // Per-generation files cannot be mistaken for a reopened popup's pixels.
    std::error_code ignored;
    fs::remove(FrameDirectory() / ("popup-" + std::to_string(popup_generation_) + ".bin"), ignored);
    ++popup_generation_;
    Send({{"event", "popup-surface"}, {"generation", popup_generation_}, {"visible", popup_visible_},
          {"rect", {popup_rect_.x, popup_rect_.y, popup_rect_.width, popup_rect_.height}}});
  }
  void OnAcceleratedPaint(CefRefPtr<CefBrowser>, PaintElementType type,
      const RectList&, const CefAcceleratedPaintInfo& info) override {
    if (type == PET_POPUP && !popup_visible_) return;
    const auto surface = type == PET_POPUP ? "popup" : "view";
    const auto generation = type == PET_POPUP ? popup_generation_ : 0;
    const auto sequence = ++frames_;
    Send({{"event", "gpu-frame"}, {"planes", info.plane_count}, {"sequence", sequence}, {"surface", surface}, {"generation", generation}});
    if (gpu_error_) return;
    try {
      if (gpu_bridge_->Copy(info, sequence, view_, surface, generation))
        Send({{"event", "gpu-export"}, {"sequence", sequence}, {"surface", surface}, {"generation", generation}});
      else Send({{"event", "gpu-dropped"}, {"sequence", sequence}, {"surface", surface}, {"generation", generation}});
    } catch (const std::exception& error) {
      gpu_error_ = true;
      Send({{"event", "gpu-error"}, {"message", error.what()}});
    }
  }
  void OnAccessibilityTreeChange(CefRefPtr<CefValue> value) override {
    const auto serialized = CefWriteJSON(value, JSON_WRITER_DEFAULT).ToString();
    auto tree = Json::parse(serialized);
    NativeAccessibility::TreeChanged(view_, tree);
    ++accessibility_updates_;
    // Native accessibility travels over AT-SPI. Dumping an entire real page's
    // redundant tree can exceed the bounded control pipe and stop all events.
    Json event{{"event", "accessibility"}, {"updates", accessibility_updates_},
               {"tree_id", tree.value("ax_tree_id", "")},
               {"serialized_bytes", serialized.size()}};
    if (diagnostics && !native_accessibility_ && serialized.size() < 3 * 1024 * 1024)
      event["tree"] = std::move(tree);
    else event["tree_omitted"] = true;
    Send(std::move(event));
  }
  void OnAccessibilityLocationChange(CefRefPtr<CefValue>) override {}
  void OnLoadingStateChange(CefRefPtr<CefBrowser>, bool loading, bool back, bool forward) override {
    Send({{"event", "navigation"}, {"loading", loading}, {"back", back}, {"forward", forward}});
  }
  void OnLoadStart(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame> frame, TransitionType) override {
    if (frame->IsMain()) {
      if (popup_visible_) { popup_visible_ = false; PopupChanged(); }
      requests_->Navigated();
      NativeAccessibility::Navigated(view_);
    }
  }
  void OnLoadingProgressChange(CefRefPtr<CefBrowser>, double progress) override {
    Send({{"event", "load-progress"}, {"progress", progress}});
  }
  void OnTitleChange(CefRefPtr<CefBrowser>, const CefString& title) override {
    Send({{"event", "title"}, {"title", title.ToString()}});
  }
  void OnLoadError(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame> frame, ErrorCode code,
      const CefString& text, const CefString& url) override {
    if (frame->IsMain() && code != ERR_ABORTED)
      Send({{"event", "load-error"}, {"code", code}, {"message", text.ToString()}, {"url", url.ToString()}});
  }
  bool OnConsoleMessage(CefRefPtr<CefBrowser>, cef_log_severity_t,
      const CefString& message, const CefString&, int) override {
    if (diagnostics) Send({{"event", "console"}, {"message", message.ToString()}});
    return true;
  }
  void OnRenderProcessTerminated(CefRefPtr<CefBrowser>, TerminationStatus status,
      int code, const CefString& text) override {
    requests_->Navigated();
    Send({{"event", "renderer-crashed"}, {"status", status}, {"code", code}, {"message", text.ToString()}});
  }
  void Command(const Json& request) {
    if (!request.is_object() || request.value("protocol", 0) != kProtocol)
      throw std::runtime_error("unsupported protocol");
    const auto name = request.at("command").get<std::string>();
    if (name == "quit") close_requested_ = true;
    if (!browser_) return;
    auto host = browser_->GetHost();
    if (name == "native-accessibility-focus-response") {
      auto action = accessibility_actions_.extract(request.at("id").get<uint64_t>());
      if (!action.empty()) {
        const bool focused = request.value("focused", false);
        if (focused) { host->SetFocus(true); NativeAccessibility::SetHostFocus(view_, true); }
        action.mapped()(focused);
        Send({{"event", "native-accessibility-focus-result"}, {"id", request.at("id")}, {"focused", focused}});
      }
    } else if (name == "request-response") { requests_->Reply(request);
    } else if (name == "download-control") { requests_->DownloadCommand(request.at("download"), request.at("action"));
    } else if (name == "resize") {
      scale_ = std::clamp(request.value("scale", 1.0), 1.0, 4.0);
      const int maximum = static_cast<int>(kMaximumDimension / scale_);
      width_ = std::clamp(request.at("width").get<int>(), 1, maximum);
      height_ = std::clamp(request.at("height").get<int>(), 1, maximum);
      Send({{"event", "resize-applied"}, {"scale", scale_}, {"width", width_}, {"height", height_}});
      host->NotifyScreenInfoChanged();
      host->WasResized();
    } else if (name == "back") { if (browser_->CanGoBack()) browser_->GoBack();
    } else if (name == "forward") { if (browser_->CanGoForward()) browser_->GoForward();
    } else if (name == "load") {
      const auto url = request.at("url").get<std::string>();
      if (url.starts_with("https://") || url.starts_with("http://"))
        browser_->GetMainFrame()->LoadURL(url);
    } else if (name == "reload") { browser_->Reload();
    } else if (name == "reload-bypass-cache") { browser_->ReloadIgnoreCache();
    } else if (name == "stop") { browser_->StopLoad();
    } else if (name == "focus") {
      const bool focused = request.at("focused").get<bool>();
      host->SetFocus(focused);
      NativeAccessibility::SetHostFocus(view_, focused);
    } else if (name == "mouse" || name == "click" || name == "scroll") {
      CefMouseEvent event;
      event.x = request.at("x").get<int>();
      event.y = request.at("y").get<int>();
      event.modifiers = request.value("modifiers", 0u);
      if (name == "mouse") host->SendMouseMoveEvent(event, request.value("leave", false));
      else if (name == "scroll") host->SendMouseWheelEvent(event, request.at("dx").get<int>(), request.at("dy").get<int>());
      else {
        const int button = request.value("button", 1);
        host->SendMouseClickEvent(event, button == 3 ? MBT_RIGHT : button == 2 ? MBT_MIDDLE : MBT_LEFT,
                                 request.at("up").get<bool>(), request.value("count", 1));
      }
    } else if (name == "key") {
      CefKeyEvent event;
      event.type = request.value("up", false) ? KEYEVENT_KEYUP : KEYEVENT_RAWKEYDOWN;
      event.windows_key_code = request.at("key").get<int>();
      event.native_key_code = request.value("native", 0);
      event.modifiers = request.value("modifiers", 0u);
      host->SendKeyEvent(event);
      // GTK sends a key event for Return, not an IM commit. Chromium needs
      // its character event as well to activate buttons / submit forms.
      if (event.type == KEYEVENT_RAWKEYDOWN && event.windows_key_code == 13 &&
          !(event.modifiers & (EVENTFLAG_CONTROL_DOWN | EVENTFLAG_ALT_DOWN | EVENTFLAG_COMMAND_DOWN))) {
        event.type = KEYEVENT_CHAR;
        event.character = event.unmodified_character = '\r';
        host->SendKeyEvent(event);
      }
    } else if (name == "text") {
      host->ImeCommitText(request.at("text").get<std::string>(), CefRange(UINT32_MAX, UINT32_MAX), 0);
    } else if (name == "composition") {
      const auto text = request.at("text").get<std::string>();
      CefString cef_text(text);
      std::vector<CefCompositionUnderline> underlines;
      host->ImeSetComposition(cef_text, underlines, CefRange(UINT32_MAX, UINT32_MAX),
                              CefRange(cef_text.length(), cef_text.length()));
    } else if (name == "cancel-composition") { host->ImeCancelComposition();
    } else if (name == "zoom") {
      const auto level = request.at("level").get<double>();
      host->SetZoomLevel(level);
      // GTK's viewport does not resize when zoom changes. Refresh OSR visual
      // properties explicitly, including documents restored through history.
      // Without this, CEF 152 can report the new zoom but keep the old layout.
      host->NotifyScreenInfoChanged();
      host->WasResized();
      Send({{"event", "zoom-changed"}, {"requested", level}, {"level", host->GetZoomLevel()}});
    } else if (name == "visibility") {
      host->WasHidden(request.at("hidden").get<bool>());
    } else if (name == "theme") {
      const bool dark = request.at("dark").get<bool>();
      host->GetRequestContext()->SetChromeColorScheme(
          dark ? CEF_COLOR_VARIANT_DARK : CEF_COLOR_VARIANT_LIGHT, 0);
      // Alloy OSR does not consistently inherit Chrome's theme for CSS media
      // queries. Set only prefers-color-scheme via the in-process CDP API;
      // there is no remote debugging port or page-accessible control channel.
      auto feature = CefDictionaryValue::Create();
      feature->SetString("name", "prefers-color-scheme");
      feature->SetString("value", dark ? "dark" : "light");
      auto features = CefListValue::Create();
      features->SetDictionary(0, feature);
      auto parameters = CefDictionaryValue::Create();
      parameters->SetList("features", features);
      host->ExecuteDevToolsMethod(0, "Emulation.setEmulatedMedia", parameters);
    } else if (name == "native-accessibility-inspect" && diagnostics) { NativeAccessibility::Inspect(view_);
    } else if (name == "evaluate" && diagnostics) {
      // Available only over the inherited parent pipe in this experimental tool.
      browser_->GetMainFrame()->ExecuteJavaScript(request.at("script").get<std::string>(), "alcove-probe", 1);
    } else if (name == "close-view") { host->CloseBrowser(false);
    } else if (name == "quit") { host->CloseBrowser(true);
    } else throw std::runtime_error("unknown command");
  }
  bool closed() const { return closed_; }
 private:
  fs::path directory_;
  CefRefPtr<CefBrowser> browser_;
  CefRefPtr<SiteRequests> requests_;
  uint64_t view_, opener_;
  std::map<int, uint64_t> pending_popups_;
  inline static std::map<uint64_t, CefRefPtr<Client>> clients_;
  inline static uint64_t next_view_ = 1;
  inline static bool shutting_down_ = false;
  bool gpu_;
  bool native_accessibility_;
  bool gpu_error_ = false;
  std::shared_ptr<GpuBridge> gpu_bridge_;
  bool closed_ = false;
  bool close_requested_ = false;
  double scale_ = 1;
  int width_ = 800, height_ = 600;
  uint64_t frames_ = 0, accessibility_updates_ = 0;
  uint64_t popup_generation_ = 0;
  bool popup_visible_ = false;
  CefRect popup_rect_;
  uint64_t accessibility_action_id_ = 0;
  std::map<uint64_t, std::function<void(bool)>> accessibility_actions_;
  IMPLEMENT_REFCOUNTING(Client);
};

class ProbeApp final : public CefApp, public CefBrowserProcessHandler {
 public:
  CefRefPtr<CefBrowserProcessHandler> GetBrowserProcessHandler() override { return this; }
  void OnContextInitialized() override { Emit({{"event", "stage"}, {"name", "context-initialized"}}); }
 private:
  IMPLEMENT_REFCOUNTING(ProbeApp);
};

int main(int argc, char** argv) {
  prctl(PR_SET_PDEATHSIG, SIGTERM);
  // Re-enable CLOEXEC before CEF can launch any of its own subprocesses.
  for (int i = 1; i < argc; ++i) {
    const std::string_view value(argv[i]);
    if (value.starts_with("--gpu-socket=")) {
      const int fd = std::stoi(std::string(value.substr(13)));
      if (fd >= 0) fcntl(fd, F_SETFD, FD_CLOEXEC);
    }
  }
  CefMainArgs args(argc, argv);
  CefRefPtr<ProbeApp> app = new ProbeApp;
  const int child = CefExecuteProcess(args, app, nullptr);
  if (child >= 0) return child;
  auto command = CefCommandLine::CreateCommandLine();
  command->InitFromArgv(argc, argv);
  if (!command->HasSwitch("probe-directory") || !command->HasSwitch("cef-root")) return 2;
  const fs::path directory(command->GetSwitchValue("probe-directory").ToString());
  const fs::path cef_root(command->GetSwitchValue("cef-root").ToString());
  if (command->HasSwitch("probe-native-accessibility")) {
    constexpr std::string_view validated_native_cef = "152.0.7+g83ffcba+chromium-152.0.7977.83";
    if (std::string_view(cef_version_full()) != validated_native_cef) {
      Emit({{"event", "protocol-error"}, {"message", "native accessibility requires validated CEF 152.0.7"}});
      return 5;
    }
    NativeAccessibility::Initialize();
  }
  Client::diagnostics = command->HasSwitch("alcove-diagnostics");
  try {
    const fs::path policy_path(command->GetSwitchValue("alcove-policy").ToString());
    if (!policy_path.is_absolute() || fs::file_size(policy_path) > 32 * 1024 * 1024)
      throw std::runtime_error("invalid Alcove policy path or size");
    std::ifstream policy_file(policy_path);
    policy_file >> Client::initial_policy;
    if (Client::initial_policy.value("schema_version", 0) != 2)
      throw std::runtime_error("unsupported Alcove policy schema");
  } catch (const std::exception& error) {
    Emit({{"event", "protocol-error"}, {"message", error.what()}});
    return 6;
  }
  const fs::path profile = command->HasSwitch("alcove-profile-root")
      ? fs::path(command->GetSwitchValue("alcove-profile-root").ToString()) : directory / "profile";
  if (!profile.is_absolute()) return 2;
  CefSettings settings;
  settings.windowless_rendering_enabled = true;
  settings.log_severity = Client::diagnostics ? LOGSEVERITY_WARNING : LOGSEVERITY_DISABLE;
  CefString(&settings.root_cache_path) = profile.string();
  CefString(&settings.cache_path) = (profile / "default").string();
  CefString(&settings.resources_dir_path) = (cef_root / "Resources").string();
  CefString(&settings.locales_dir_path) = (cef_root / "Resources" / "locales").string();
  CefString(&settings.log_file) = (directory / "cef.log").string();
  Emit({{"event", "stage"}, {"name", "initialize"}});
  if (!CefInitialize(args, settings, app, nullptr)) return 3;
  Emit({{"event", "stage"}, {"name", "initialized"}});
  const bool gpu = command->HasSwitch("probe-gpu");
  const int gpu_socket = gpu ? std::stoi(command->GetSwitchValue("gpu-socket").ToString()) : -1;
  CefRefPtr<Client> client = new Client(directory, gpu ? std::make_shared<GpuBridge>(gpu_socket) : nullptr, command->HasSwitch("probe-native-accessibility"));
  CefWindowInfo window;
  window.SetAsWindowless(0);
  window.shared_texture_enabled = gpu;
  CefBrowserSettings browser_settings;
  browser_settings.windowless_frame_rate = 30;
  std::string url = command->GetSwitchValue("url").ToString();
  const bool created = CefBrowserHost::CreateBrowser(window, client, url, browser_settings, nullptr, nullptr);
  Emit({{"event", "stage"}, {"name", "create-browser"}, {"queued", created}});
  if (!created) { client=nullptr; CefShutdown(); return 4; }
  // All CEF entrypoints run on the same thread; the parent pipe is polled without
  // blocking the CEF message pump. A bounded line buffer rejects oversized input.
  std::string pending;
  bool eof = false;
  Client::Add(client);
  while (!Client::Empty()) {
    CefDoMessageLoopWork();
    NativeAccessibility::Pump();
    Client::Reap();
    pollfd input{STDIN_FILENO, POLLIN, 0};
    if (poll(&input, 1, 5) > 0 && !eof) {
      char buffer[8192];
      const auto count = read(STDIN_FILENO, buffer, sizeof(buffer));
      if (count <= 0) {
        eof = true;
        Client::Dispatch({{"protocol", kProtocol}, {"command", "quit"}});
      } else {
        pending.append(buffer, count);
        if (pending.size() > 65536) {
          Emit({{"event", "protocol-error"}, {"message", "command too large"}});
          pending.clear();
          eof = true;
          Client::Dispatch({{"protocol", kProtocol}, {"command", "quit"}});
        }
        for (size_t pos; (pos = pending.find('\n')) != std::string::npos;) {
          const auto line = pending.substr(0, pos);
          pending.erase(0, pos + 1);
          try { Client::Dispatch(Json::parse(line)); }
          catch (const std::exception& error) {
            Emit({{"event", "protocol-error"}, {"message", error.what()}});
          }
        }
      }
    }
  }
  client = nullptr;
  NativeAccessibility::Shutdown();
  CefShutdown();
  return 0;
}
