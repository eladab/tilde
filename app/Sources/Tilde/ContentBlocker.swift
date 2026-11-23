import Foundation
import WebKit
import TildeCore

/// Blocks requests to advertising and tracking networks, in WebKit itself:
/// the rules are compiled once and applied in the network process, so a
/// blocked request costs nothing and its script never runs.
///
/// The rules come from a short list built in and from a public list of ad
/// servers that is fetched once a week and kept beside the session. A site
/// that breaks can be exempted (`toggle`), which is remembered.
final class ContentBlocker {
    private let directory: URL
    private let enabled: Bool
    private let listURL: URL?
    private unowned let store: TabStore
    /// Sites whose pages are left alone.
    private(set) var exempt: Set<String> = []
    private var current: WKContentRuleList?
    private var compiling = false
    private var compileAgain = false

    private var exemptFile: URL { directory.appendingPathComponent("blocking.json") }
    private var listFile: URL { directory.appendingPathComponent("blocklist.txt") }

    init(directory: URL, settings: Settings, store: TabStore) {
        self.directory = directory
        self.enabled = settings.contentBlocking
        self.listURL = settings.blocklistURL
        self.store = store
        if let data = try? Data(contentsOf: exemptFile),
           let saved = try? JSONDecoder().decode([String].self, from: data) {
            exempt = Set(saved)
        }
    }

    func start() {
        guard enabled else { return Log.write("blocking: off in settings.json") }
        compile()
        refreshListIfOld()
    }

    /// Called for the pages of each profile as its configuration is made.
    func install(on controller: WKUserContentController) {
        if let current { controller.add(current) }
    }

    /// Whether pages of `url`'s site are blocked for.
    func isActive(for url: URL?) -> Bool {
        guard enabled, let site = BlockRules.site(of: url) else { return false }
        return !exempt.contains(site)
    }

    /// Switches blocking off for the site of `url`, or back on. Answers with
    /// the site and whether it is blocked for now; nil when there is nothing
    /// to switch.
    func toggle(for url: URL?, done: @escaping () -> Void) -> (site: String, active: Bool)? {
        guard enabled, let site = BlockRules.site(of: url) else { return nil }
        if exempt.remove(site) == nil { exempt.insert(site) }
        if let data = try? JSONEncoder().encode(exempt.sorted()) { try? data.write(to: exemptFile, options: .atomic) }
        compile(then: done)
        return (site, !exempt.contains(site))
    }

    private var waiting: [() -> Void] = []

    private func compile(then done: (() -> Void)? = nil) {
        if let done { waiting.append(done) }
        if compiling {
            compileAgain = true
            return
        }
        compiling = true
        let fetched = (try? String(contentsOf: listFile, encoding: .utf8)).map(BlockRules.domains(in:)) ?? []
        let exempt = Array(self.exempt)
        DispatchQueue.global(qos: .utility).async {
            let json = BlockRules.json(domains: BlockRules.builtIn + fetched, exempt: exempt)
            let identifier = BlockRules.identifier(for: json)
            // The blocker lasts as long as the app does.
            DispatchQueue.main.async {
                guard let rules = WKContentRuleListStore.default() else { return }
                rules.lookUpContentRuleList(forIdentifier: identifier) { found, _ in
                    if let found { return self.use(found, count: fetched.count) }
                    rules.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: json) { compiled, error in
                        if let error { Log.write("blocking: cannot compile the rules: \(error.localizedDescription)") }
                        self.use(compiled, count: fetched.count)
                    }
                }
            }
        }
    }

    private func use(_ list: WKContentRuleList?, count: Int) {
        compiling = false
        if let list {
            for controller in store.userContentControllers {
                if let current { controller.remove(current) }
                controller.add(list)
            }
            current = list
            Log.write("blocking: \(BlockRules.builtIn.count) built-in and \(count) fetched domains, \(exempt.count) sites exempt")
            // Lists compiled from earlier inputs are of no more use.
            WKContentRuleListStore.default()?.getAvailableContentRuleListIdentifiers { identifiers in
                for old in identifiers ?? [] where old.hasPrefix("tilde-block-") && old != list.identifier {
                    WKContentRuleListStore.default()?.removeContentRuleList(forIdentifier: old) { _ in }
                }
            }
        }
        if compileAgain {
            compileAgain = false
            return compile()
        }
        let done = waiting
        waiting = []
        done.forEach { $0() }
    }

    /// Fetches the list of ad servers again when the copy is a week old.
    private func refreshListIfOld() {
        guard let listURL else { return }
        let modified = (try? FileManager.default.attributesOfItem(atPath: listFile.path))?[.modificationDate] as? Date
        if let modified, -modified.timeIntervalSinceNow < 7 * 24 * 3600 { return }
        let session = URLSession(configuration: .ephemeral)
        var request = URLRequest(url: listURL, timeoutInterval: 30)
        request.setValue("text/plain", forHTTPHeaderField: "Accept")
        session.dataTask(with: request) { [weak self] data, response, error in
            guard let self, let data, (response as? HTTPURLResponse)?.statusCode == 200,
                  let text = String(data: data, encoding: .utf8) else {
                return Log.write("blocking: could not fetch the list: \(error?.localizedDescription ?? "no usable answer")")
            }
            // A page of something else (an error page, a login) is not a list.
            guard BlockRules.domains(in: text).count >= 100 else { return Log.write("blocking: the fetched list is too short to be one") }
            try? text.write(to: self.listFile, atomically: true, encoding: .utf8)
            DispatchQueue.main.async { self.compile() }
        }.resume()
    }
}
