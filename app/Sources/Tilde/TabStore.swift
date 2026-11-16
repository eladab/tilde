import AppKit
import WebKit
import TildeCore

typealias TabID = Int

final class Tab {
    let id: TabID
    let webView: WKWebView
    var observations: [NSKeyValueObservation] = []

    init(id: TabID, webView: WKWebView) {
        self.id = id
        self.webView = webView
    }

    /// Counts the pages this tab has arrived at, for the history. A page that
    /// takes the place of the one before it (a redirect by script, an address
    /// rewritten with `replaceState`, as search engines do) keeps the number,
    /// so the history can tell "another page" from "the same page, renamed".
    private var entry = 0
    private var lastItem: WKBackForwardListItem?
    private var lastBackCount = 0

    private func refreshEntry() {
        let list = webView.backForwardList
        guard let item = list.currentItem else { return }
        let back = list.backList.count
        // A replacement leaves the number of pages behind this one unchanged.
        if lastItem == nil || (item !== lastItem && back != lastBackCount) { entry += 1 }
        lastItem = item
        lastBackCount = back
    }

    var state: [String: Any] {
        refreshEntry()
        return [
            "tab_id": id,
            // The committed page, unlike `url`, which runs ahead while a page loads.
            "entry": entry,
            "entry_url": webView.backForwardList.currentItem?.url.absoluteString ?? "",
            "url": webView.url?.absoluteString ?? "",
            "title": webView.title ?? "",
            "loading": webView.isLoading,
            "progress": webView.estimatedProgress,
            "can_go_back": webView.canGoBack,
            "can_go_forward": webView.canGoForward,
        ]
    }
}

protocol TabStoreDelegate: AnyObject {
    func tabStore(_ store: TabStore, didUpdate tab: Tab)
    func tabStore(_ store: TabStore, didCreate tab: Tab, opener: TabID?, url: String, background: Bool)
    func tabStore(_ store: TabStore, didClose id: TabID)
    func tabStore(_ store: TabStore, didCrash id: TabID)
}

/// Owns every web view and reports changes in their state.
final class TabStore: NSObject, WKNavigationDelegate, WKUIDelegate {
    weak var delegate: TabStoreDelegate?
    /// Given every download a page starts, before it has a destination.
    var onDownload: ((WKDownload) -> Void)?
    private(set) var tabs: [TabID: Tab] = [:]
    private var order: [TabID] = []
    private var nextID: TabID = 1
    private var dirty = Set<TabID>()
    private var flushScheduled = false
    /// One configuration per profile; tabs of a profile share cookies and logins.
    private var configurations: [String: WKWebViewConfiguration] = [:]

    private func configuration(for profile: String?) -> WKWebViewConfiguration {
        let name = (profile?.isEmpty == false) ? profile! : Profile.defaultName
        if let existing = configurations[name] { return existing }
        let configuration = WKWebViewConfiguration()
        if let identifier = Profile.storeIdentifier(for: name) {
            configuration.websiteDataStore = WKWebsiteDataStore(forIdentifier: identifier)
        } else {
            configuration.websiteDataStore = .default()
        }
        // Without this WebKit sends a user agent with no browser token and
        // many sites serve a degraded page.
        configuration.applicationNameForUserAgent = "Version/18.0 Safari/605.1.15"
        configuration.preferences.isElementFullscreenEnabled = true
        configurations[name] = configuration
        return configuration
    }

    var orderedTabs: [Tab] { order.compactMap { tabs[$0] } }

    func tab(_ id: TabID) -> Tab? { tabs[id] }

    func tab(for webView: WKWebView) -> Tab? {
        tabs.values.first { $0.webView === webView }
    }

    @discardableResult
    func create(url: URL?, profile: String? = nil) -> Tab {
        let tab = adopt(WKWebView(frame: .zero, configuration: configuration(for: profile)))
        if let url { tab.webView.load(URLRequest(url: url)) }
        return tab
    }

    private func adopt(_ webView: WKWebView) -> Tab {
        let tab = Tab(id: nextID, webView: webView)
        nextID += 1
        webView.navigationDelegate = self
        webView.uiDelegate = self
        webView.allowsBackForwardNavigationGestures = true
        // Pinch on the trackpad (and smart magnify) zooms the page.
        webView.allowsMagnification = true
        webView.isInspectable = true
        let id = tab.id
        let mark: () -> Void = { [weak self] in self?.markDirty(id) }
        tab.observations = [
            webView.observe(\.title) { _, _ in mark() },
            webView.observe(\.url) { _, _ in mark() },
            webView.observe(\.isLoading) { _, _ in mark() },
            webView.observe(\.estimatedProgress) { _, _ in mark() },
            webView.observe(\.canGoBack) { _, _ in mark() },
            webView.observe(\.canGoForward) { _, _ in mark() },
        ]
        tabs[id] = tab
        order.append(id)
        return tab
    }

    func close(_ id: TabID) {
        guard let tab = tabs.removeValue(forKey: id) else { return }
        order.removeAll { $0 == id }
        dirty.remove(id)
        tab.observations.forEach { $0.invalidate() }
        tab.observations = []
        tab.webView.navigationDelegate = nil
        tab.webView.uiDelegate = nil
        tab.webView.stopLoading()
        tab.webView.removeFromSuperview()
    }

    // Coalesce bursts of KVO changes into at most one event per tab per 50 ms.
    private func markDirty(_ id: TabID) {
        dirty.insert(id)
        if flushScheduled { return }
        flushScheduled = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in
            guard let self else { return }
            self.flushScheduled = false
            let ids = self.dirty
            self.dirty.removeAll()
            for id in ids.sorted() {
                if let tab = self.tabs[id] { self.delegate?.tabStore(self, didUpdate: tab) }
            }
        }
    }

    // MARK: WKNavigationDelegate

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        // A link with the `download` attribute, or "Download Linked File".
        if action.shouldPerformDownload { return decisionHandler(.download) }
        guard let url = action.request.url else { return decisionHandler(.allow) }
        let scheme = url.scheme?.lowercased() ?? ""
        let internalSchemes: Set<String> = ["http", "https", "about", "file", "data", "blob", "javascript"]
        if !internalSchemes.contains(scheme) {
            NSWorkspace.shared.open(url)
            return decisionHandler(.cancel)
        }
        // Cmd-click opens the link in a background tab.
        if action.navigationType == .linkActivated, action.modifierFlags.contains(.command),
           action.targetFrame?.isMainFrame != false {
            let opener = tab(for: webView)?.id
            // Same configuration as the page it came from, so it stays in its profile.
            let tab = adopt(WKWebView(frame: .zero, configuration: webView.configuration))
            tab.webView.load(URLRequest(url: url))
            delegate?.tabStore(self, didCreate: tab, opener: opener, url: url.absoluteString, background: true)
            return decisionHandler(.cancel)
        }
        decisionHandler(.allow)
    }

    func webView(_ webView: WKWebView, decidePolicyFor response: WKNavigationResponse,
                 decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void) {
        let disposition = (response.response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Disposition") ?? ""
        let attachment = disposition.trimmingCharacters(in: .whitespaces).lowercased().hasPrefix("attachment")
        // What WebKit cannot show, and what the server says to save, is downloaded.
        decisionHandler(!response.canShowMIMEType || attachment ? .download : .allow)
    }

    func webView(_ webView: WKWebView, navigationAction: WKNavigationAction, didBecome download: WKDownload) {
        began(download, in: webView)
    }

    func webView(_ webView: WKWebView, navigationResponse: WKNavigationResponse, didBecome download: WKDownload) {
        began(download, in: webView)
    }

    private func began(_ download: WKDownload, in webView: WKWebView) {
        onDownload?(download)
        // A tab opened only to fetch the file never shows a page: close it.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self, weak webView] in
            guard let self, let webView, let tab = self.tab(for: webView),
                  webView.backForwardList.currentItem == nil, !webView.isLoading else { return }
            let id = tab.id
            self.close(id)
            self.delegate?.tabStore(self, didClose: id)
        }
    }

    func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
        if let tab = tab(for: webView) { markDirty(tab.id) }
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        if let tab = tab(for: webView) { delegate?.tabStore(self, didCrash: tab.id) }
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        Log.write("navigation failed: \(error.localizedDescription)")
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        let nsError = error as NSError
        Log.write("provisional navigation failed: \(nsError.localizedDescription)")
        if nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled { return }
        if nsError.domain == "WebKitErrorDomain" && nsError.code == 102 { return } // frame load interrupted
        let failingURL = (nsError.userInfo[NSURLErrorFailingURLErrorKey] as? URL)
            ?? (nsError.userInfo[NSURLErrorFailingURLStringErrorKey] as? String).flatMap(URL.init(string:))
        let html = Self.errorPage(message: nsError.localizedDescription, url: failingURL?.absoluteString ?? "")
        if let failingURL {
            // Keeps the failed address as the tab's URL, so reload retries it.
            webView.loadSimulatedRequest(URLRequest(url: failingURL), responseHTML: html)
        } else {
            webView.loadHTMLString(html, baseURL: nil)
        }
    }

    static func errorPage(message: String, url: String) -> String {
        func esc(_ s: String) -> String {
            s.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;")
        }
        return """
        <!doctype html><meta charset="utf-8"><title>Can't open page</title>
        <body style="font:15px -apple-system;margin:15vh auto;max-width:520px;padding:0 24px;color:#888;background:#18181c">
        <h1 style="font-size:20px;color:#ddd">Can't open this page</h1>
        <p>\(esc(message))</p><p style="word-break:break-all;font-size:13px">\(esc(url))</p></body>
        """
    }

    // MARK: WKUIDelegate

    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        // WebKit requires the new view to be built from the configuration it hands us.
        let opener = tab(for: webView)?.id
        let tab = adopt(WKWebView(frame: .zero, configuration: configuration))
        let background = action.modifierFlags.contains(.command)
        delegate?.tabStore(self, didCreate: tab, opener: opener,
                           url: action.request.url?.absoluteString ?? "", background: background)
        return tab.webView
    }

    func webViewDidClose(_ webView: WKWebView) {
        guard let tab = tab(for: webView) else { return }
        let id = tab.id
        close(id)
        delegate?.tabStore(self, didClose: id)
    }

    // JS dialogs get a real UI in phase 2; until then they are dismissed and logged.
    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping () -> Void) {
        Log.write("js alert dismissed: \(message.prefix(200))")
        completionHandler()
    }

    func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (Bool) -> Void) {
        Log.write("js confirm dismissed: \(message.prefix(200))")
        completionHandler(false)
    }

    func webView(_ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String, defaultText: String?,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (String?) -> Void) {
        Log.write("js prompt dismissed: \(prompt.prefix(200))")
        completionHandler(nil)
    }
}
