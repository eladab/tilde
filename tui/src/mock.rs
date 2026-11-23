//! An in-process stand-in for the app, for `--mock` and for tests.

use crate::resolve::{display_host, resolve};
use crate::transport::Transport;
use serde_json::{json, Value};

struct MockTab {
    id: u64,
    #[cfg_attr(not(test), allow(dead_code))]
    profile: String,
    history: Vec<String>,
    index: usize,
}

impl MockTab {
    fn url(&self) -> &str {
        self.history.get(self.index).map(String::as_str).unwrap_or("")
    }

    fn state(&self) -> Value {
        json!({
            "tab_id": self.id,
            "url": self.url(),
            "title": display_host(self.url()),
            "loading": false,
            "progress": 1.0,
            "can_go_back": self.index > 0,
            "can_go_forward": self.index + 1 < self.history.len(),
            "state": format!("state-{}", self.id),
        })
    }

    fn visit(&mut self, url: String) {
        if !self.history.is_empty() {
            self.history.truncate(self.index + 1);
        }
        self.history.push(url);
        self.index = self.history.len() - 1;
    }
}

#[derive(Default)]
pub struct MockEngine {
    /// The match the last search stopped on.
    find_index: u64,
    tabs: Vec<MockTab>,
    next_id: u64,
    pub active: Option<u64>,
    pub mode: &'static str,
    pub viewport: Option<(u64, u64, u64, u64)>,
    pub visible: bool,
    pub shut_down: bool,
    /// Every request received, for assertions.
    pub log: Vec<Value>,
    /// The tab whose requests the dev tools watch; the outer `None` is "not capturing".
    pub net_watch: Option<Option<u64>>,
    /// Tabs that had their page before capturing began.
    late: Vec<u64>,
    cookies: Vec<Value>,
    local: Vec<Value>,
    session: Vec<Value>,
    /// Tabs whose pages were let go, and the state names they were kept under.
    pub discarded: Vec<(u64, String)>,
    /// The cells drawn over the page, while there are any.
    pub overlay: Option<(u64, u64, u64, u64)>,
    pub icons: Vec<Value>,
    /// What has been typed of a label, while labels are up.
    pub hints: Option<String>,
    pub scrolled: Vec<String>,
    muted: Vec<u64>,
    /// Sites content blocking is switched off for.
    unblocked: Vec<String>,
    /// The state names every tab was made with.
    pub restored_from: Vec<String>,
}

/// The requests a page at `url` is pretended to have made, in full.
fn mock_requests(url: &str) -> Vec<Value> {
    let origin = {
        let host = url.split_once("://").map(|(_, r)| r).unwrap_or(url).split(['/', '?', '#']).next().unwrap_or("");
        format!("https://{host}")
    };
    let t0 = 1_800_000_000_000.0;
    let json_headers = json!([["content-type", "application/json; charset=utf-8"], ["cache-control", "no-store"], ["x-request-id", "7f3c2a9e41d84b0c"]]);
    let user = r#"{"id":4217,"name":"Ada Lovelace","email":"ada@example.com","roles":["admin","editor"],"active":true,"lastSeen":null,"plan":{"name":"team","seats":12}}"#;
    vec![
        json!({"id": 1, "method": "GET", "url": url, "status": 200, "status_text": "OK", "kind": "doc", "state": "done", "size": 18234, "body_size": 61250,
            "duration": 212.0, "started": t0, "mime": "text/html", "protocol": "h2", "detail": "document",
            "response_headers": [["content-type", "text/html; charset=utf-8"], ["content-encoding", "br"], ["server", "nginx"], ["strict-transport-security", "max-age=31536000; includeSubDomains"]],
            "request_headers": [["Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"]]}),
        json!({"id": 2, "method": "GET", "url": format!("{origin}/assets/site.css?v=42"), "status": 200, "kind": "css", "state": "done", "size": 9120, "body_size": 48211,
            "duration": 38.0, "started": t0 + 230.0, "protocol": "h2", "detail": "timing"}),
        json!({"id": 3, "method": "GET", "url": format!("{origin}/assets/app.js"), "status": 200, "kind": "js", "state": "done", "size": 84210, "body_size": 301224,
            "duration": 96.0, "started": t0 + 232.0, "protocol": "h2", "detail": "timing"}),
        json!({"id": 4, "method": "GET", "url": "https://fonts.example.net/inter/Inter-Regular.woff2", "status": 200, "kind": "font", "state": "done", "size": 46812,
            "duration": 61.0, "started": t0 + 280.0, "protocol": "h3", "detail": "timing"}),
        json!({"id": 14, "method": "GET", "url": format!("{origin}/images/logo.svg"), "kind": "img", "state": "done", "cached": true, "body_size": 4810,
            "duration": 0.0, "started": t0 + 290.0, "detail": "timing"}),
        json!({"id": 5, "method": "GET", "url": format!("{origin}/images/hero.webp"), "status": 200, "kind": "img", "state": "done", "size": 212044,
            "duration": 143.0, "started": t0 + 300.0, "protocol": "h2", "detail": "timing"}),
        json!({"id": 6, "method": "GET", "url": format!("{origin}/api/user/me"), "status": 200, "status_text": "OK", "kind": "fetch", "state": "done", "size": 412, "body_size": 163,
            "duration": 84.0, "started": t0 + 410.0, "mime": "application/json", "protocol": "h2", "detail": "full",
            "request_headers": [["Accept", "application/json"], ["Authorization", "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiI0MjE3In0.c2lnbmF0dXJl"]],
            "response_headers": json_headers, "response_body": user}),
        json!({"id": 7, "method": "POST", "url": format!("{origin}/api/search?q=terminal+browser&page=1&lang=en"), "status": 200, "status_text": "OK", "kind": "fetch", "state": "done",
            "size": 1893, "body_size": 5120, "duration": 187.0, "started": t0 + 640.0, "mime": "application/json", "protocol": "h2", "detail": "full",
            "request_headers": [["Content-Type", "application/json"], ["X-CSRF-Token", "b1946ac92492d2347c6235b4d2611184"]],
            "request_body": r#"{"query":"terminal browser","filters":{"lang":"en","safe":true},"limit":20}"#,
            "response_headers": json_headers,
            "response_body": r#"{"total":2,"took_ms":41,"results":[{"title":"Tilde","url":"https://github.com/eladab/tilde","score":0.98,"tags":["browser","tui"]},{"title":"WebKit","url":"https://webkit.org/","score":0.71,"tags":[]}],"next":null}"#}),
        json!({"id": 8, "method": "POST", "url": format!("{origin}/login"), "status": 302, "status_text": "Found", "kind": "xhr", "state": "done", "size": 380,
            "duration": 122.0, "started": t0 + 900.0, "protocol": "h2", "detail": "full",
            "request_headers": [["Content-Type", "application/x-www-form-urlencoded"]], "request_body": "user=ada%40example.com&remember=on&next=%2Fdashboard",
            "response_headers": [["location", "/dashboard"], ["set-cookie", "(not visible to scripts)"]]}),
        json!({"id": 9, "method": "GET", "url": format!("{origin}/api/notifications"), "status": 404, "status_text": "Not Found", "kind": "fetch", "state": "done", "size": 96,
            "duration": 45.0, "started": t0 + 1100.0, "mime": "application/json", "protocol": "h2", "detail": "full",
            "response_headers": json_headers, "response_body": r#"{"error":"not_found","message":"No such endpoint"}"#}),
        json!({"id": 10, "method": "GET", "url": "https://metrics.example.net/collect?e=pageview&t=1800000000", "status": 0, "kind": "fetch", "state": "failed",
            "started": t0 + 1300.0, "detail": "full", "error": "Load failed", "request_headers": []}),
        json!({"id": 11, "method": "GET", "url": format!("{origin}/api/export/report.csv"), "status": 500, "status_text": "Internal Server Error", "kind": "xhr", "state": "done",
            "size": 210, "duration": 2140.0, "started": t0 + 1500.0, "mime": "text/plain", "detail": "full",
            "response_headers": [["content-type", "text/plain"]], "response_body": "upstream timed out after 2000 ms\ntrace: 9d1f-22ab"}),
        json!({"id": 12, "method": "GET", "url": format!("wss://{}/live", origin.trim_start_matches("https://")), "status": 101, "status_text": "Switching Protocols", "kind": "ws",
            "state": "pending", "started": t0 + 1700.0, "detail": "full",
            "frames": [{"out": true, "data": r#"{"type":"subscribe","channel":"updates"}"#}, {"out": false, "data": r#"{"type":"ack","channel":"updates"}"#}, {"out": false, "data": r#"{"type":"tick","n":1}"#}]}),
        json!({"id": 13, "method": "GET", "url": format!("{origin}/api/feed?cursor=eyJvIjoyMH0"), "status": 0, "kind": "fetch", "state": "pending", "started": t0 + 2100.0, "detail": "full",
            "request_headers": [["Accept", "application/json"]]}),
    ]
}

/// The fields of a request that the list needs.
fn summary(request: &Value) -> Value {
    let mut out = json!({});
    for key in ["id", "method", "url", "status", "kind", "state", "size", "cached", "duration", "started", "mime", "error"] {
        if let Some(value) = request.get(key) {
            out[key] = value.clone();
        }
    }
    out
}

fn host_of(url: &str) -> String {
    display_host(url).split(':').next().unwrap_or("").to_string()
}

impl MockEngine {
    pub fn new() -> Self {
        let cookie = |name: &str, value: &str, domain: &str, expires: Value, http_only: bool| {
            json!({"name": name, "value": value, "domain": domain, "path": "/", "expires": expires, "secure": true, "http_only": http_only, "same_site": "lax"})
        };
        Self {
            next_id: 1,
            mode: "nav",
            visible: true,
            cookies: vec![
                cookie("session", "s%3Aq8Zk1mW3.pL0v%2BXc9", "webkit.org", Value::Null, true),
                cookie("theme", "dark", ".webkit.org", json!(1_900_000_000u64), false),
                cookie("prefs", "%7B%22lang%22%3A%22en%22%2C%22beta%22%3Atrue%7D", ".webkit.org", json!(1_890_000_000u64), false),
                cookie("_ga", "GA1.2.1234567890.1700000000", ".example.com", json!(1_860_000_000u64), false),
                cookie("sid", "f3a9c1d2e4b5", "accounts.example.com", json!(1_000_000_000u64), true),
                cookie("NID", "511=abcDEF123", ".search.example", json!(1_880_000_000u64), true),
            ],
            local: vec![
                json!({"key": "settings", "value": r#"{"theme":"dark","sidebar":{"width":280,"collapsed":false},"recent":["/docs","/blog"]}"#, "size": 86}),
                json!({"key": "onboarding.done", "value": "true", "size": 4}),
                json!({"key": "draft:42", "value": "Dev tools in a terminal: the page stays on the left, the requests on the right.", "size": 78}),
            ],
            session: vec![json!({"key": "scroll:/", "value": "1280", "size": 4}), json!({"key": "csrf", "value": "b1946ac92492d2347c6235b4d2611184", "size": 32})],
            ..Default::default()
        }
    }

    #[cfg(test)]
    pub fn tab_ids(&self) -> Vec<u64> {
        self.tabs.iter().map(|t| t.id).collect()
    }

    #[cfg(test)]
    pub fn url_of(&self, id: u64) -> Option<String> {
        self.tabs.iter().find(|t| t.id == id).map(|t| t.url().to_string())
    }

    #[cfg(test)]
    pub fn profile_of(&self, id: u64) -> Option<String> {
        self.tabs.iter().find(|t| t.id == id).map(|t| t.profile.clone())
    }

    /// Adds a tab as if the app already had it loaded (for re-adoption tests).
    #[cfg(test)]
    pub fn preload(&mut self, url: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(MockTab { id, profile: "default".into(), history: vec![url.to_string()], index: 0 });
        id
    }

    fn updated(tab: &MockTab) -> String {
        let mut state = tab.state();
        state["event"] = json!("tab.updated");
        state.to_string()
    }

    pub fn handle(&mut self, line: &str) -> Vec<String> {
        let Ok(req) = serde_json::from_str::<Value>(line) else {
            return vec![json!({"id": null, "ok": false, "error": "malformed request"}).to_string()];
        };
        self.log.push(req.clone());
        let id = req["id"].clone();
        let ok = |extra: Value| {
            let mut out = extra;
            out["id"] = id.clone();
            out["ok"] = json!(true);
            out.to_string()
        };
        let fail = |msg: &str| json!({"id": id.clone(), "ok": false, "error": msg}).to_string();
        let method = req["method"].as_str().unwrap_or("");
        let tab_index = req["tab_id"].as_u64().and_then(|t| self.tabs.iter().position(|x| x.id == t));

        let mut out = Vec::new();
        match method {
            "hello" => out.push(ok(json!({"app_version": "mock", "mode": self.mode, "instance": 1}))),
            "viewport.set" => {
                self.viewport = Some((
                    req["col"].as_u64().unwrap_or(0),
                    req["row"].as_u64().unwrap_or(0),
                    req["cols"].as_u64().unwrap_or(0),
                    req["rows"].as_u64().unwrap_or(0),
                ));
                out.push(ok(json!({})));
            }
            "viewport.visible" => {
                self.visible = req["visible"].as_bool().unwrap_or(true);
                out.push(ok(json!({})));
            }
            "tab.list" => {
                let tabs: Vec<Value> = self.tabs.iter().map(MockTab::state).collect();
                out.push(ok(json!({"tabs": tabs, "active": self.active})));
            }
            "tab.create" => {
                let url = match req["url"].as_str() {
                    Some(u) if !u.is_empty() => Some(u.to_string()),
                    _ => req["input"].as_str().and_then(resolve),
                };
                let tab_id = self.next_id;
                self.next_id += 1;
                let profile = req["profile"].as_str().unwrap_or("default").to_string();
                let mut tab = MockTab { id: tab_id, profile, history: vec![], index: 0 };
                if let Some(url) = url {
                    tab.visit(url);
                }
                if req["activate"].as_bool() == Some(true) {
                    self.active = Some(tab_id);
                }
                if let Some(state) = req["state"].as_str() {
                    self.restored_from.push(state.to_string());
                }
                out.push(ok(json!({"tab_id": tab_id})));
                out.push(Self::updated(&tab));
                self.tabs.push(tab);
            }
            "tab.close" | "tab.activate" | "tab.navigate" | "tab.back" | "tab.forward" | "tab.reload" | "tab.stop" => {
                let Some(i) = tab_index else {
                    return vec![fail("no such tab")];
                };
                match method {
                    "tab.close" => {
                        let closed = self.tabs.remove(i);
                        if self.active == Some(closed.id) {
                            self.active = None;
                        }
                        out.push(ok(json!({})));
                    }
                    "tab.activate" => {
                        self.active = Some(self.tabs[i].id);
                        out.push(ok(json!({})));
                    }
                    "tab.navigate" => match resolve(req["input"].as_str().unwrap_or("")) {
                        Some(url) => {
                            self.tabs[i].visit(url.clone());
                            out.push(ok(json!({"url": url})));
                            out.push(Self::updated(&self.tabs[i]));
                            if self.net_watch == Some(Some(self.tabs[i].id)) {
                                let requests: Vec<Value> = mock_requests(&url).iter().map(summary).collect();
                                let id = self.tabs[i].id;
                                self.late.retain(|t| *t != id);
                                out.push(json!({"event": "net.updated", "tab_id": id, "reset": true, "requests": requests}).to_string());
                            }
                        }
                        None => out.push(fail("empty input")),
                    },
                    "tab.back" => {
                        if self.tabs[i].index > 0 {
                            self.tabs[i].index -= 1;
                        }
                        out.push(ok(json!({})));
                        out.push(Self::updated(&self.tabs[i]));
                    }
                    "tab.forward" => {
                        if self.tabs[i].index + 1 < self.tabs[i].history.len() {
                            self.tabs[i].index += 1;
                        }
                        out.push(ok(json!({})));
                        out.push(Self::updated(&self.tabs[i]));
                    }
                    "tab.reload" => {
                        out.push(ok(json!({})));
                        // A page loaded while capturing is recorded from its start.
                        let id = self.tabs[i].id;
                        if self.net_watch.is_some() {
                            self.late.retain(|t| *t != id);
                        }
                        if self.net_watch == Some(Some(id)) {
                            let requests: Vec<Value> = mock_requests(self.tabs[i].url()).iter().map(summary).collect();
                            out.push(json!({"event": "net.updated", "tab_id": id, "reset": true, "late": false, "requests": requests}).to_string());
                        }
                    }
                    _ => out.push(ok(json!({}))),
                }
            }
            "focus.set" => {
                let wanted = if req["mode"] == "page" { "page" } else { "nav" };
                // Like the app, the page can only take focus when it is on screen.
                let granted = if wanted == "page" && (self.active.is_none() || !self.visible) { "nav" } else { wanted };
                out.push(ok(json!({"mode": granted})));
                if granted != self.mode {
                    self.mode = granted;
                    out.push(json!({"event": "focus.changed", "mode": granted}).to_string());
                }
            }
            "clipboard.set" | "state.keep" => out.push(ok(json!({}))),
            "tab.discard" => match tab_index {
                Some(i) => {
                    let gone = self.tabs.remove(i);
                    if self.active == Some(gone.id) {
                        self.active = None;
                    }
                    self.discarded.push((gone.id, format!("state-{}", gone.id)));
                    out.push(ok(json!({})));
                }
                None => out.push(fail("no such tab")),
            },
            "tab.mute" => match tab_index {
                Some(i) => {
                    let id = self.tabs[i].id;
                    let muted = req["muted"].as_bool() == Some(true);
                    self.muted.retain(|t| *t != id);
                    if muted {
                        self.muted.push(id);
                    }
                    out.push(ok(json!({"muted": muted})));
                    out.push(json!({"event": "tab.updated", "tab_id": id, "muted": muted}).to_string());
                }
                None => out.push(fail("no such tab")),
            },
            "page.scroll" => {
                self.scrolled.push(req["how"].as_str().unwrap_or("").to_string());
                out.push(ok(json!({})));
            }
            // Every page has three things to follow, labelled s, a and d:
            // a link, a field and a button.
            "hints.start" => {
                self.hints = Some(String::new());
                out.push(ok(json!({"count": 3})));
            }
            "hints.type" => {
                let typed = req["typed"].as_str().unwrap_or("").to_string();
                let new_tab = req["new_tab"].as_bool() == Some(true);
                let answer = match typed.as_str() {
                    "" => json!({"done": false, "count": 3}),
                    "s" if new_tab => json!({"done": true, "count": 1, "url": "https://linked.example/"}),
                    "s" | "d" => json!({"done": true, "count": 1}),
                    "a" => json!({"done": true, "count": 1, "focus": true}),
                    _ => json!({"done": false, "count": 0}),
                };
                self.hints = if answer["done"] == true || answer["count"] == 0 { None } else { Some(typed) };
                out.push(ok(answer));
            }
            "hints.stop" => {
                self.hints = None;
                out.push(ok(json!({})));
            }
            "overlay.set" => {
                self.overlay = Some((
                    req["col"].as_u64().unwrap_or(0),
                    req["row"].as_u64().unwrap_or(0),
                    req["cols"].as_u64().unwrap_or(0),
                    req["rows"].as_u64().unwrap_or(0),
                ));
                out.push(ok(json!({})));
            }
            "overlay.clear" => {
                self.overlay = None;
                out.push(ok(json!({})));
            }
            "icons.set" => {
                self.icons = req["icons"].as_array().cloned().unwrap_or_default();
                out.push(ok(json!({})));
            }
            "blocking.toggle" => match tab_index {
                Some(i) => {
                    let site = host_of(self.tabs[i].url());
                    let was_off = self.unblocked.contains(&site);
                    self.unblocked.retain(|s| *s != site);
                    if !was_off {
                        self.unblocked.push(site.clone());
                    }
                    out.push(ok(json!({"site": site, "active": was_off})));
                }
                None => out.push(fail("no such tab")),
            },
            "net.watch" => {
                let tab = req["tab_id"].as_u64();
                if self.net_watch.is_none() {
                    self.late = self.tabs.iter().map(|t| t.id).collect();
                }
                self.net_watch = Some(tab);
                let requests: Vec<Value> = tab
                    .and_then(|id| self.tabs.iter().find(|t| t.id == id))
                    .map(|t| mock_requests(t.url()).iter().map(summary).collect())
                    .unwrap_or_default();
                out.push(ok(json!({"requests": requests, "late": tab.is_some_and(|id| self.late.contains(&id))})));
            }
            "net.stop" => {
                self.net_watch = None;
                self.late.clear();
                out.push(ok(json!({})));
            }
            "net.get" => {
                let wanted = req["request_id"].as_u64();
                let found = tab_index.and_then(|i| mock_requests(self.tabs[i].url()).into_iter().find(|r| r["id"].as_u64() == wanted));
                match found {
                    Some(request) => out.push(ok(json!({"request": request}))),
                    None => out.push(fail("no such request")),
                }
            }
            "net.clear" | "net.preserve" => out.push(ok(json!({}))),
            "storage.get" => {
                let Some(i) = tab_index else {
                    return vec![fail("no such tab")];
                };
                let host = host_of(self.tabs[i].url());
                out.push(ok(json!({
                    "sites": [
                        {"name": "example.com", "kinds": ["Local Storage", "IndexedDB"]},
                        {"name": "search.example", "kinds": []},
                        {"name": host, "kinds": ["Local Storage", "Session Storage", "IndexedDB", "Cache Storage"]},
                    ],
                    "cookies": self.cookies,
                    "origin": format!("https://{host}"),
                    "site": host,
                    "local": self.local,
                    "session": self.session,
                    "databases": [{"name": "offline", "version": 3, "stores": [
                        {"name": "articles", "count": 128, "sample": [
                            {"key": "1", "value": r#"{"id":1,"title":"Hello","read":true}"#, "size": 36},
                            {"key": "2", "value": r#"{"id":2,"title":"Second post","read":false}"#, "size": 43}]},
                        {"name": "queue", "count": 0, "sample": []}]}],
                    "caches": [{"name": "static-v7", "count": 2, "urls": [format!("https://{host}/assets/app.js"), format!("https://{host}/assets/site.css")]}],
                })));
            }
            "storage.delete" => {
                let name = req["name"].as_str().unwrap_or("").to_string();
                let all = req["all"].as_bool() == Some(true);
                let site = req["site"].as_str().unwrap_or("").to_string();
                let in_site = |cookie: &Value| {
                    let domain = cookie["domain"].as_str().unwrap_or("").trim_start_matches('.').to_string();
                    domain == site || domain.ends_with(&format!(".{site}"))
                };
                match req["kind"].as_str().unwrap_or("") {
                    "cookie" | "site" if all => self.cookies.retain(|c| !in_site(c)),
                    "cookie" => self.cookies.retain(|c| !(c["name"] == name.as_str() && c["domain"] == req["domain"])),
                    "local" if all => self.local.clear(),
                    "local" => self.local.retain(|item| item["key"] != name.as_str()),
                    "session" if all => self.session.clear(),
                    "session" => self.session.retain(|item| item["key"] != name.as_str()),
                    _ => {}
                }
                out.push(ok(json!({})));
            }
            "find.set" => {
                // Every page has three matches of anything but "zzz".
                let count = if req["text"].as_str().is_some_and(|t| t.contains("zzz")) { 0 } else { 3 };
                self.find_index = match (count, req["fresh"] == true, req["backwards"] == true) {
                    (0, _, _) => 0,
                    (_, true, _) => self.find_index.max(1),
                    (_, _, false) => self.find_index % 3 + 1,
                    (_, _, true) => (self.find_index + 1) % 3 + 1,
                };
                out.push(ok(json!({})));
                out.push(json!({"event": "find.result", "tab_id": req["tab_id"], "text": req["text"], "count": count, "index": self.find_index}).to_string());
            }
            "find.close" => {
                self.find_index = 0;
                out.push(ok(json!({})));
            }
            "download.list" => out.push(ok(json!({"downloads": []}))),
            "download.open" | "download.reveal" | "download.cancel" | "download.remove" | "download.clear" => out.push(ok(json!({}))),
            "shutdown" => {
                self.shut_down = true;
                out.push(ok(json!({})));
            }
            _ => out.push(fail("unknown method")),
        }
        out
    }
}

impl Transport for MockEngine {
    fn send(&mut self, line: &str) -> Vec<String> {
        self.handle(line)
    }
}

/// Lets a test keep a handle on the engine that the app also owns.
impl Transport for std::rc::Rc<std::cell::RefCell<MockEngine>> {
    fn send(&mut self, line: &str) -> Vec<String> {
        self.borrow_mut().handle(line)
    }
}
