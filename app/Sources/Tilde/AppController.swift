import AppKit
import WebKit
import TildeCore

/// The window gives the main menu first refusal on key equivalents, so a page
/// that swallows every keydown cannot block browser shortcuts.
final class BrowserWindow: NSWindow {
    /// Called for Ctrl+Tab (`false`) and Ctrl+Shift+Tab (`true`).
    var onCycleTabs: ((Bool) -> Void)?

    /// AppKit does not match Ctrl+Shift+Tab against menu items reliably, so
    /// tab cycling is recognised by key code.
    private func handleTabCycling(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.keyCode == 48 else { return false }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard flags.contains(.control), !flags.contains(.command), !flags.contains(.option) else { return false }
        onCycleTabs?(flags.contains(.shift))
        return true
    }

    /// Called for Cmd+Delete while the terminal has the keyboard.
    var onClearLine: (() -> Void)?

    /// The terminal has no encoding for Cmd+Delete and drops it, so the TUI
    /// is told to clear its text field. A focused page keeps the key.
    private func handleClearLine(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown, event.keyCode == 51 else { return false }
        let flags = event.modifierFlags.intersection([.command, .control, .option, .shift])
        guard flags == .command, !Self.isInsideWebView(firstResponder as? NSView) else { return false }
        onClearLine?()
        return true
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        // Before the web view sees it: a focused web view claims every key equivalent.
        if handleTabCycling(event) { return true }
        if handleClearLine(event) { return true }
        if NSApp.mainMenu?.performKeyEquivalent(with: event) == true { return true }
        return super.performKeyEquivalent(with: event)
    }

    override func sendEvent(_ event: NSEvent) {
        if handleTabCycling(event) { return }
        super.sendEvent(event)
    }

    /// Set while the app itself moves focus to the page.
    var allowPageFocus = false

    /// A page can ask for keyboard focus from script (autofocus, `focus()`).
    /// Only a click on the page or an explicit mode switch may grant it;
    /// otherwise a loading page would pull the keyboard away from the TUI.
    @discardableResult
    override func makeFirstResponder(_ responder: NSResponder?) -> Bool {
        if !allowPageFocus, let view = responder as? NSView, Self.isInsideWebView(view),
           !Self.isInsideWebView(firstResponder as? NSView) {
            let mouseTypes: [NSEvent.EventType] = [.leftMouseDown, .rightMouseDown, .otherMouseDown]
            let byClick = NSApp.currentEvent.map { mouseTypes.contains($0.type) } ?? false
            if !byClick { return false }
        }
        return super.makeFirstResponder(responder)
    }

    private static func isInsideWebView(_ view: NSView?) -> Bool {
        var current = view
        while let v = current {
            if v is WKWebView { return true }
            current = v.superview
        }
        return false
    }
}

/// Shows a picture of a page; clicks go through to the page underneath.
final class PlaceholderView: NSView {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

enum Mode: String {
    case nav
    case page
}

final class AppController: NSObject, NSApplicationDelegate, NSWindowDelegate, TabStoreDelegate {
    /// The bundle's version; an unbundled debug build has none.
    static let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "dev"

    private var window: BrowserWindow!
    private var container: NSView!
    private var host: TerminalHost!
    private let store = TabStore()
    private var server: SocketServer!
    private var debugServer: SocketServer?
    private var tui: Connection?

    private var activeID: TabID?
    /// The last picture of the active page, drawn over it until it has drawn itself.
    private var placeholder: NSView?
    private var viewport: CellRect?
    private var viewportVisible = true
    /// True from a TUI crash until the restarted TUI lays the page out again.
    private var pageSuppressed = false
    private var desiredMode: Mode = .nav
    private var reportedMode: Mode = .nav
    private var awaitingRestart = false
    private var quitting = false
    private var responderObservation: NSKeyValueObservation?
    private let passwordFill = PasswordFill()
    /// Records pages' requests while the dev tools are open.
    private lazy var network = NetworkMonitor(store: store)
    private var updates: Updates?
    /// Addresses other apps asked us to open before the TUI could take them.
    private var urlsToOpen: [URL] = []
    /// The TUI has asked for the tabs, so it can be told to open more.
    private var tuiReady = false

    private let supportDir: URL = {
        let dir = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/tilde")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()

    /// The list of downloads lives beside the session; the files go to ~/Downloads.
    private lazy var downloads: DownloadStore = {
        let env = ProcessInfo.processInfo.environment
        var dataDir = supportDir
        if let session = env["TILDE_SESSION"], !session.isEmpty {
            dataDir = URL(fileURLWithPath: session).deletingLastPathComponent()
        }
        var target = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Downloads")
        if let override = env["TILDE_DOWNLOADS"], !override.isEmpty {
            target = URL(fileURLWithPath: override)
        }
        return DownloadStore(file: dataDir.appendingPathComponent("downloads.json"), directory: target)
    }()

    private var socketPath: String { supportDir.appendingPathComponent("tilde-\(getpid()).sock").path }

    // MARK: Launch

    func applicationDidFinishLaunching(_ notification: Notification) {
        Log.write("launch version \(Self.version)")
        store.delegate = self
        store.onDownload = { [weak self] download in self?.downloads.adopt(download) }
        store.onSaveData = { [weak self] data, name, source in self?.downloads.save(data, suggestedFilename: name, from: source) }
        store.monitor = network
        network.onUpdate = { [weak self] tab, reset, late, requests in
            self?.emit("net.updated", ["tab_id": tab, "reset": reset, "late": late, "requests": requests])
        }
        downloads.onUpdate = { [weak self] record in self?.emit("download.updated", record.wire) }
        downloads.onListChange = { [weak self] in
            guard let self else { return }
            self.emit("downloads.changed", ["downloads": self.downloads.records.map(\.wire)])
        }
        NSApp.mainMenu = MainMenu.build(target: self)
        updates = Updates()

        host = SwiftTermHost(font: NSFont.monospacedSystemFont(ofSize: 13, weight: .regular))
        host.onExit = { [weak self] code in self?.tuiExited(code) }
        host.onResize = { [weak self] in self?.applyViewport() }
        host.onInputWhileStopped = { [weak self] in self?.restartTUIIfWaiting() }

        buildWindow()
        startServers()
        startTUI()

        if ProcessInfo.processInfo.environment["TILDE_BACKGROUND"] != nil {
            // Test runs that must not take the keyboard from whatever the user is doing.
            window.orderBack(nil)
        } else {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
        }
        window.makeFirstResponder(host.view)
    }

    private func buildWindow() {
        let cell = host.cellSize
        let screen = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let cols = min(150, Int((screen.width - 40) / cell.width))
        let rows = min(46, Int((screen.height - 80) / cell.height))
        let content = NSRect(x: 0, y: 0, width: CGFloat(cols) * cell.width, height: CGFloat(rows) * cell.height)

        window = BrowserWindow(contentRect: content,
                               styleMask: [.titled, .closable, .miniaturizable, .resizable],
                               backing: .buffered, defer: false)
        window.title = "Tilde"
        window.onCycleTabs = { [weak self] backwards in
            self?.emit("command", ["name": backwards ? "prev-tab" : "next-tab"])
        }
        window.onClearLine = { [weak self] in
            self?.emit("command", ["name": "clear-line"])
        }
        window.delegate = self
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.backgroundColor = NSColor(srgbRed: 0.09, green: 0.09, blue: 0.11, alpha: 1)

        container = NSView(frame: content)
        container.autoresizingMask = [.width, .height]
        window.contentView = container
        host.view.frame = container.bounds
        host.view.autoresizingMask = [.width, .height]
        container.addSubview(host.view)

        applyResizeIncrements()
        window.center()

        responderObservation = window.observe(\.firstResponder) { [weak self] _, _ in
            DispatchQueue.main.async { self?.syncMode() }
        }
    }

    private func applyResizeIncrements() {
        let cell = host.cellSize
        window.contentResizeIncrements = cell
        window.contentMinSize = NSSize(width: cell.width * 80, height: cell.height * 24)
    }

    /// Removes sockets left behind by app processes that did not exit cleanly.
    private func removeStaleSockets() {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: supportDir.path)) ?? []
        for name in names where name.hasPrefix("tilde-") && name.hasSuffix(".sock") {
            let digits = name.dropFirst("tilde-".count).dropLast(".sock".count)
            guard let pid = pid_t(digits), pid != getpid() else { continue }
            // Signal 0 only checks that the process exists.
            if kill(pid, 0) != 0 && errno == ESRCH {
                unlink(supportDir.appendingPathComponent(name).path)
            }
        }
    }

    private func startServers() {
        removeStaleSockets()
        server = SocketServer(path: socketPath)
        server.onConnect = { [weak self] conn in
            guard let self else { return }
            // One TUI at a time: a new connection replaces the old one.
            self.tui?.close()
            self.tui = conn
            Log.write("tui connected")
        }
        server.onLine = { [weak self] conn, line in
            guard let self, conn === self.tui else { return }
            // Requests that have to wait for WebKit answer later, by themselves.
            if let reply = self.handle(line: line, from: conn) { conn.send(reply) }
        }
        server.onDisconnect = { [weak self] conn in
            guard let self, conn === self.tui else { return }
            self.tui = nil
            // Nothing is left to show the requests to.
            self.network.stop()
            Log.write("tui disconnected")
        }
        do {
            try server.start()
        } catch {
            Log.write("cannot start socket at \(socketPath): \(error)")
        }

        let env = ProcessInfo.processInfo.environment
        if let debugPath = env["TILDE_DEBUG_SOCKET"], !debugPath.isEmpty {
            let debug = SocketServer(path: debugPath)
            debug.onLine = { [weak self] conn, line in
                guard let self else { return }
                self.handleDebug(line: line, conn: conn)
            }
            do {
                try debug.start()
                debugServer = debug
                Log.write("debug socket at \(debugPath)")
            } catch {
                Log.write("cannot start debug socket: \(error)")
            }
        }
    }

    // MARK: TUI process

    private func tuiExecutable() -> String? {
        let fm = FileManager.default
        var candidates: [String] = []
        if let override = ProcessInfo.processInfo.environment["TILDE_TUI"], !override.isEmpty {
            candidates.append(override)
        }
        candidates.append(Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/tilde-tui").path)
        if let exe = Bundle.main.executableURL {
            candidates.append(exe.deletingLastPathComponent().appendingPathComponent("tilde-tui").path)
        }
        return candidates.first { fm.isExecutableFile(atPath: $0) }
    }

    private func startTUI() {
        guard let exe = tuiExecutable() else {
            host.display("tilde: cannot find the tilde-tui executable.\r\nSet TILDE_TUI or run from Tilde.app.\r\n")
            Log.write("tilde-tui not found")
            return
        }
        var env = ProcessInfo.processInfo.environment
        env["TERM"] = "xterm-256color"
        env["COLORTERM"] = "truecolor"
        env["TILDE_SOCKET"] = socketPath
        env["TERM_PROGRAM"] = "Tilde"
        env.removeValue(forKey: "TILDE_DEBUG_SOCKET")
        Log.write("starting \(exe)")
        host.start(executable: exe, args: [], environment: env)
    }

    private func tuiExited(_ code: Int32?) {
        Log.write("tilde-tui exited: \(code.map(String.init) ?? "signal")")
        tuiReady = false
        if quitting { return }
        if code == 0 {
            quitting = true
            NSApp.terminate(nil)
            return
        }
        awaitingRestart = true
        pageSuppressed = true
        applyViewport()
        let reason = code.map { "with code \($0)" } ?? "after being killed"
        // The TUI may have died without restoring the terminal: leave the
        // alternate screen, stop mouse reporting and bracketed paste, show the cursor.
        host.display("\u{1b}[?1049l\u{1b}[?1000l\u{1b}[?1002l\u{1b}[?1003l\u{1b}[?1006l\u{1b}[?2004l\u{1b}[0m\u{1b}[?25h")
        host.display("\r\n\u{1b}[1;31mtilde-tui exited \(reason).\u{1b}[0m\r\nYour tabs are still loaded. Press any key to restart.\r\n")
    }

    private func restartTUIIfWaiting() {
        guard awaitingRestart else { return }
        awaitingRestart = false
        host.display("\u{1b}[2J\u{1b}[H")
        startTUI()
    }

    // MARK: Quit

    @objc func quitRequested(_ sender: Any?) {
        if quitting { return }
        guard let tui, host.isProcessRunning else {
            quitting = true
            NSApp.terminate(nil)
            return
        }
        // Let the TUI save its session; it answers with `shutdown`.
        tui.send(Wire.event("command", ["name": "quit"]))
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in
            guard let self, !self.quitting else { return }
            Log.write("tui did not answer quit; terminating")
            self.quitting = true
            NSApp.terminate(nil)
        }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        quitRequested(sender)
        return false
    }

    // MARK: Opening addresses for other apps (links clicked elsewhere, the default browser)

    func application(_ application: NSApplication, open urls: [URL]) {
        openExternally(urls)
    }

    /// Each address becomes a new tab of the space that is showing. Which
    /// space that is, is the TUI's to know, so it is asked to open them.
    private func openExternally(_ urls: [URL]) {
        urlsToOpen += urls.filter { ["http", "https", "file"].contains($0.scheme?.lowercased() ?? "") }
        // At launch this can come before there is a window; the addresses wait.
        if window != nil, ProcessInfo.processInfo.environment["TILDE_BACKGROUND"] == nil {
            window.makeKeyAndOrderFront(nil)
        }
        flushURLsToOpen()
    }

    private func flushURLsToOpen() {
        guard tuiReady, tui != nil, host.isProcessRunning else { return }
        for url in urlsToOpen {
            Log.write("opening for another app: \(url.absoluteString.prefix(200))")
            emit("open-url", ["url": url.absoluteString])
        }
        urlsToOpen = []
    }

    /// Asks macOS to send web links to Tilde. macOS confirms with the user.
    @objc func checkForUpdates(_ sender: Any?) {
        updates?.check()
    }

    @objc func makeDefaultBrowser(_ sender: Any?) {
        let app = Bundle.main.bundleURL
        NSWorkspace.shared.setDefaultApplication(at: app, toOpenURLsWithScheme: "http") { error in
            if let error { Log.write("default browser (http): \(error.localizedDescription)") }
            NSWorkspace.shared.setDefaultApplication(at: app, toOpenURLsWithScheme: "https") { error in
                if let error { Log.write("default browser (https): \(error.localizedDescription)") }
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationWillTerminate(_ notification: Notification) {
        quitting = true
        host.terminateProcess()
        server?.stop()
        debugServer?.stop()
        Log.write("terminated")
    }

    // MARK: Layout

    func windowDidResize(_ notification: Notification) { applyViewport() }

    func windowDidChangeBackingProperties(_ notification: Notification) {
        applyResizeIncrements()
        applyViewport()
    }

    private var activeWebView: WKWebView? { activeID.flatMap { store.tab($0)?.webView } }

    private var pageShown: Bool { viewportVisible && !pageSuppressed && viewport != nil }

    private func applyViewport() {
        guard let web = activeWebView else { return }
        if let viewport {
            let frame = host.frame(for: viewport)
            if web.frame != frame { web.frame = frame }
        }
        let hidden = !pageShown
        if web.isHidden != hidden { web.isHidden = hidden }
        if hidden || placeholder?.frame != web.frame { removePlaceholder() }
        if hidden && currentMode() == .page {
            window.makeFirstResponder(host.view)
        }
    }

    private func activate(_ id: TabID) {
        guard let tab = store.tab(id) else { return }
        if activeID != id {
            let previous = activeID.flatMap { store.tab($0) }
            removePlaceholder()
            activeID = id
            tab.turn += 1
            tab.webView.isHidden = !pageShown
            if let viewport { tab.webView.frame = host.frame(for: viewport) }
            // Above the page being left, which stays in the window until its picture is taken.
            container.addSubview(tab.webView, positioned: .above, relativeTo: previous?.webView ?? host.view)
            if let previous { park(previous) }
            welcomeBack(tab)
        }
        applyViewport()
        applyResponder()
        updateTitle()
    }

    // MARK: Switching between pages

    /// Takes a tab out of the window, keeping a picture of its page for its return.
    private func park(_ tab: Tab) {
        tab.turn += 1
        tab.leftAt = Date()
        let web = tab.webView
        guard web.superview != nil, !web.isHidden, web.url != nil else { return web.removeFromSuperview() }
        let turn = tab.turn
        let size = web.bounds.size
        var parked = false
        let finish: (NSImage?) -> Void = { [weak tab] image in
            guard !parked, let tab, tab.turn == turn else { return }
            parked = true
            tab.webView.removeFromSuperview()
            guard let image else { return }
            DispatchQueue.global(qos: .utility).async {
                guard let jpeg = Self.jpeg(from: image) else { return }
                DispatchQueue.main.async { [weak tab] in
                    if let tab, tab.turn == turn { tab.snapshot = (jpeg, size) }
                }
            }
        }
        web.takeSnapshot(with: nil) { image, _ in finish(image) }
        // A page that cannot be pictured must not stay in the window, where it would keep running.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { finish(nil) }
    }

    /// Covers a page that has been away with its last picture until it has drawn
    /// itself again, and notes in the log how long that took.
    private func welcomeBack(_ tab: Tab) {
        guard let leftAt = tab.leftAt, pageShown else { return }
        let started = Date()
        let turn = tab.turn
        let web = tab.webView
        var cover = "no picture"
        if let snapshot = tab.snapshot {
            if abs(snapshot.size.width - web.frame.width) > 1 || abs(snapshot.size.height - web.frame.height) > 1 {
                cover = "picture of another size"
            } else if let image = Self.image(from: snapshot.jpeg) {
                let view = PlaceholderView(frame: web.frame)
                view.wantsLayer = true
                view.layer?.contents = image
                view.layer?.contentsGravity = .resize
                container.addSubview(view, positioned: .above, relativeTo: web)
                placeholder = view
                cover = "picture ready in \(Self.milliseconds(since: started)) ms"
            }
        }
        var done = false
        let finish: (String) -> Void = { [weak self, weak tab] outcome in
            guard !done, let self, let tab, tab.turn == turn else { return }
            done = true
            self.removePlaceholder()
            let host = tab.webView.backForwardList.currentItem?.url.host ?? "-"
            Log.write("switch: tab \(tab.id) \(host) \(outcome) after \(Int(started.timeIntervalSince(leftAt))) s away (\(cover))")
        }
        // Frames are only produced for a page that is visible, so the second one
        // means the page is awake and has drawn itself.
        let script = "await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))"
        web.callAsyncJavaScript(script, arguments: [:], in: nil, in: .defaultClient) { result in
            switch result {
            case .success: finish("drawn in \(Self.milliseconds(since: started)) ms")
            case .failure(let error): finish("not measured (\(error.localizedDescription))")
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { finish("not drawn within 2000 ms") }
    }

    private func removePlaceholder() {
        placeholder?.removeFromSuperview()
        placeholder = nil
    }

    private static func milliseconds(since date: Date) -> Int { Int(-date.timeIntervalSinceNow * 1000) }

    private static func jpeg(from image: NSImage) -> Data? {
        guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return nil }
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, "public.jpeg" as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(destination, cgImage, [kCGImageDestinationLossyCompressionQuality: 0.8] as CFDictionary)
        return CGImageDestinationFinalize(destination) ? data as Data : nil
    }

    private static func image(from jpeg: Data) -> CGImage? {
        guard let source = CGImageSourceCreateWithData(jpeg as CFData, nil) else { return nil }
        // Decoded now, not while the window is being drawn.
        return CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary)
    }

    private func updateTitle() {
        let title = activeWebView?.title ?? ""
        window.title = title.isEmpty ? "Tilde" : title
    }

    // MARK: Modes

    private func currentMode() -> Mode {
        guard let web = activeWebView, let responder = window.firstResponder as? NSView else { return .nav }
        return responder === web || responder.isDescendant(of: web) ? .page : .nav
    }

    /// Put keyboard focus where `desiredMode` says it should be.
    private func applyResponder() {
        if desiredMode == .page, let web = activeWebView, pageShown {
            if currentMode() != .page {
                window.allowPageFocus = true
                window.makeFirstResponder(web)
                window.allowPageFocus = false
            }
        } else if window.firstResponder !== host.view {
            window.makeFirstResponder(host.view)
        }
        syncMode()
    }

    /// Report the real focus state; the TUI only ever displays what it is told.
    private func syncMode() {
        // The relay field of a password fill holds focus on the page's behalf.
        if passwordFill.isActive { return }
        let mode = currentMode()
        if mode == reportedMode { return }
        reportedMode = mode
        desiredMode = mode
        emit("focus.changed", ["mode": mode.rawValue])
    }

    // MARK: Menu commands

    @objc func menuCommand(_ sender: NSMenuItem) {
        guard let command = sender.representedObject as? MenuCommand else { return }
        var fields: [String: Any] = ["name": command.name]
        if let arg = command.arg { fields["arg"] = arg }
        emit("command", fields)
    }

    /// Copy in the page is WebKit's. In the terminal the only text a user can
    /// select is a field the TUI draws, so the TUI is asked for it and answers
    /// with `clipboard.set`.
    @objc func copyRequested(_ sender: Any?) {
        if window.firstResponder === host.view {
            emit("command", ["name": "copy"])
        } else {
            NSApp.sendAction(#selector(NSText.copy(_:)), to: nil, from: sender)
        }
    }

    @objc func fillFromPasswords(_ sender: Any?) {
        guard currentMode() == .page, let web = activeWebView, !passwordFill.isActive else {
            return NSSound.beep()
        }
        passwordFill.begin(web: web, window: window, container: container) { [weak self] outcome in
            Log.write("fill: \(outcome)")
            if outcome.hasPrefix("failed") { NSSound.beep() }
            self?.syncMode()
        }
    }

    // MARK: Page zoom

    /// The steps Cmd+Plus and Cmd+Minus move through, as in Safari.
    private static let zoomSteps: [CGFloat] = [0.5, 0.67, 0.75, 0.85, 1, 1.15, 1.25, 1.5, 1.75, 2, 2.5, 3]

    /// Zoom is per tab and lasts as long as the tab's web view.
    private func stepZoom(_ direction: Int) {
        guard let web = activeWebView else { return NSSound.beep() }
        let current = web.pageZoom
        let next = direction > 0
            ? Self.zoomSteps.first { $0 > current + 0.001 }
            : Self.zoomSteps.last { $0 < current - 0.001 }
        guard let next else { return NSSound.beep() }
        web.pageZoom = next
    }

    @objc func zoomIn(_ sender: Any?) { stepZoom(1) }

    @objc func zoomOut(_ sender: Any?) { stepZoom(-1) }

    /// Undoes both Cmd+Plus/Minus zoom and a trackpad pinch.
    @objc func zoomReset(_ sender: Any?) {
        guard let web = activeWebView else { return NSSound.beep() }
        web.pageZoom = 1
        web.magnification = 1
    }

    private func emit(_ event: String, _ fields: [String: Any] = [:]) {
        tui?.send(Wire.event(event, fields))
    }

    // MARK: TabStoreDelegate

    func tabStore(_ store: TabStore, didUpdate tab: Tab) {
        emit("tab.updated", tab.state)
        if tab.id == activeID { updateTitle() }
    }

    func tabStore(_ store: TabStore, didCreate tab: Tab, opener: TabID?, url: String, background: Bool) {
        var fields: [String: Any] = ["tab_id": tab.id, "url": url, "background": background]
        if let opener { fields["opener_tab_id"] = opener }
        emit("tab.created", fields)
    }

    func tabStore(_ store: TabStore, didClose id: TabID) {
        if activeID == id {
            activeID = nil
            removePlaceholder()
            window.makeFirstResponder(host.view)
            updateTitle()
        }
        emit("tab.closed", ["tab_id": id])
    }

    func tabStore(_ store: TabStore, didCrash id: TabID) {
        emit("tab.crashed", ["tab_id": id])
    }

    // MARK: Protocol

    /// Answers a request from the TUI. `nil` means the answer is sent to
    /// `conn` later, when WebKit has given it.
    private func handle(line: String, from conn: Connection) -> String? {
        let req: Wire.Request
        do {
            req = try Wire.decodeRequest(line)
        } catch {
            Log.write("bad request: \(line.prefix(200))")
            return Wire.failure(id: nil, "malformed request")
        }
        let id = req.id

        func withTab(_ body: (Tab) -> String?) -> String? {
            guard let tabID = req.int("tab_id") else { return Wire.failure(id: id, "missing tab_id") }
            guard let tab = store.tab(tabID) else { return Wire.failure(id: id, "no such tab") }
            return body(tab)
        }

        switch req.method {
        case "hello":
            guard req.int("protocol") == Wire.protocolVersion else {
                return Wire.failure(id: id, "unsupported protocol version")
            }
            return Wire.reply(id: id, [
                "app_version": Self.version,
                "mode": currentMode().rawValue,
                "instance": Int(getpid()),
            ])

        case "viewport.set":
            guard let col = req.int("col"), let row = req.int("row"),
                  let cols = req.int("cols"), let rows = req.int("rows"),
                  col >= 0, row >= 0, cols >= 0, rows >= 0 else {
                return Wire.failure(id: id, "viewport.set needs col, row, cols, rows")
            }
            viewport = CellRect(col: col, row: row, cols: cols, rows: rows)
            pageSuppressed = false
            applyViewport()
            return Wire.reply(id: id)

        case "viewport.visible":
            guard let visible = req.bool("visible") else { return Wire.failure(id: id, "missing visible") }
            viewportVisible = visible
            applyViewport()
            applyResponder()
            return Wire.reply(id: id)

        case "tab.list":
            // Once it has the tabs the TUI can take addresses to open; they
            // must reach it after this reply.
            tuiReady = true
            DispatchQueue.main.async { [weak self] in self?.flushURLsToOpen() }
            var fields: [String: Any] = ["tabs": store.orderedTabs.map { $0.state }]
            if let activeID { fields["active"] = activeID }
            return Wire.reply(id: id, fields)

        case "tab.create":
            var url: URL?
            if let raw = req.string("url"), !raw.isEmpty {
                url = URL(string: raw)
            } else if let input = req.string("input") {
                url = InputResolver.resolve(input)
            }
            let tab = store.create(url: url, profile: req.string("profile"))
            if req.bool("activate") == true { activate(tab.id) }
            return Wire.reply(id: id, ["tab_id": tab.id])

        case "tab.close":
            return withTab { tab in
                let wasActive = tab.id == activeID
                store.close(tab.id)
                if wasActive {
                    activeID = nil
                    removePlaceholder()
                    window.makeFirstResponder(host.view)
                    updateTitle()
                }
                return Wire.reply(id: id)
            }

        case "tab.activate":
            return withTab { tab in
                activate(tab.id)
                return Wire.reply(id: id)
            }

        case "tab.navigate":
            return withTab { tab in
                guard let url = InputResolver.resolve(req.string("input") ?? "") else {
                    return Wire.failure(id: id, "empty input")
                }
                tab.webView.load(URLRequest(url: url))
                return Wire.reply(id: id, ["url": url.absoluteString])
            }

        case "tab.back":
            return withTab { tab in tab.webView.goBack(); return Wire.reply(id: id) }
        case "tab.forward":
            return withTab { tab in tab.webView.goForward(); return Wire.reply(id: id) }
        case "tab.reload":
            return withTab { tab in
                // After a crash or a failed load there is nothing to reload in place.
                if tab.webView.url == nil, let item = tab.webView.backForwardList.currentItem {
                    tab.webView.go(to: item)
                } else {
                    tab.webView.reload()
                }
                return Wire.reply(id: id)
            }
        case "tab.stop":
            return withTab { tab in tab.webView.stopLoading(); return Wire.reply(id: id) }

        case "focus.set":
            guard let raw = req.string("mode"), let mode = Mode(rawValue: raw) else {
                return Wire.failure(id: id, "mode must be nav or page")
            }
            desiredMode = mode
            applyResponder()
            return Wire.reply(id: id, ["mode": currentMode().rawValue])

        case "download.list":
            return Wire.reply(id: id, ["downloads": downloads.records.map(\.wire)])

        case "download.open", "download.reveal", "download.cancel", "download.remove":
            guard let download = req.int("download_id") else { return Wire.failure(id: id, "missing download_id") }
            var problem: String?
            switch req.method {
            case "download.open": problem = downloads.open(download)
            case "download.reveal": problem = downloads.reveal(download)
            case "download.cancel": downloads.cancel(download)
            default: downloads.remove(download)
            }
            return problem.map { Wire.failure(id: id, $0) } ?? Wire.reply(id: id)

        case "download.clear":
            downloads.clear()
            return Wire.reply(id: id)

        case "clipboard.set":
            guard let text = req.string("text") else { return Wire.failure(id: id, "missing text") }
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            return Wire.reply(id: id)

        case "net.watch":
            // Without a tab, requests are still recorded but none are reported.
            let tabID = req.int("tab_id")
            if let tabID, store.tab(tabID) == nil { return Wire.failure(id: id, "no such tab") }
            network.start()
            network.watched = tabID
            return Wire.reply(id: id, ["requests": tabID.map(network.requests(of:)) ?? [],
                                       "late": tabID.map(network.isLate) ?? false])

        case "net.stop":
            network.stop()
            return Wire.reply(id: id)

        case "net.get":
            return withTab { tab in
                guard let number = req.int("request_id"), let request = network.request(number, of: tab.id) else {
                    return Wire.failure(id: id, "no such request")
                }
                return Wire.reply(id: id, ["request": request])
            }

        case "net.clear":
            return withTab { tab in
                network.clear(tab.id)
                return Wire.reply(id: id)
            }

        case "net.preserve":
            guard let enabled = req.bool("enabled") else { return Wire.failure(id: id, "missing enabled") }
            network.preserve = enabled
            return Wire.reply(id: id)

        case "storage.get":
            return withTab { tab in
                StorageInspector.read(tab) { fields in conn.send(Wire.reply(id: id, fields)) }
                return nil
            }

        case "storage.delete":
            return withTab { tab in
                StorageInspector.delete(tab, request: req) { problem in
                    conn.send(problem.map { Wire.failure(id: id, $0) } ?? Wire.reply(id: id))
                }
                return nil
            }

        case "shutdown":
            quitting = true
            DispatchQueue.main.async { NSApp.terminate(nil) }
            return Wire.reply(id: id)

        default:
            return Wire.failure(id: id, "unknown method \(req.method)")
        }
    }

    // MARK: Debug socket (only with TILDE_DEBUG_SOCKET; used by the end-to-end tests)

    private func handleDebug(line: String, conn: Connection) {
        guard let req = try? Wire.decodeRequest(line) else {
            conn.send(Wire.failure(id: nil, "malformed request"))
            return
        }
        let id = req.id
        switch req.method {
        case "debug.state":
            let grid = host.gridSize
            let cell = host.cellSize
            var fields: [String: Any] = [
                "mode": currentMode().rawValue,
                "tabs": store.orderedTabs.map { $0.state },
                "grid": ["cols": grid.cols, "rows": grid.rows],
                "cell": ["w": cell.width, "h": cell.height],
                "viewport_visible": viewportVisible,
                "app_active": NSApp.isActive,
                "pid": Int(getpid()),
                "tui_pid": Int(host.processID ?? 0),
                "tui_running": host.isProcessRunning,
                "tui_connected": tui != nil,
                "awaiting_restart": awaitingRestart,
                "window_number": window.windowNumber,
                "window_title": window.title,
                "content": ["w": container.bounds.width, "h": container.bounds.height],
                "scale": window.backingScaleFactor,
                "downloads": downloads.records.map(\.wire),
                "first_responder": window.firstResponder.map { String(describing: Swift.type(of: $0)) } ?? "",
            ]
            if let activeID { fields["active"] = activeID }
            if let viewport {
                fields["viewport"] = ["col": viewport.col, "row": viewport.row, "cols": viewport.cols, "rows": viewport.rows]
            }
            if let web = activeWebView {
                fields["web_frame"] = ["x": web.frame.minX, "y": web.frame.minY, "w": web.frame.width, "h": web.frame.height]
                fields["web_hidden"] = web.isHidden
                fields["page_zoom"] = web.pageZoom
                fields["magnification"] = web.magnification
                fields["allows_magnification"] = web.allowsMagnification
            }
            conn.send(Wire.reply(id: id, fields))

        case "debug.activate":
            // Synthesised clicks are only delivered to views of the active app.
            NSApp.activate(ignoringOtherApps: true)
            window.makeKeyAndOrderFront(nil)
            conn.send(Wire.reply(id: id))

        case "debug.open_url":
            // What macOS does when a link is clicked in another app.
            if let url = URL(string: req.string("url") ?? "") { openExternally([url]) }
            conn.send(Wire.reply(id: id))

        case "debug.type":
            host.type(req.string("text") ?? "")
            conn.send(Wire.reply(id: id))

        case "debug.screen":
            conn.send(Wire.reply(id: id, ["text": host.screenText()]))

        case "debug.key":
            // Synthesises a key press through the normal AppKit dispatch path.
            let chars = req.string("chars") ?? ""
            var flags: NSEvent.ModifierFlags = []
            for mod in (req.string("mods") ?? "").split(separator: "+") {
                switch mod {
                case "cmd": flags.insert(.command)
                case "ctrl": flags.insert(.control)
                case "shift": flags.insert(.shift)
                case "alt": flags.insert(.option)
                default: break
                }
            }
            let code = UInt16(req.int("key_code") ?? 0)
            for type in [NSEvent.EventType.keyDown, .keyUp] {
                if let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: flags,
                                                timestamp: ProcessInfo.processInfo.systemUptime,
                                                windowNumber: window.windowNumber, context: nil,
                                                characters: chars, charactersIgnoringModifiers: chars,
                                                isARepeat: false, keyCode: code) {
                    NSApp.postEvent(event, atStart: false)
                }
            }
            conn.send(Wire.reply(id: id))

        case "debug.click":
            // Click at a cell (terminal) or at a point inside the page.
            let target = req.string("target") ?? "cell"
            var point = NSPoint.zero
            if target == "page", let web = activeWebView {
                point = NSPoint(x: web.frame.minX + CGFloat(req.int("x") ?? 10),
                                y: web.frame.maxY - CGFloat(req.int("y") ?? 10))
            } else {
                let rect = host.frame(for: CellRect(col: req.int("col") ?? 0, row: req.int("row") ?? 0, cols: 1, rows: 1))
                point = NSPoint(x: rect.midX, y: rect.midY)
            }
            let inWindow = container.convert(point, to: nil)
            // WebKit hit-tests on mouse movement, so a click needs a move before it.
            for type in [NSEvent.EventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
                if let event = NSEvent.mouseEvent(with: type, location: inWindow, modifierFlags: [],
                                                  timestamp: ProcessInfo.processInfo.systemUptime,
                                                  windowNumber: window.windowNumber, context: nil,
                                                  eventNumber: 0, clickCount: type == .mouseMoved ? 0 : 1, pressure: type == .mouseMoved ? 0 : 1) {
                    NSApp.postEvent(event, atStart: false)
                }
            }
            conn.send(Wire.reply(id: id))

        case "debug.js":
            guard let web = activeWebView else {
                conn.send(Wire.failure(id: id, "no active tab"))
                return
            }
            web.evaluateJavaScript(req.string("script") ?? "") { result, error in
                if let error {
                    conn.send(Wire.failure(id: id, error.localizedDescription))
                } else {
                    conn.send(Wire.reply(id: id, ["result": result.map { "\($0)" } ?? ""]))
                }
            }

        case "debug.resize":
            let cell = host.cellSize
            let size = NSSize(width: CGFloat(req.int("cols") ?? 120) * cell.width,
                              height: CGFloat(req.int("rows") ?? 40) * cell.height)
            window.setContentSize(size)
            conn.send(Wire.reply(id: id))

        case "debug.kill_web":
            // Simulates a web content process crash for the active tab.
            // Kills the process from outside, as a real crash would.
            if let web = activeWebView,
               let pid = web.value(forKey: "_webProcessIdentifier") as? Int32, pid > 0 {
                kill(pid, SIGKILL)
                conn.send(Wire.reply(id: id, ["pid": Int(pid)]))
            } else {
                conn.send(Wire.failure(id: id, "no web process"))
            }

        default:
            conn.send(Wire.failure(id: id, "unknown method \(req.method)"))
        }
    }
}

extension AppController: NSMenuItemValidation {
    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        if menuItem.action == #selector(checkForUpdates(_:)) {
            return updates?.canCheck ?? false
        }
        return true
    }
}
