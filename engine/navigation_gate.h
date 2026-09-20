// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "include/cef_resource_request_handler.h"
#include "include/cef_task.h"
#include "site_requests.h"

class NavigationTask final : public CefTask {
 public:
  explicit NavigationTask(std::function<void()> run) : run_(std::move(run)) {}
  void Execute() override { run_(); }
 private:
  std::function<void()> run_;
  IMPLEMENT_REFCOUNTING(NavigationTask);
};

// One handler per top-level resource request. Only the UI thread touches
// SiteRequests; IO-thread callbacks retain their own reference until completion.
class NavigationGate final : public CefResourceRequestHandler {
 public:
  explicit NavigationGate(CefRefPtr<SiteRequests> requests) : requests_(requests) {}
  ReturnValue OnBeforeResourceLoad(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame>,
      CefRefPtr<CefRequest> request, CefRefPtr<CefCallback> callback) override {
    const auto requests = requests_;
    const auto id = request->GetIdentifier();
    const auto url = request->GetURL().ToString();
    if (!CefPostTask(TID_UI, new NavigationTask([requests, id, url, callback] {
      requests->Navigation(id, url, callback);
    }))) return RV_CANCEL;
    return RV_CONTINUE_ASYNC;
  }
  void OnResourceLoadComplete(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame>,
      CefRefPtr<CefRequest> request, CefRefPtr<CefResponse>, URLRequestStatus, int64_t) override {
    const auto requests = requests_;
    const auto id = request->GetIdentifier();
    CefPostTask(TID_UI, new NavigationTask([requests, id] { requests->NavigationComplete(id); }));
  }
 private:
  CefRefPtr<SiteRequests> requests_;
  IMPLEMENT_REFCOUNTING(NavigationGate);
};
