import WebKit

/// The keyboard's reach into the page: scrolling it, and following its links
/// by typed labels ("hints"), both without the page having the keyboard.
///
/// The script runs in a world of its own, so the page's script cannot see it
/// or stand in its way, and nothing is put in a page until one of these is
/// first used on it. Only the top frame is reached.
enum PageKeys {
    private static let world = WKContentWorld.world(name: "tilde-keys")

    private static func call(_ webView: WKWebView, _ body: String, _ arguments: [String: Any] = [:],
                             done: @escaping ([String: Any]) -> Void) {
        webView.callAsyncJavaScript(library + body, arguments: arguments, in: nil, in: world) { result in
            switch result {
            case .success(let value): done(value as? [String: Any] ?? [:])
            case .failure(let error):
                Log.write("page keys: \(error.localizedDescription)")
                done([:])
            }
        }
    }

    /// `how`: line-down, line-up, half-down, half-up, page-down, page-up, top, bottom.
    static func scroll(_ webView: WKWebView, how: String) {
        call(webView, "return tilde.scroll(how)", ["how": how]) { _ in }
    }

    /// Labels everything on screen that can be followed. Answers with how many there are.
    static func startHints(_ webView: WKWebView, done: @escaping (Int) -> Void) {
        call(webView, "return tilde.start()") { done($0["count"] as? Int ?? 0) }
    }

    /// Narrows the labels to those that begin with `typed`, and follows the
    /// one that is typed in full. The answer: `count` labels still match;
    /// `done` when one was followed; `url` when it is a link that was asked
    /// to be opened in a new tab; `focus` when it is a field to type in.
    static func typeHint(_ webView: WKWebView, typed: String, newTab: Bool, done: @escaping ([String: Any]) -> Void) {
        call(webView, "return tilde.type(typed, newTab)", ["typed": typed, "newTab": newTab], done: done)
    }

    static func stopHints(_ webView: WKWebView) {
        call(webView, "return tilde.stop()") { _ in }
    }

    private static let library = #"""
    const tilde = globalThis.__tilde || (globalThis.__tilde = (() => {
      const LETTERS = 'sadfjklewcmpgh';
      const WANTED = 'a[href],area[href],button,input:not([type=hidden]),textarea,select,summary,label[for],details>summary,' +
        '[role=button],[role=link],[role=tab],[role=menuitem],[role=menuitemcheckbox],[role=menuitemradio],[role=option],' +
        '[role=checkbox],[role=radio],[role=switch],[role=combobox],[role=textbox],[role=searchbox],[role=treeitem],' +
        '[onclick],[contenteditable=""],[contenteditable=true],[tabindex]:not([tabindex="-1"])';
      let host = null;
      let items = [];

      function scroller() {
        const root = document.scrollingElement || document.documentElement;
        // An application that scrolls a pane of its own: the pane under the
        // middle of the window, if it is a large part of it.
        let el = document.elementFromPoint(innerWidth / 2, innerHeight / 2);
        while (el && el !== root && el !== document.body) {
          const style = getComputedStyle(el);
          if (/(auto|scroll|overlay)/.test(style.overflowY) && el.scrollHeight > el.clientHeight + 1 &&
              el.clientHeight >= innerHeight * 0.4) return el;
          el = el.parentElement || (el.getRootNode() && el.getRootNode().host) || null;
        }
        return root;
      }

      function scroll(how) {
        const el = scroller();
        const view = el === document.scrollingElement ? innerHeight : el.clientHeight;
        const by = { 'line-down': 64, 'line-up': -64, 'half-down': view / 2, 'half-up': -view / 2,
                     'page-down': view * 0.9, 'page-up': -view * 0.9 }[how];
        if (how === 'top') el.scrollTo({ top: 0, behavior: 'instant' });
        else if (how === 'bottom') el.scrollTo({ top: el.scrollHeight, behavior: 'instant' });
        else if (by) el.scrollBy({ top: by, behavior: 'instant' });
        return {};
      }

      function collect(root, out) {
        for (const el of root.querySelectorAll(WANTED)) out.push(el);
        for (const el of root.querySelectorAll('*')) if (el.shadowRoot) collect(el.shadowRoot, out);
      }

      // Where the label of an element goes, or null when it cannot be reached.
      function place(el) {
        if (el.disabled || el.getAttribute('aria-disabled') === 'true') return null;
        for (const rect of el.getClientRects()) {
          if (rect.width < 3 || rect.height < 3) continue;
          if (rect.bottom <= 0 || rect.right <= 0 || rect.top >= innerHeight || rect.left >= innerWidth) continue;
          const style = getComputedStyle(el);
          if (style.visibility !== 'visible' || style.pointerEvents === 'none' || +style.opacity === 0) return null;
          const x = Math.min(Math.max(rect.left + Math.min(rect.width / 2, 12), 1), innerWidth - 1);
          const y = Math.min(Math.max(rect.top + Math.min(rect.height / 2, 10), 1), innerHeight - 1);
          const scope = el.getRootNode().elementFromPoint ? el.getRootNode() : document;
          const top = scope.elementFromPoint(x, y);
          // Covered by something that is no part of it: not on screen.
          if (top && top !== el && !el.contains(top) && !top.contains(el) && !(top.shadowRoot && top.shadowRoot.contains(el))) continue;
          return { left: Math.max(rect.left, 0), top: Math.max(rect.top, 0) };
        }
        return null;
      }

      function codes(count) {
        const base = LETTERS.length;
        let length = 1;
        while (Math.pow(base, length) < count) length++;
        const out = [];
        for (let i = 0; i < count; i++) {
          let code = '';
          for (let n = i, k = 0; k < length; k++, n = Math.floor(n / base)) code = LETTERS[n % base] + code;
          out.push(code);
        }
        return out;
      }

      function stop() {
        if (host) host.remove();
        host = null;
        items = [];
        return {};
      }

      function start() {
        stop();
        const found = [];
        collect(document, found);
        const seen = new Set();
        const placed = [];
        for (const el of found) {
          if (seen.has(el)) continue;
          seen.add(el);
          // What is inside a link or a button is reached by its label.
          const outer = el.parentElement && el.parentElement.closest('a[href],button');
          if (outer && seen.has(outer)) continue;
          const at = place(el);
          if (at) placed.push({ el, at });
        }
        if (!placed.length) return { count: 0 };
        host = document.createElement('div');
        host.style.cssText = 'all:initial;position:fixed;inset:0;z-index:2147483647;pointer-events:none';
        const shadow = host.attachShadow({ mode: 'closed' });
        const style = document.createElement('style');
        style.textContent = 'span{position:fixed;font:bold 11px/1.25 ui-monospace,Menlo,monospace;letter-spacing:.5px;' +
          'text-transform:uppercase;color:#1b1b1f;background:#ffd76e;border:1px solid #c79a2a;border-radius:3px;' +
          'padding:0 3px;box-shadow:0 1px 3px rgba(0,0,0,.35);white-space:nowrap}b{font-weight:bold;opacity:.38}';
        shadow.appendChild(style);
        const labels = codes(placed.length);
        items = placed.map(({ el, at }, i) => {
          const label = document.createElement('span');
          label.style.left = at.left + 'px';
          label.style.top = at.top + 'px';
          label.textContent = labels[i];
          shadow.appendChild(label);
          return { el, label, code: labels[i] };
        });
        document.documentElement.appendChild(host);
        return { count: items.length };
      }

      function follow(el, newTab) {
        stop();
        const link = el.closest('a[href],area[href]');
        if (newTab && link && /^https?:/.test(link.href)) return { done: true, count: 1, url: link.href };
        const tag = el.tagName;
        const typed = tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable ||
          (tag === 'INPUT' && !/^(button|submit|reset|checkbox|radio|file|image|color|range)$/i.test(el.type)) ||
          /^(textbox|searchbox|combobox)$/.test(el.getAttribute('role') || '');
        if (typed) {
          el.focus();
          return { done: true, count: 1, focus: true };
        }
        try { el.focus({ preventScroll: true }); } catch (e) {}
        el.click();
        return { done: true, count: 1 };
      }

      function type(typed, newTab) {
        typed = String(typed).toLowerCase();
        let count = 0;
        let exact = null;
        for (const item of items) {
          const matches = item.code.startsWith(typed);
          item.label.style.display = matches ? '' : 'none';
          if (!matches) continue;
          count++;
          if (item.code === typed) exact = item;
          item.label.textContent = '';
          if (typed) {
            const done = document.createElement('b');
            done.textContent = typed;
            item.label.appendChild(done);
          }
          item.label.appendChild(document.createTextNode(item.code.slice(typed.length)));
        }
        if (exact) return follow(exact.el, newTab);
        if (!count) stop();
        return { done: false, count };
      }

      return { scroll, start, type, stop };
    })());

    """#
}
