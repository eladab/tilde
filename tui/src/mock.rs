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
    tabs: Vec<MockTab>,
    next_id: u64,
    pub active: Option<u64>,
    pub mode: &'static str,
    pub viewport: Option<(u64, u64, u64, u64)>,
    pub visible: bool,
    pub shut_down: bool,
    /// Every request received, for assertions.
    pub log: Vec<Value>,
}

impl MockEngine {
    pub fn new() -> Self {
        Self { next_id: 1, mode: "nav", visible: true, ..Default::default() }
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
            "clipboard.set" => out.push(ok(json!({}))),
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
