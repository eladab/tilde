import Foundation
import WebKit
import TildeCore

/// Reads and deletes what sites store, for the dev tools.
///
/// Cookies and the list of sites come from the profile's data store, which
/// knows every site. Local and session storage, IndexedDB and Cache Storage
/// can only be read by script running in a page of their origin, so they
/// are read for the page the tab is on and for no other.
enum StorageInspector {
    /// Page scripts cannot see this world, so they cannot interfere with the reading.
    private static let world = WKContentWorld.world(name: "tilde-devtools")
    private static let timeout: TimeInterval = 5

    /// What WebKit calls each kind of stored data, in words. Cookies are
    /// counted separately; HTTP caches are not something a site "stores".
    private static let kindNames: [(String, String)] = [
        (WKWebsiteDataTypeLocalStorage, "Local Storage"),
        (WKWebsiteDataTypeSessionStorage, "Session Storage"),
        (WKWebsiteDataTypeIndexedDBDatabases, "IndexedDB"),
        (WKWebsiteDataTypeFetchCache, "Cache Storage"),
        (WKWebsiteDataTypeServiceWorkerRegistrations, "Service Workers"),
        (WKWebsiteDataTypeWebSQLDatabases, "WebSQL"),
        (WKWebsiteDataTypeFileSystem, "File System"),
        (WKWebsiteDataTypeMediaKeys, "Media Keys"),
    ]
    private static var storedTypes: Set<String> {
        Set(kindNames.map(\.0)).union([WKWebsiteDataTypeCookies])
    }

    private static func wire(_ cookie: HTTPCookie) -> [String: Any] {
        var out: [String: Any] = [
            "name": cookie.name,
            "value": String(cookie.value.prefix(16_384)),
            "domain": cookie.domain,
            "path": cookie.path,
            "secure": cookie.isSecure,
            "http_only": cookie.isHTTPOnly,
            "same_site": cookie.sameSitePolicy?.rawValue ?? "",
        ]
        if let expires = cookie.expiresDate, !cookie.isSessionOnly {
            out["expires"] = expires.timeIntervalSince1970
        }
        return out
    }

    /// Runs a script in the tab's page, out of the page's sight. `done` is
    /// called once, with nothing if the page did not answer in time.
    private static func run(_ script: String, arguments: [String: Any] = [:], in web: WKWebView, done: @escaping (Any?) -> Void) {
        var finished = false
        let finish: (Any?) -> Void = { value in
            guard !finished else { return }
            finished = true
            done(value)
        }
        guard let scheme = web.url?.scheme?.lowercased(), ["http", "https", "file"].contains(scheme) else { return finish(nil) }
        web.callAsyncJavaScript(script, arguments: arguments, in: nil, in: world) { result in
            if case .success(let value) = result { finish(value) } else { finish(nil) }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + timeout) { finish(nil) }
    }

    // MARK: Reading

    /// Everything `storage.get` answers with.
    static func read(_ tab: Tab, done: @escaping ([String: Any]) -> Void) {
        let web = tab.webView
        let store = web.configuration.websiteDataStore
        let group = DispatchGroup()
        var cookies: [HTTPCookie] = []
        var records: [WKWebsiteDataRecord] = []
        var page: [String: Any] = [:]

        group.enter()
        store.httpCookieStore.getAllCookies { all in
            cookies = all
            group.leave()
        }
        group.enter()
        store.fetchDataRecords(ofTypes: storedTypes) { all in
            records = all
            group.leave()
        }
        group.enter()
        run(readScript, in: web) { value in
            page = value.flatMap(Wire.jsonSafe) as? [String: Any] ?? [:]
            group.leave()
        }
        group.notify(queue: .main) {
            let names = records.map(\.displayName)
            let host = web.url?.host ?? ""
            var out: [String: Any] = [
                "sites": records.map { record in
                    ["name": record.displayName.lowercased(),
                     "kinds": kindNames.filter { record.dataTypes.contains($0.0) }.map(\.1)] as [String: Any]
                },
                "cookies": cookies.map(wire),
                "origin": page["origin"] as? String ?? "",
                "site": host.isEmpty ? "" : SiteMatch.site(for: host, among: names),
            ]
            for key in ["local", "session", "databases", "caches"] {
                out[key] = page[key] as? [Any] ?? []
            }
            done(out)
        }
    }

    /// Reads the storages of the page's origin. Values are cut to a size
    /// that is useful to look at; `size` says how long each really is.
    private static let readScript = #"""
    const CUT = 65536;
    const item = (key, value) => ({ key: String(key), value: value.length > CUT ? value.slice(0, CUT) : value, size: value.length });
    const dump = (name) => {
      const out = [];
      try {
        const storage = window[name];
        for (let i = 0; i < storage.length && i < 2000; i++) {
          const key = storage.key(i);
          out.push(item(key, String(storage.getItem(key))));
        }
      } catch (e) {}
      return out;
    };
    const show = (value) => {
      try {
        const text = JSON.stringify(value, (k, v) => {
          if (typeof v === 'bigint') return v.toString() + 'n';
          if (v instanceof ArrayBuffer || ArrayBuffer.isView(v)) return '(binary, ' + v.byteLength + ' bytes)';
          if (typeof Blob !== 'undefined' && v instanceof Blob) return '(blob, ' + v.size + ' bytes)';
          return v;
        });
        return text === undefined ? String(value) : text;
      } catch (e) { return String(value); }
    };
    const within = (promise, ms, fallback) => Promise.race([promise.catch(() => fallback), new Promise((done) => setTimeout(() => done(fallback), ms))]);
    const asked = (request) => new Promise((done, fail) => { request.onsuccess = () => done(request.result); request.onerror = () => fail(request.error); });

    const databases = async () => {
      if (!self.indexedDB || !indexedDB.databases) return [];
      const out = [];
      for (const info of (await indexedDB.databases()).slice(0, 30)) {
        const entry = { name: String(info.name), version: info.version || 0, stores: [] };
        try {
          const open = indexedDB.open(info.name);
          open.onblocked = () => {};
          const db = await asked(open);
          // Never stand in the way of the page upgrading its own database.
          db.onversionchange = () => db.close();
          try {
            for (const name of Array.from(db.objectStoreNames).slice(0, 50)) {
              const store = db.transaction(name, 'readonly').objectStore(name);
              const [count, keys, values] = await Promise.all([asked(store.count()), asked(store.getAllKeys(null, 25)), asked(store.getAll(null, 25))]);
              entry.stores.push({ name, count, sample: keys.map((key, i) => item(show(key), show(values[i]))) });
            }
          } finally { db.close(); }
        } catch (e) {}
        out.push(entry);
      }
      return out;
    };
    const cacheList = async () => {
      if (!self.caches) return [];
      const out = [];
      for (const name of (await caches.keys()).slice(0, 50)) {
        const keys = await (await caches.open(name)).keys();
        out.push({ name, count: keys.length, urls: keys.slice(0, 100).map((request) => request.url) });
      }
      return out;
    };
    const [stored, cached] = await Promise.all([within(databases(), 3000, []), within(cacheList(), 3000, [])]);
    return {
      origin: location.origin === 'null' ? '' : location.origin,
      local: dump('localStorage'),
      session: dump('sessionStorage'),
      databases: stored,
      caches: cached,
    };
    """#

    // MARK: Deleting

    private static let deleteScript = #"""
    const within = (promise, ms) => Promise.race([promise.catch(() => {}), new Promise((done) => setTimeout(done, ms))]);
    const dropped = (name) => new Promise((done) => {
      const request = indexedDB.deleteDatabase(name);
      request.onsuccess = request.onerror = request.onblocked = () => done();
    });
    if (kind === 'local' || kind === 'session') {
      const storage = kind === 'local' ? localStorage : sessionStorage;
      if (all) storage.clear(); else storage.removeItem(name);
    } else if (kind === 'indexeddb') {
      const names = all ? (await indexedDB.databases()).map((info) => info.name) : [name];
      await within(Promise.all(names.map(dropped)), 3000);
    } else if (kind === 'cache') {
      const names = all ? await caches.keys() : [name];
      await within(Promise.all(names.map((each) => caches.delete(each))), 3000);
    }
    return true;
    """#

    /// Carries out `storage.delete`. `done` receives what went wrong, if anything did.
    static func delete(_ tab: Tab, request: Wire.Request, done: @escaping (String?) -> Void) {
        let web = tab.webView
        let store = web.configuration.websiteDataStore
        let kind = request.string("kind") ?? ""
        let all = request.bool("all") ?? false
        let name = request.string("name") ?? ""
        let site = request.string("site") ?? ""
        if all ? (site.isEmpty && kind != "local" && kind != "session") : name.isEmpty {
            return done(all ? "missing site" : "missing name")
        }

        func deleteCookies(_ doomed: @escaping (HTTPCookie) -> Bool, then: @escaping () -> Void) {
            store.httpCookieStore.getAllCookies { cookies in
                let group = DispatchGroup()
                for cookie in cookies where doomed(cookie) {
                    group.enter()
                    store.httpCookieStore.delete(cookie) { group.leave() }
                }
                group.notify(queue: .main, execute: then)
            }
        }

        switch kind {
        case "cookie" where all:
            deleteCookies({ SiteMatch.belongs($0.domain, to: site) }) { done(nil) }
        case "cookie":
            let domain = request.string("domain"), path = request.string("path")
            deleteCookies({ $0.name == name && (domain == nil || $0.domain == domain) && (path == nil || $0.path == path) }) { done(nil) }
        case "local", "session", "indexeddb", "cache":
            run(deleteScript, arguments: ["kind": kind, "name": name, "all": all], in: web) { value in
                done(value == nil ? "The page did not answer." : nil)
            }
        case "site":
            store.fetchDataRecords(ofTypes: WKWebsiteDataStore.allWebsiteDataTypes()) { records in
                let doomed = records.filter { $0.displayName.lowercased() == site.lowercased() }
                store.removeData(ofTypes: WKWebsiteDataStore.allWebsiteDataTypes(), for: doomed) {
                    // Cookies of its subdomains are the site's too.
                    deleteCookies({ SiteMatch.belongs($0.domain, to: site) }) { done(nil) }
                }
            }
        default:
            done("unknown kind \(kind)")
        }
    }
}
