import WebKit

/// Find in page, done by WebKit: it counts the matches, marks them all and
/// points out the current one, in web pages and in PDFs alike.
///
/// WebKit's public `find` says only whether a match exists. The count and the
/// marks come from the interface Safari itself uses, which is not public, so
/// it is called by name and only if it is there.
extension TabStore {
    private struct Options: OptionSet {
        let rawValue: UInt
        static let caseInsensitive = Options(rawValue: 1 << 0)
        static let backwards = Options(rawValue: 1 << 3)
        static let wrapAround = Options(rawValue: 1 << 4)
        static let showFindIndicator = Options(rawValue: 1 << 6)
        static let showHighlight = Options(rawValue: 1 << 7)
        static let determineMatchIndex = Options(rawValue: 1 << 9)
    }

    /// More matches than this are not counted.
    static let findLimit = 1000

    private static let findString = NSSelectorFromString("_findString:options:maxCount:")
    private static let hideFindUI = NSSelectorFromString("_hideFindUI")
    private static let setFindDelegate = NSSelectorFromString("_setFindDelegate:")

    /// Makes the tab report what its searches find.
    func watchFinds(in webView: WKWebView) {
        if webView.responds(to: Self.setFindDelegate) {
            webView.perform(Self.setFindDelegate, with: self)
        }
    }

    /// Goes to the next match of `text`, or the previous one. With `fresh`
    /// the text has changed, and the search starts over from the first match.
    func find(_ tab: Tab, text: String, backwards: Bool, fresh: Bool) {
        let webView = tab.webView
        guard !text.isEmpty else { return endFind(tab) }
        guard webView.responds(to: Self.findString), let method = webView.method(for: Self.findString) else {
            // Without the count, at least go to the match.
            let configuration = WKFindConfiguration()
            configuration.backwards = backwards
            configuration.caseSensitive = false
            configuration.wraps = true
            let id = tab.id
            webView.find(text, configuration: configuration) { [weak self] result in
                self?.onFind?(id, text, result.matchFound ? -1 : 0, 0)
            }
            return
        }
        var options: Options = [.caseInsensitive, .wrapAround, .showHighlight, .showFindIndicator, .determineMatchIndex]
        if backwards { options.insert(.backwards) }
        typealias Find = @convention(c) (AnyObject, Selector, NSString, UInt, UInt) -> Void
        let call = unsafeBitCast(method, to: Find.self)
        let run = { call(webView, Self.findString, text as NSString, options.rawValue, UInt(Self.findLimit)) }
        if fresh {
            // WebKit keeps its place in the matches by counting steps, which
            // means nothing once the text is another. A new text is searched
            // for from the top of the page, where the place is known: first.
            // Typing sends one text after another; only the last is searched for.
            endFind(tab)
            tab.findGeneration += 1
            let generation = tab.findGeneration
            webView.evaluateJavaScript("try { getSelection().removeAllRanges() } catch (e) {}") { [weak tab] _, _ in
                if tab?.findGeneration == generation { run() }
            }
        } else {
            run()
        }
    }

    /// Takes the marks of a search off the page.
    func endFind(_ tab: Tab) {
        if tab.webView.responds(to: Self.hideFindUI) {
            tab.webView.perform(Self.hideFindUI)
        }
    }

    @objc(_webView:didFindMatches:forString:withMatchIndex:)
    func webView(_ webView: WKWebView, didFindMatches matches: UInt, forString string: String, withMatchIndex index: Int) {
        guard let tab = tab(for: webView) else { return }
        // WebKit says "more than it counted" with the largest number there is.
        let count = matches > UInt(Self.findLimit) ? Self.findLimit : Int(matches)
        onFind?(tab.id, string, count, index >= 0 ? index + 1 : 0)
    }

    @objc(_webView:didFailToFindString:)
    func webView(_ webView: WKWebView, didFailToFindString string: String) {
        guard let tab = tab(for: webView) else { return }
        onFind?(tab.id, string, 0, 0)
    }
}
