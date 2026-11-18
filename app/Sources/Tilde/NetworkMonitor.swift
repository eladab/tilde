import Foundation
import WebKit
import TildeCore

/// Records the requests pages make, for the dev tools.
///
/// WebKit has no way to watch a web view's requests, so the record is put
/// together from what can be seen:
/// - a script in the page wraps `fetch`, `XMLHttpRequest` and `WebSocket`,
///   which gives those calls in full, bodies included;
/// - the same script reads WebKit's resource timings, which give every
///   other request's address, size and duration but nothing more;
/// - the navigation delegate gives each document's status and headers.
///
/// Nothing is put in any page until the dev tools are opened, and when they
/// are closed the pages get their own `fetch`, `XMLHttpRequest` and
/// `WebSocket` back.
///
/// A page that was already loaded when the dev tools opened is recorded only
/// in part: pages and their libraries keep their own reference to `fetch` as
/// they load, and calls made through it go around a wrapper put in place
/// afterwards. Such a tab is reported as `late` until it loads a page again.
final class NetworkMonitor: NSObject, WKScriptMessageHandler {
    private static let handlerName = "tildeNet"

    private unowned let store: TabStore
    private(set) var enabled = false
    /// Requests are kept when a tab goes to another page.
    var preserve = false
    /// The tab whose changes are reported.
    var watched: TabID?
    /// A tab's requests changed: the tab, whether they were started afresh,
    /// whether its page is only recorded in part, and the changed requests
    /// as the list shows them.
    var onUpdate: ((TabID, Bool, Bool, [[String: Any]]) -> Void)?

    private var logs: [TabID: NetLog] = [:]
    /// Present in the pages of every profile while recording, and only then.
    private let script = WKUserScript(source: NetworkMonitor.source, injectionTime: .atDocumentStart,
                                      forMainFrameOnly: false, in: .page)
    /// Tabs whose page was loaded before recording began.
    private var late = Set<TabID>()

    /// What a tab's page in the making is known by, until it has arrived.
    private struct Navigation {
        var method = "GET"
        var headers: [[String]] = []
        var started = Date()
        /// The request recorded for the document.
        var record: Int?
        /// The document's response has been seen, so the commit that follows is its own.
        var responded = false
    }
    private var navigations: [TabID: Navigation] = [:]
    /// Requests of frames, by address, until their responses arrive.
    private var frameRequests: [TabID: [String: Navigation]] = [:]
    /// Tabs about to show Tilde's own "can't open this page", which is not a page of its own.
    private var showingFailure = Set<TabID>()

    init(store: TabStore) {
        self.store = store
    }

    // MARK: On and off

    func start() {
        guard !enabled else { return }
        enabled = true
        for controller in store.userContentControllers { install(on: controller) }
        // Pages that are already loaded are watched from now on, as far as
        // that goes, and report what WebKit has measured of them so far.
        for tab in store.orderedTabs where tab.webView.url != nil {
            late.insert(tab.id)
            tab.webView.evaluateJavaScript(Self.source, completionHandler: nil)
        }
    }

    func stop() {
        guard enabled else { return }
        enabled = false
        watched = nil
        for controller in store.userContentControllers {
            guard controller.userScripts.contains(where: { $0 === script }) else { continue }
            let others = controller.userScripts.filter { $0 !== script }
            controller.removeAllUserScripts()
            others.forEach(controller.addUserScript)
            controller.removeScriptMessageHandler(forName: Self.handlerName, contentWorld: .page)
        }
        // Pages get their own functions back.
        for tab in store.orderedTabs {
            tab.webView.evaluateJavaScript("window.__tildeNet && window.__tildeNet.stop(), undefined", completionHandler: nil)
        }
        logs = [:]
        navigations = [:]
        frameRequests = [:]
        showingFailure = []
        late = []
    }

    /// Called for the pages of each profile, including profiles first used while recording.
    func install(on controller: WKUserContentController) {
        guard enabled, !controller.userScripts.contains(where: { $0 === script }) else { return }
        controller.addUserScript(script)
        controller.add(self, contentWorld: .page, name: Self.handlerName)
    }

    func tabClosed(_ id: TabID) {
        logs[id] = nil
        navigations[id] = nil
        frameRequests[id] = nil
        showingFailure.remove(id)
        late.remove(id)
        if watched == id { watched = nil }
    }

    // MARK: Reading

    private func log(_ id: TabID) -> NetLog {
        if let log = logs[id] { return log }
        let log = NetLog()
        logs[id] = log
        return log
    }

    func requests(of id: TabID) -> [[String: Any]] { logs[id]?.summaries ?? [] }

    /// Whether the tab's page was loaded before recording began.
    func isLate(_ id: TabID) -> Bool { late.contains(id) }

    func request(_ request: Int, of id: TabID) -> [String: Any]? { logs[id]?.full(request) }

    func clear(_ id: TabID) { logs[id]?.clear() }

    private func report(_ id: TabID, reset: Bool = false, _ changed: [Int]) {
        guard watched == id, let log = logs[id] else { return }
        let summaries = changed.compactMap(log.summary)
        if reset || !summaries.isEmpty { onUpdate?(id, reset, late.contains(id), summaries) }
    }

    // MARK: What the page's script reports

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard enabled, let webView = message.webView, let tab = store.tab(for: webView),
              let body = message.body as? [String: Any],
              let document = body["d"] as? String,
              let messages = body["m"] as? [[String: Any]] else { return }
        report(tab.id, log(tab.id).apply(document: document, messages: messages))
    }

    // MARK: What the navigation delegate sees

    private static func headers(_ fields: [AnyHashable: Any]?) -> [[String]] {
        (fields ?? [:]).compactMap { key, value in
            guard let key = key as? String else { return nil }
            return [key, "\(value)"]
        }.sorted { $0[0].lowercased() < $1[0].lowercased() }
    }

    /// A page or a frame is about to be asked for.
    func willNavigate(_ tab: Tab, action: WKNavigationAction) {
        guard enabled, let frame = action.targetFrame, let url = action.request.url,
              !showingFailure.contains(tab.id) else { return }
        let navigation = Navigation(method: action.request.httpMethod ?? "GET",
                                    headers: Self.headers(action.request.allHTTPHeaderFields))
        if frame.isMainFrame {
            navigations[tab.id] = navigation
        } else {
            var waiting = frameRequests[tab.id] ?? [:]
            if waiting.count > 50 { waiting = [:] }
            waiting[url.absoluteString] = navigation
            frameRequests[tab.id] = waiting
        }
    }

    /// A document's response has arrived. `shown` is false for one that is
    /// downloaded instead, which leaves the page as it is.
    func didRespond(_ tab: Tab, response: WKNavigationResponse, shown: Bool) {
        guard enabled, let url = response.response.url else { return }
        let main = response.isForMainFrame
        var navigation = (main ? navigations[tab.id] : frameRequests[tab.id]?.removeValue(forKey: url.absoluteString)) ?? Navigation()
        let log = log(tab.id)
        var reset = false
        // The page that comes now has the script from its start.
        if main && shown { late.remove(tab.id) }
        if main && shown && !preserve {
            log.reset()
            reset = true
        }
        var fields: [String: Any] = [
            "kind": "doc", "method": navigation.method, "url": url.absoluteString, "detail": "document",
            "started": navigation.started.timeIntervalSince1970 * 1000,
            "state": main && shown ? "pending" : "done",
            "request_headers": navigation.headers,
            "mime": response.response.mimeType ?? "",
        ]
        if let http = response.response as? HTTPURLResponse {
            fields["status"] = http.statusCode
            fields["response_headers"] = Self.headers(http.allHeaderFields)
        }
        if response.response.expectedContentLength > 0 {
            fields["body_size"] = Int(response.response.expectedContentLength)
        }
        if !shown { fields["response_note"] = "Saved as a download." }
        let id = log.add(fields)
        if main && shown {
            navigation.record = id
            navigation.responded = true
            navigations[tab.id] = navigation
        }
        report(tab.id, reset: reset, [id])
    }

    /// The tab is on its new page. One that arrived without a response of
    /// its own (out of the page cache, or made by script) starts the log afresh too.
    func didCommit(_ tab: Tab) {
        guard enabled else { return }
        // The failed request stays in the log under the page that reports it.
        if showingFailure.remove(tab.id) != nil { return }
        if navigations[tab.id]?.responded == true {
            navigations[tab.id]?.responded = false
            return
        }
        let log = log(tab.id)
        if !preserve { log.reset() }
        var navigation = navigations[tab.id] ?? Navigation()
        let url = tab.webView.url?.absoluteString ?? ""
        guard !url.isEmpty else { return report(tab.id, reset: !preserve, []) }
        let id = log.add([
            "kind": "doc", "method": navigation.method, "url": url, "detail": "document", "state": "pending",
            "started": navigation.started.timeIntervalSince1970 * 1000,
            "response_note": "Shown without asking the network: from the page cache, or made by the page itself.",
        ])
        navigation.record = id
        navigations[tab.id] = navigation
        report(tab.id, reset: !preserve, [id])
    }

    func didFinish(_ tab: Tab) {
        guard enabled, let navigation = navigations[tab.id], let id = navigation.record,
              log(tab.id).summary(id)?["state"] as? String == "pending" else { return }
        navigations[tab.id] = nil
        let log = log(tab.id)
        // The script's measurement, if it has come, is the better one.
        var fields: [String: Any] = ["state": "done"]
        if log.summary(id)?["duration"] == nil {
            fields["duration"] = -navigation.started.timeIntervalSinceNow * 1000
        }
        log.update(id, fields)
        report(tab.id, [id])
    }

    /// A page could not be loaded. With `errorPage`, the tab goes on to show
    /// Tilde's own page saying so.
    func didFail(_ tab: Tab, url: URL?, error: Error, errorPage: Bool) {
        guard enabled else { return }
        if errorPage { showingFailure.insert(tab.id) }
        let navigation = navigations.removeValue(forKey: tab.id) ?? Navigation()
        let log = log(tab.id)
        let failure: [String: Any] = ["state": "failed", "error": error.localizedDescription,
                                      "duration": -navigation.started.timeIntervalSinceNow * 1000]
        if let id = navigation.record {
            log.update(id, failure)
            return report(tab.id, [id])
        }
        guard let url else { return }
        // Nothing came back at all: the request is all there is to show.
        if !preserve { log.reset() }
        let id = log.add(failure.merging([
            "kind": "doc", "method": navigation.method, "url": url.absoluteString, "detail": "document",
            "started": navigation.started.timeIntervalSince1970 * 1000, "request_headers": navigation.headers,
        ]) { old, _ in old })
        report(tab.id, reset: !preserve, [id])
    }

    // MARK: The script

    /// Runs in each page at its start while recording, and once in the pages
    /// already there when recording begins. It posts batches of `{ i, p }`:
    /// a request's number in this document and fields to set on it.
    private static let source = #"""
    (() => {
      'use strict';
      if (window.__tildeNet) { window.__tildeNet.start(); return; }
      const handler = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.tildeNet;
      if (!handler) return;
      const post = handler.postMessage.bind(handler);
      const later = window.setTimeout.bind(window);
      const doc = Math.random().toString(36).slice(2) + Date.now().toString(36);
      const BODY = 512 * 1024;
      let on = true, seq = 0, queue = [], timer = 0;
      const now = () => performance.timeOrigin + performance.now();
      const flush = () => {
        timer = 0;
        if (!queue.length) return;
        const batch = queue;
        queue = [];
        try { post({ d: doc, m: batch }); } catch (e) {}
      };
      const push = (message) => {
        if (!on) return;
        queue.push(message);
        if (queue.length > 400) flush();
        else if (!timer) timer = later(flush, 60);
      };
      const send = (i, p) => push({ i, p });
      // Frames of the same origin are switched with their page; others
      // start with their next load.
      const frames = (name) => {
        for (let i = 0; i < window.frames.length; i++) {
          try { const hook = window.frames[i].__tildeNet; if (hook) hook[name](); } catch (e) {}
        }
      };
      // To a page that asks, the wrapped functions read as the ones they stand for.
      const disguise = (wrapper, native) => {
        try { Object.defineProperty(wrapper, 'toString', { value: () => Function.prototype.toString.call(native) }); } catch (e) {}
        return wrapper;
      };
      // What is wrapped: where it lives, its name there, the page's own and ours.
      const wrapped = [];
      const wrap = (owner, name, wrapper) => {
        const native = owner[name];
        disguise(wrapper, native);
        wrapped.push({ owner, name, native, wrapper });
        owner[name] = wrapper;
      };
      let replay = () => {}, forget = () => {};
      Object.defineProperty(window, '__tildeNet', { value: {
        start() {
          if (!on) {
            on = true;
            for (const w of wrapped) { try { if (w.owner[w.name] === w.native) w.owner[w.name] = w.wrapper; } catch (e) {} }
            replay();
          }
          frames('start');
        },
        // The page gets its own functions back, unless it has since put others in their place.
        stop() {
          on = false;
          queue = [];
          for (const w of wrapped) { try { if (w.owner[w.name] === w.wrapper) w.owner[w.name] = w.native; } catch (e) {} }
          forget();
          frames('stop');
        },
      } });
      addEventListener('pagehide', flush);

      const abs = (u) => { try { return new URL(String(u), document.baseURI).href; } catch (e) { return String(u); } };
      const textual = (type) => !type
        || /^(text\/|application\/(json|xml|javascript|x-javascript|ecmascript|x-www-form-urlencoded|graphql|x-ndjson))/i.test(type)
        || /^[^;]*\+(json|xml)/i.test(type);
      const clip = (text) => text.length > BODY ? text.slice(0, BODY) : text;
      const pairs = (headers) => { const out = []; try { headers.forEach((v, k) => out.push([k, v])); } catch (e) {} return out; };
      const mimeOf = (type) => (type || '').split(';')[0].trim();

      // Requests made from script, for matching with WebKit's timings.
      const recent = [];
      const track = (id, url, start) => { recent.push({ id, url, start, matched: false }); if (recent.length > 300) recent.shift(); };

      // What can be said of a body that is being sent.
      const describe = (id, body, patch) => {
        if (body == null) return;
        if (typeof body === 'string') {
          patch.request_body = clip(body);
          if (body.length > BODY) patch.request_note = 'Cut short: the whole body is ' + body.length + ' characters.';
        } else if (body instanceof URLSearchParams) {
          patch.request_body = clip(body.toString());
        } else if (typeof FormData !== 'undefined' && body instanceof FormData) {
          const lines = [];
          for (const [k, v] of body) lines.push(k + ': ' + (typeof v === 'string' ? v : '(file ' + v.name + ', ' + v.size + ' bytes)'));
          patch.request_body = clip(lines.join('\n'));
          patch.request_note = 'Sent as a multipart form; shown here as its fields.';
        } else if (typeof Blob !== 'undefined' && body instanceof Blob) {
          if (body.size <= BODY && textual(body.type)) {
            body.text().then((text) => send(id, { request_body: text }), () => {});
          } else {
            patch.request_note = 'Not text (' + (body.type || 'no type given') + ', ' + body.size + ' bytes), so not recorded.';
          }
        } else if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) {
          patch.request_note = 'Binary data (' + body.byteLength + ' bytes), so not recorded.';
        } else if (typeof ReadableStream !== 'undefined' && body instanceof ReadableStream) {
          patch.request_note = 'A stream, so not recorded.';
        } else {
          try { patch.request_body = clip(String(body)); } catch (e) {}
        }
      };

      // fetch
      const nativeFetch = window.fetch;
      if (nativeFetch) {
        const finish = (id, start, fields) => send(id, Object.assign({ state: 'done', duration: now() - start }, fields));
        const answered = (id, res, start, method) => {
          const type = res.headers.get('content-type') || '';
          send(id, { status: res.status, status_text: res.statusText, response_headers: pairs(res.headers), mime: mimeOf(type) });
          if (res.type === 'opaque' || res.type === 'opaqueredirect') {
            return finish(id, start, { response_note: 'An opaque response: the page asked for it without permission to read it, so Tilde cannot read it either.' });
          }
          if (!res.body || method === 'HEAD' || res.status === 204 || res.status === 304) return finish(id, start, {});
          if (/event-stream/i.test(type)) return finish(id, start, { response_note: 'A stream of events; they are not recorded.' });
          if (!textual(type)) return finish(id, start, { response_note: 'Not text (' + mimeOf(type) + '), so not recorded.' });
          let reader;
          try { reader = res.clone().body.getReader(); } catch (e) { return finish(id, start, {}); }
          const decoder = new TextDecoder();
          let text = '', size = 0;
          const step = () => reader.read().then(({ done, value }) => {
            if (done) return finish(id, start, { response_body: text + decoder.decode(), body_size: size });
            size += value.byteLength;
            text += decoder.decode(value, { stream: true });
            if (text.length > BODY) {
              // The page's own copy of the body is not affected.
              try { reader.cancel(); } catch (e) {}
              return finish(id, start, { response_body: text.slice(0, BODY), response_truncated: true });
            }
            return step();
          }).catch(() => finish(id, start, { response_body: text, response_truncated: text.length > 0 }));
          step();
        };
        wrap(window, 'fetch', function fetch(input, init) {
          if (!on) return nativeFetch.apply(this, arguments);
          let id = 0, method = 'GET';
          const start = now();
          try {
            const request = typeof Request !== 'undefined' && input instanceof Request ? input : null;
            const url = abs(request ? request.url : input);
            method = String((init && init.method) || (request && request.method) || 'GET').toUpperCase();
            const patch = { kind: 'fetch', method, url, started: start, state: 'pending', detail: 'full', request_headers: [] };
            const headers = (init && init.headers) || (request && request.headers);
            if (headers) { try { patch.request_headers = pairs(new Headers(headers)); } catch (e) {} }
            id = ++seq;
            if (init && init.body != null) {
              describe(id, init.body, patch);
            } else if (request && method !== 'GET' && method !== 'HEAD') {
              const type = request.headers.get('content-type') || '';
              if (textual(type)) {
                try { request.clone().text().then((text) => { if (text) send(id, { request_body: clip(text) }); }, () => {}); } catch (e) {}
              } else {
                patch.request_note = 'Not text (' + mimeOf(type) + '), so not recorded.';
              }
            }
            track(id, url, start);
            send(id, patch);
          } catch (e) { id = 0; }
          const promise = nativeFetch.apply(this, arguments);
          if (id) {
            promise.then(
              (res) => { try { answered(id, res, start, method); } catch (e) {} },
              (error) => send(id, { state: 'failed', error: String((error && error.message) || error), duration: now() - start }));
          }
          return promise;
        });
      }

      // XMLHttpRequest
      const XHR = window.XMLHttpRequest;
      if (XHR) {
        const known = new WeakMap();
        const open = XHR.prototype.open, sendBody = XHR.prototype.send, setHeader = XHR.prototype.setRequestHeader;
        wrap(XHR.prototype, 'open', function (method, url) {
          try { if (on) known.set(this, { method: String(method || 'GET').toUpperCase(), url: abs(url), headers: [] }); } catch (e) {}
          return open.apply(this, arguments);
        });
        wrap(XHR.prototype, 'setRequestHeader', function (name, value) {
          try { const meta = known.get(this); if (meta) meta.headers.push([String(name), String(value)]); } catch (e) {}
          return setHeader.apply(this, arguments);
        });
        wrap(XHR.prototype, 'send', function (body) {
          const meta = on && known.get(this);
          if (meta) {
            try {
              const id = ++seq, start = now(), xhr = this;
              const patch = { kind: 'xhr', method: meta.method, url: meta.url, started: start, state: 'pending', detail: 'full', request_headers: meta.headers.slice() };
              if (body != null && !(typeof Document !== 'undefined' && body instanceof Document)) describe(id, body, patch);
              track(id, meta.url, start);
              send(id, patch);
              let headed = false, why = '';
              const head = () => {
                if (headed || xhr.readyState < 2 || !xhr.status) return;
                headed = true;
                const headers = (xhr.getAllResponseHeaders() || '').trim().split(/[\r\n]+/).filter(Boolean).map((line) => {
                  const at = line.indexOf(':');
                  return at < 0 ? [line, ''] : [line.slice(0, at).trim(), line.slice(at + 1).trim()];
                });
                send(id, { status: xhr.status, status_text: xhr.statusText, response_headers: headers, mime: mimeOf(xhr.getResponseHeader('content-type')) });
              };
              xhr.addEventListener('readystatechange', () => { try { head(); } catch (e) {} });
              xhr.addEventListener('abort', () => { why = 'Cancelled by the page.'; });
              xhr.addEventListener('timeout', () => { why = 'Timed out.'; });
              xhr.addEventListener('error', () => { why = 'The request could not be completed.'; });
              xhr.addEventListener('loadend', () => {
                try {
                  head();
                  const done = { duration: now() - start };
                  if (why || !xhr.status) {
                    done.state = 'failed';
                    done.error = why || 'The request could not be completed.';
                    return send(id, done);
                  }
                  done.state = 'done';
                  const kind = xhr.responseType;
                  if (kind === '' || kind === 'text') {
                    if (textual(xhr.getResponseHeader('content-type'))) {
                      const text = xhr.responseText || '';
                      done.response_body = clip(text);
                      done.response_truncated = text.length > BODY;
                    } else {
                      done.response_note = 'Not text (' + mimeOf(xhr.getResponseHeader('content-type')) + '), so not recorded.';
                    }
                  } else if (kind === 'json') {
                    const text = JSON.stringify(xhr.response);
                    if (text) { done.response_body = clip(text); done.response_truncated = text.length > BODY; }
                  } else if (kind === 'document') {
                    done.response_note = 'Read by the page as a document; its text is not recorded.';
                  } else {
                    const data = xhr.response;
                    done.response_note = 'Binary data (' + ((data && (data.byteLength || data.size)) || 0) + ' bytes), so not recorded.';
                  }
                  send(id, done);
                } catch (e) {}
              });
            } catch (e) {}
          }
          return sendBody.apply(this, arguments);
        });
      }

      // WebSocket
      const NativeSocket = window.WebSocket;
      if (NativeSocket) {
        const Socket = function WebSocket(url, protocols) {
          const socket = protocols === undefined ? new NativeSocket(url) : new NativeSocket(url, protocols);
          if (on) {
            try {
              const id = ++seq, start = now();
              let failed = false;
              send(id, { kind: 'ws', method: 'GET', url: abs(url).replace(/^http/, 'ws'), started: start, state: 'pending', detail: 'full' });
              const frame = (out, data) => push({ i: id, f: { out, data: typeof data === 'string' ? data.slice(0, 4096) : '(binary, ' + ((data && (data.byteLength || data.size)) || 0) + ' bytes)' } });
              socket.addEventListener('open', () => send(id, { status: 101, status_text: 'Switching Protocols' }));
              socket.addEventListener('message', (event) => { try { frame(false, event.data); } catch (e) {} });
              socket.addEventListener('error', () => { failed = true; });
              socket.addEventListener('close', (event) => send(id, failed && !event.wasClean
                ? { state: 'failed', error: 'The connection failed.', duration: now() - start }
                : { state: 'done', duration: now() - start }));
              const nativeSend = socket.send;
              socket.send = function (data) {
                try { frame(true, data); } catch (e) {}
                return nativeSend.apply(this, arguments);
              };
            } catch (e) {}
          }
          return socket;
        };
        Socket.prototype = NativeSocket.prototype;
        for (const name of ['CONNECTING', 'OPEN', 'CLOSING', 'CLOSED']) Socket[name] = NativeSocket[name];
        wrap(window, 'WebSocket', Socket);
      }

      // Everything else: what WebKit measures of each request.
      const kindOf = (initiator, url) => {
        let ext = '';
        try { ext = (new URL(url).pathname.match(/\.([a-z0-9]+)$/i) || [])[1] || ''; } catch (e) {}
        ext = ext.toLowerCase();
        if (/^(m?js)$/.test(ext)) return 'js';
        if (ext === 'css') return 'css';
        if (/^(png|jpe?g|gif|webp|avif|svg|ico|bmp)$/.test(ext)) return 'img';
        if (/^(woff2?|ttf|otf|eot)$/.test(ext)) return 'font';
        if (/^(mp4|webm|mp3|m4a|m4s|ogg|wav|mov|m3u8|ts|aac)$/.test(ext)) return 'media';
        switch (initiator) {
          case 'script': return 'js';
          case 'link': return 'css';
          case 'img': case 'image': case 'input': return 'img';
          case 'video': case 'audio': case 'track': return 'media';
          case 'iframe': case 'frame': case 'navigation': return 'doc';
          case 'fetch': return 'fetch';
          case 'xmlhttprequest': return 'xhr';
          case 'beacon': case 'ping': return 'ping';
          default: return 'other';
        }
      };
      const measured = (entry, duration) => {
        const patch = { duration };
        if (entry.transferSize > 0) patch.size = entry.transferSize;
        // Nothing transferred for a body that exists, or no time taken at all: it came from the cache.
        else if (entry.decodedBodySize > 0 || duration === 0) patch.cached = true;
        if (entry.decodedBodySize > 0) patch.body_size = entry.decodedBodySize;
        if (entry.nextHopProtocol) patch.protocol = entry.nextHopProtocol;
        if (entry.responseStatus) patch.status = entry.responseStatus;
        return patch;
      };
      // Entries already reported, so that none is listed twice.
      const reported = new Set();
      const resource = (entry) => {
        const mark = entry.name + ' ' + entry.startTime;
        if (!on || reported.has(mark)) return;
        reported.add(mark);
        if (reported.size > 5000) reported.clear();
        const start = performance.timeOrigin + entry.startTime;
        const patch = measured(entry, entry.duration);
        if (entry.initiatorType === 'fetch' || entry.initiatorType === 'xmlhttprequest') {
          let best = null;
          for (const call of recent) {
            if (call.matched || call.url !== entry.name) continue;
            if (!best || Math.abs(call.start - start) < Math.abs(best.start - start)) best = call;
          }
          if (best && Math.abs(best.start - start) < 500) {
            best.matched = true;
            delete patch.status;
            return push({ i: best.id, p: patch, t: true });
          }
        }
        // How a call from script was made is not something its timing says.
        const scripted = entry.initiatorType === 'fetch' || entry.initiatorType === 'xmlhttprequest' || entry.initiatorType === 'beacon';
        Object.assign(patch, { kind: kindOf(entry.initiatorType, entry.name), method: scripted ? '' : 'GET', url: entry.name, started: start, state: 'done', detail: 'timing' });
        push({ i: ++seq, p: patch, t: true });
      };
      let pageReported = false;
      const page = (entry) => {
        if (!on || pageReported) return;
        pageReported = true;
        const patch = measured(entry, entry.responseEnd);
        Object.assign(patch, { kind: 'doc', method: 'GET', url: entry.name, started: performance.timeOrigin, state: 'done', detail: 'timing' });
        push({ i: ++seq, p: patch, t: true });
      };
      // The page's own timing is only complete once it has loaded.
      const reportPage = () => {
        try {
          const entry = performance.getEntriesByType('navigation')[0];
          if (entry && entry.responseEnd > 0) page(entry);
        } catch (e) {}
      };
      if (document.readyState === 'complete') reportPage();
      else addEventListener('load', () => later(reportPage, 0), { once: true });
      // Switched on in a page that is already there: what it has fetched so far.
      forget = () => { reported.clear(); pageReported = false; };
      replay = () => {
        if (document.readyState === 'complete') reportPage();
        try { performance.getEntriesByType('resource').forEach(resource); } catch (e) {}
      };
      try { new PerformanceObserver((list) => list.getEntries().forEach(resource)).observe({ type: 'resource', buffered: true }); } catch (e) {}
    })();
    """#
}
