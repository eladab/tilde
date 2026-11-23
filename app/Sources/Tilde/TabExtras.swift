import AppKit
import WebKit
import TildeCore

/// Watches a property that has no Swift key path (WebKit's own, reached by name).
final class NamedObserver: NSObject {
    private weak var object: NSObject?
    private let keyPath: String
    private let onChange: () -> Void

    init(_ object: NSObject, keyPath: String, onChange: @escaping () -> Void) {
        self.object = object
        self.keyPath = keyPath
        self.onChange = onChange
        super.init()
        object.addObserver(self, forKeyPath: keyPath, options: [], context: nil)
    }

    override func observeValue(forKeyPath keyPath: String?, of object: Any?, change: [NSKeyValueChangeKey: Any]?,
                               context: UnsafeMutableRawPointer?) {
        onChange()
    }

    func invalidate() {
        object?.removeObserver(self, forKeyPath: keyPath)
        object = nil
    }

    deinit { invalidate() }
}

/// Sound: whether a page is making any, and silencing it. WebKit tells
/// Safari both; neither is public, so they are used by name and only if
/// they are there.
enum PageAudio {
    private static let playingKey = "_isPlayingAudio"
    private static let setMuted = NSSelectorFromString("_setPageMuted:")

    static func observe(_ webView: WKWebView, onChange: @escaping () -> Void) -> NamedObserver? {
        guard webView.responds(to: NSSelectorFromString(playingKey)) else { return nil }
        return NamedObserver(webView, keyPath: playingKey, onChange: onChange)
    }

    static func isPlaying(_ webView: WKWebView) -> Bool {
        guard webView.responds(to: NSSelectorFromString(playingKey)) else { return false }
        return (webView.value(forKey: playingKey) as? NSNumber)?.boolValue ?? false
    }

    /// Answers whether the page could be muted at all.
    @discardableResult
    static func mute(_ webView: WKWebView, _ muted: Bool) -> Bool {
        guard webView.responds(to: setMuted), let method = webView.method(for: setMuted) else { return false }
        typealias SetMuted = @convention(c) (AnyObject, Selector, UInt) -> Void
        // 1 is the audio of the page; capture devices are left as they are.
        unsafeBitCast(method, to: SetMuted.self)(webView, setMuted, muted ? 1 : 0)
        return true
    }
}

/// What a tab was: its pages back and forward, where each was scrolled to,
/// what was typed into its forms. Kept in a file per tab, so that a tab put
/// to sleep, closed and reopened, or restored at the next launch comes back
/// as it was and not merely at its address.
final class TabStateStore {
    private let directory: URL

    init(directory: URL) {
        self.directory = directory
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    static func isToken(_ text: String) -> Bool {
        !text.isEmpty && text.count <= 64 && text.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") }
    }

    private func file(_ token: String) -> URL { directory.appendingPathComponent(token + ".state") }

    func save(_ tab: Tab) {
        // A tab that never arrived anywhere has nothing to keep.
        guard tab.webView.backForwardList.currentItem != nil, let data = tab.webView.interactionState as? Data else { return }
        do {
            try data.write(to: file(tab.token), options: .atomic)
        } catch {
            Log.write("cannot keep the state of tab \(tab.id): \(error.localizedDescription)")
        }
    }

    func load(_ token: String) -> Data? {
        guard Self.isToken(token) else { return nil }
        return try? Data(contentsOf: file(token))
    }

    /// Removes the states of tabs that no longer exist anywhere.
    func keep(only tokens: Set<String>) {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: directory.path)) ?? []
        for name in names where name.hasSuffix(".state") && !tokens.contains(String(name.dropLast(".state".count))) {
            try? FileManager.default.removeItem(at: directory.appendingPathComponent(name))
        }
    }
}

/// Page zoom, remembered per site: a site that was made larger is larger in
/// every tab and after a restart.
final class ZoomStore {
    private var levels: [String: Double] = [:]
    private let file: URL

    init(file: URL) {
        self.file = file
        if let data = try? Data(contentsOf: file), let saved = try? JSONDecoder().decode([String: Double].self, from: data) {
            levels = saved
        }
    }

    func level(for url: URL?) -> CGFloat {
        guard let site = BlockRules.site(of: url), let level = levels[site], level >= 0.3, level <= 5 else { return 1 }
        return CGFloat(level)
    }

    func set(_ level: CGFloat, for url: URL?) {
        guard let site = BlockRules.site(of: url) else { return }
        if abs(level - 1) < 0.001 {
            if levels.removeValue(forKey: site) == nil { return }
        } else {
            levels[site] = Double(level)
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        if let data = try? encoder.encode(levels) { try? data.write(to: file, options: .atomic) }
    }
}

enum UserAgent {
    /// The part of the user agent that names the browser. Sites look for a
    /// Safari version to decide what to serve, so it is the version of the
    /// Safari on this Mac, whose WebKit is the one Tilde shows pages with.
    static let application: String = {
        let safari = Bundle(path: "/Applications/Safari.app")?.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        // Safari has carried the system's number since macOS 26; before that it ran three ahead.
        let system = ProcessInfo.processInfo.operatingSystemVersion
        let fallback = system.majorVersion >= 26 ? "\(system.majorVersion).\(system.minorVersion)" : "\(system.majorVersion + 3).\(system.minorVersion)"
        let version = safari.flatMap { $0.range(of: #"^\d+(\.\d+){0,2}$"#, options: .regularExpression) != nil ? $0 : nil } ?? fallback
        return "Version/\(version) Safari/605.1.15"
    }()
}
