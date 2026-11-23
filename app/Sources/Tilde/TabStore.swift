import AppKit
import WebKit
import TildeCore

typealias TabID = Int

final class Tab {
    let id: TabID
    let webView: WKWebView
    var observations: [NSKeyValueObservation] = []

    /// When the tab was last switched away from; nil while it has never been left.
    var leftAt: Date?
    /// What the page looked like then, as JPEG, with its size in points. It
    /// stands in for the page on return, until the page has drawn itself.
    var snapshot: (jpeg: Data, size: NSSize)?
    /// Advances each time the tab is shown or left, so that work begun for an
    /// earlier turn can tell it is out of date.
    var turn = 0
    /// Advances with each new text searched for in the page.
    var findGeneration = 0

    /// Names the file the tab's state is kept in (see `TabStateStore`). It
    /// outlives the web view: a tab made again from its state keeps the name.
    let token: String
    /// The page's sound is switched off.
    var muted = false
    var audioObservation: NamedObserver?
    /// Dialogs the page's script has put up since the tab arrived at it.
    var dialogCount = 0
    /// The user asked to see no more of them from this page.
    var dialogsBlocked = false

    init(id: TabID, webView: WKWebView, token: String) {
        self.id = id
        self.webView = webView
        self.token = token
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
            // Muted devices count: the page still holds them.
            "camera": webView.cameraCaptureState != .none,
            "microphone": webView.microphoneCaptureState != .none,
            "audible": PageAudio.isPlaying(webView),
            "muted": muted,
            "state": token,
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
    /// Given a file a page hands over whole, with the name it suggests and where it came from.
    var onSaveData: ((Data, String, URL?) -> Void)?
    /// A page wants the camera or the microphone; the answer goes to the handler.
    var onMediaRequest: ((WKSecurityOrigin, WKMediaCaptureType, @escaping (WKPermissionDecision) -> Void) -> Void)?
    /// What a search in a tab found: the text, how many matches (-1 when that
    /// is not known), and which of them the page is on, counting from 1.
    var onFind: ((TabID, String, Int, Int) -> Void)?
    /// Told of every navigation while the dev tools are recording requests.
    var monitor: NetworkMonitor?
    var blocker: ContentBlocker?
    var states: TabStateStore?
    var zoom: ZoomStore?
    var favicons: Favicons?
    /// The window that sheets are shown on.
    var presenter: (() -> NSWindow?)?
    /// The answer awaited from the certificate chooser, which reports back through a selector.
    var certificateAnswer: ((Bool) -> Void)?
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
        configuration.applicationNameForUserAgent = UserAgent.application
        configuration.preferences.isElementFullscreenEnabled = true
        configurations[name] = configuration
        monitor?.install(on: configuration.userContentController)
        blocker?.install(on: configuration.userContentController)
        return configuration
    }

    /// One per profile: where scripts for that profile's pages are installed.
    var userContentControllers: [WKUserContentController] {
        var seen: [WKUserContentController] = []
        let all = configurations.values.map(\.userContentController) + tabs.values.map(\.webView.configuration.userContentController)
        for controller in all where !seen.contains(where: { $0 === controller }) { seen.append(controller) }
        return seen
    }

    var orderedTabs: [Tab] { order.compactMap { tabs[$0] } }

    func tab(_ id: TabID) -> Tab? { tabs[id] }

    func tab(for webView: WKWebView) -> Tab? {
        tabs.values.first { $0.webView === webView }
    }

    /// Makes a tab. With `state`, the name of a state kept earlier, the tab
    /// comes back as it was: its pages back and forward, and where it was
    /// scrolled to. `url` is where it goes when there is no such state.
    @discardableResult
    func create(url: URL?, profile: String? = nil, state: String? = nil) -> Tab {
        let token = state.flatMap { TabStateStore.isToken($0) ? $0 : nil }
        let tab = adopt(WKWebView(frame: .zero, configuration: configuration(for: profile)), token: token)
        if let token, let saved = states?.load(token) {
            tab.webView.interactionState = saved
            if tab.webView.backForwardList.currentItem != nil { return tab }
            Log.write("tab \(tab.id): its kept state could not be used")
        }
        if let url { tab.webView.load(URLRequest(url: url)) }
        return tab
    }

    private func adopt(_ webView: WKWebView, token: String? = nil) -> Tab {
        let tab = Tab(id: nextID, webView: webView, token: token ?? UUID().uuidString.lowercased())
        nextID += 1
        webView.navigationDelegate = self
        webView.uiDelegate = self
        webView.allowsBackForwardNavigationGestures = true
        // Pinch on the trackpad (and smart magnify) zooms the page.
        webView.allowsMagnification = true
        webView.isInspectable = true
        watchFinds(in: webView)
        let id = tab.id
        let mark: () -> Void = { [weak self] in self?.markDirty(id) }
        tab.observations = [
            webView.observe(\.title) { _, _ in mark() },
            webView.observe(\.url) { _, _ in mark() },
            webView.observe(\.isLoading) { _, _ in mark() },
            webView.observe(\.estimatedProgress) { _, _ in mark() },
            webView.observe(\.canGoBack) { _, _ in mark() },
            webView.observe(\.canGoForward) { _, _ in mark() },
            webView.observe(\.cameraCaptureState) { _, _ in mark() },
            webView.observe(\.microphoneCaptureState) { _, _ in mark() },
        ]
        tab.audioObservation = PageAudio.observe(webView, onChange: mark)
        tabs[id] = tab
        order.append(id)
        return tab
    }

    func close(_ id: TabID) {
        guard let tab = tabs.removeValue(forKey: id) else { return }
        // Kept so that the tab can be reopened, or woken, as it was.
        states?.save(tab)
        tab.audioObservation?.invalidate()
        tab.audioObservation = nil
        monitor?.tabClosed(id)
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
        if let monitor, let tab = tab(for: webView) { monitor.willNavigate(tab, action: action) }
        decisionHandler(.allow)
    }

    func webView(_ webView: WKWebView, decidePolicyFor response: WKNavigationResponse,
                 decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void) {
        let disposition = (response.response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Disposition") ?? ""
        let attachment = disposition.trimmingCharacters(in: .whitespaces).lowercased().hasPrefix("attachment")
        // What WebKit cannot show, and what the server says to save, is downloaded.
        let download = !response.canShowMIMEType || attachment
        if let monitor, let tab = tab(for: webView) { monitor.didRespond(tab, response: response, shown: !download) }
        decisionHandler(download ? .download : .allow)
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
        guard let tab = tab(for: webView) else { return }
        tab.dialogCount = 0
        tab.dialogsBlocked = false
        // Each site is shown at the size it was last given.
        if let zoom {
            let level = zoom.level(for: webView.url)
            if abs(webView.pageZoom - level) > 0.001 { webView.pageZoom = level }
        }
        markDirty(tab.id)
        monitor?.didCommit(tab)
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        favicons?.learn(from: webView)
        if let monitor, let tab = tab(for: webView) { monitor.didFinish(tab) }
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        guard let tab = tab(for: webView) else { return }
        let host = webView.backForwardList.currentItem?.url.host ?? "-"
        let away = tab.leftAt.map { "\(Int(-$0.timeIntervalSinceNow)) s in the background" } ?? "in front"
        Log.write("web process terminated: tab \(tab.id) \(host), \(away)")
        delegate?.tabStore(self, didCrash: tab.id)
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        Log.write("navigation failed: \(error.localizedDescription)")
        if let monitor, let tab = tab(for: webView) { monitor.didFail(tab, url: webView.url, error: error, errorPage: false) }
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        let nsError = error as NSError
        Log.write("provisional navigation failed: \(nsError.localizedDescription)")
        if nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled { return }
        if nsError.domain == "WebKitErrorDomain" && nsError.code == 102 { return } // frame load interrupted
        let failingURL = (nsError.userInfo[NSURLErrorFailingURLErrorKey] as? URL)
            ?? (nsError.userInfo[NSURLErrorFailingURLStringErrorKey] as? String).flatMap(URL.init(string:))
        if let monitor, let tab = tab(for: webView) { monitor.didFail(tab, url: failingURL, error: error, errorPage: true) }
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

    /// The save button WebKit lays over a PDF it is showing. WebKit has the
    /// file already and offers it here; without this the button does nothing.
    @objc(_webView:saveDataToFile:suggestedFilename:mimeType:originatingURL:)
    func webView(_ webView: WKWebView, saveDataToFile data: Data, suggestedFilename: String, mimeType: String,
                 originatingURL: URL?) {
        onSaveData?(data, suggestedFilename, originatingURL ?? webView.url)
    }

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

    func webView(_ webView: WKWebView, requestMediaCapturePermissionFor origin: WKSecurityOrigin,
                 initiatedByFrame frame: WKFrameInfo, type: WKMediaCaptureType,
                 decisionHandler: @escaping (WKPermissionDecision) -> Void) {
        guard let onMediaRequest else { return decisionHandler(.deny) }
        onMediaRequest(origin, type, decisionHandler)
    }

    func webViewDidClose(_ webView: WKWebView) {
        guard let tab = tab(for: webView) else { return }
        let id = tab.id
        close(id)
        delegate?.tabStore(self, didClose: id)
    }
}
