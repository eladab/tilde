import AppKit
import WebKit
import TildeCore

/// The icons of sites, shown beside the tabs. A terminal cannot draw a
/// picture, so the TUI says which cells belong to which address and the app
/// lays the icons over them (`IconStrip`).
///
/// An icon is learned from a page once it has loaded (its `<link rel=icon>`,
/// or `/favicon.ico`), fetched apart from the page with no cookies, and kept
/// on disk by site, so that tabs which are not loaded have theirs too.
final class Favicons {
    private let directory: URL
    private var images: [String: CGImage] = [:]
    /// Sites known to have no usable icon, or being fetched: not asked again in this run.
    private var settled = Set<String>()
    private let session = URLSession(configuration: .ephemeral)
    /// A site's icon became known.
    var onChange: (() -> Void)?

    init(directory: URL) {
        self.directory = directory
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    private func file(_ site: String) -> URL {
        directory.appendingPathComponent(site.replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: ":", with: "_"))
    }

    private static func decode(_ data: Data) -> CGImage? {
        guard let image = NSImage(data: data), image.size.width >= 8, image.size.height >= 8 else { return nil }
        // The largest representation, drawn down to what a cell can show.
        var rect = NSRect(x: 0, y: 0, width: 64, height: 64)
        return image.cgImage(forProposedRect: &rect, context: nil, hints: nil)
    }

    func image(for address: String) -> CGImage? {
        guard let site = BlockRules.site(of: URL(string: address)) else { return nil }
        if let known = images[site] { return known }
        guard !settled.contains(site) else { return nil }
        if let data = try? Data(contentsOf: file(site)), let image = Self.decode(data) {
            images[site] = image
            return image
        }
        return nil
    }

    /// Finds out the icon of the page a tab has just loaded.
    func learn(from webView: WKWebView) {
        guard let page = webView.url, ["http", "https"].contains(page.scheme ?? ""),
              let site = BlockRules.site(of: page), images[site] == nil, !settled.contains(site) else { return }
        // On disk from an earlier run, and young enough: nothing to fetch.
        if let modified = (try? FileManager.default.attributesOfItem(atPath: file(site).path))?[.modificationDate] as? Date,
           -modified.timeIntervalSinceNow < 14 * 24 * 3600, image(for: page.absoluteString) != nil { return }
        settled.insert(site)
        let script = """
        (() => {
          const links = [...document.querySelectorAll('link[rel~="icon" i], link[rel="apple-touch-icon" i], link[rel="shortcut icon" i]')];
          const size = l => { const m = /(\\d+)x\\d+/.exec(l.getAttribute('sizes') || ''); return m ? +m[1] : (/apple/i.test(l.rel) ? 180 : 16); };
          // The smallest that is still sharp in a cell at twice its size.
          links.sort((a, b) => (size(a) >= 32 ? size(a) : 1000 - size(a)) - (size(b) >= 32 ? size(b) : 1000 - size(b)));
          return links.filter(l => !/svg/i.test(l.type || '') && !/\\.svg(\\?|$)/i.test(l.href)).map(l => l.href).slice(0, 3);
        })()
        """
        webView.evaluateJavaScript(script, in: nil, in: .defaultClient) { [weak self] result in
            var candidates: [URL] = []
            if case .success(let value) = result, let list = value as? [String] { candidates = list.compactMap(URL.init(string:)) }
            if let fallback = URL(string: "/favicon.ico", relativeTo: page)?.absoluteURL { candidates.append(fallback) }
            self?.fetch(candidates.filter { ["http", "https"].contains($0.scheme ?? "") }, site: site)
        }
    }

    private func fetch(_ candidates: [URL], site: String) {
        guard let url = candidates.first else { return }
        var request = URLRequest(url: url, timeoutInterval: 15)
        request.setValue("image/*", forHTTPHeaderField: "Accept")
        session.dataTask(with: request) { [weak self] data, response, _ in
            let ok = (response as? HTTPURLResponse)?.statusCode == 200
            guard ok, let data, data.count < 1_000_000, let image = Self.decode(data) else {
                DispatchQueue.main.async { self?.fetch(Array(candidates.dropFirst()), site: site) }
                return
            }
            DispatchQueue.main.async {
                guard let self else { return }
                try? data.write(to: self.file(site), options: .atomic)
                self.images[site] = image
                self.settled.remove(site)
                self.onChange?()
            }
        }.resume()
    }
}

/// Lies over the terminal and draws the icons on the cells the TUI names.
/// Clicks go through to the terminal.
final class IconStrip: NSView {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    /// `icons`: the frame of the cells an icon may fill, and the icon.
    func show(_ icons: [(NSRect, CGImage)], scale: CGFloat) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        layer?.sublayers?.forEach { $0.removeFromSuperlayer() }
        for (cells, image) in icons {
            let side = (min(cells.width, cells.height) - 2).rounded(.down)
            guard side >= 6 else { continue }
            let icon = CALayer()
            icon.contents = image
            icon.contentsGravity = .resizeAspect
            icon.contentsScale = scale
            icon.minificationFilter = .trilinear
            icon.cornerRadius = 2
            icon.masksToBounds = true
            icon.frame = NSRect(x: (cells.midX - side / 2).rounded(), y: (cells.midY - side / 2).rounded(), width: side, height: side)
            layer?.addSublayer(icon)
        }
        CATransaction.commit()
    }
}
