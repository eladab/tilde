//! Protocol v1: newline-delimited JSON between the TUI and the app.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Nav,
    Page,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "method")]
pub enum Request {
    #[serde(rename = "hello")]
    Hello { protocol: u32 },
    #[serde(rename = "viewport.set")]
    ViewportSet { col: u16, row: u16, cols: u16, rows: u16 },
    #[serde(rename = "viewport.visible")]
    ViewportVisible { visible: bool },
    #[serde(rename = "tab.list")]
    TabList,
    #[serde(rename = "tab.create")]
    TabCreate {
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<String>,
        activate: bool,
        /// Which cookie jar the tab uses; absent means the default one.
        #[serde(skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
    },
    #[serde(rename = "tab.close")]
    TabClose { tab_id: u64 },
    #[serde(rename = "tab.activate")]
    TabActivate { tab_id: u64 },
    #[serde(rename = "tab.navigate")]
    TabNavigate { tab_id: u64, input: String },
    #[serde(rename = "tab.back")]
    TabBack { tab_id: u64 },
    #[serde(rename = "tab.forward")]
    TabForward { tab_id: u64 },
    #[serde(rename = "tab.reload")]
    TabReload { tab_id: u64 },
    #[serde(rename = "tab.stop")]
    TabStop { tab_id: u64 },
    #[serde(rename = "focus.set")]
    FocusSet { mode: Mode },
    #[serde(rename = "download.list")]
    DownloadList,
    /// Open the downloaded file.
    #[serde(rename = "download.open")]
    DownloadOpen { download_id: u64 },
    /// Show the downloaded file in the Finder.
    #[serde(rename = "download.reveal")]
    DownloadReveal { download_id: u64 },
    #[serde(rename = "download.cancel")]
    DownloadCancel { download_id: u64 },
    /// Take a download off the list, stopping it first if it is running.
    #[serde(rename = "download.remove")]
    DownloadRemove { download_id: u64 },
    /// Take every download that is not running off the list.
    #[serde(rename = "download.clear")]
    DownloadClear,
    /// Put text on the system clipboard.
    #[serde(rename = "clipboard.set")]
    ClipboardSet { text: String },
    /// Start capturing requests in every tab, and report those of one tab
    /// (or of none, while the tab that is showing has no page).
    #[serde(rename = "net.watch")]
    NetWatch {
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<u64>,
    },
    /// Stop capturing and forget what was captured.
    #[serde(rename = "net.stop")]
    NetStop,
    /// Everything known about one request: headers and bodies.
    #[serde(rename = "net.get")]
    NetGet { tab_id: u64, request_id: u64 },
    #[serde(rename = "net.clear")]
    NetClear { tab_id: u64 },
    /// Whether a tab's requests are kept when it goes to another page.
    #[serde(rename = "net.preserve")]
    NetPreserve { enabled: bool },
    /// What the tab's profile stores: every site, every cookie, and the
    /// storages of the page the tab is on.
    #[serde(rename = "storage.get")]
    StorageGet { tab_id: u64 },
    /// Deletes one item (`cookie`, `local`, `session`, `indexeddb`, `cache`),
    /// every item of that kind with `all`, or everything a `site` stores.
    #[serde(rename = "storage.delete")]
    StorageDelete {
        tab_id: u64,
        kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        site: Option<String>,
        /// A cookie's name, a storage key, or a database's or cache's name.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        domain: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Search the tab's page for `text` and go to the next match, or the
    /// previous one. With `fresh` the text has changed, and the match the
    /// page is on is kept if it still matches. The answer is `find.result`.
    #[serde(rename = "find.set")]
    FindSet { tab_id: u64, text: String, backwards: bool, fresh: bool },
    /// Take the marks of the search off the tab's page.
    #[serde(rename = "find.close")]
    FindClose { tab_id: u64 },
    #[serde(rename = "shutdown")]
    Shutdown,
}

pub fn encode(id: u64, request: &Request) -> String {
    let mut value = serde_json::to_value(request).expect("request serialises");
    value
        .as_object_mut()
        .expect("request is an object")
        .insert("id".into(), Value::from(id));
    value.to_string()
}

/// Web state of one tab as the app reports it. Every field but the id is
/// optional so the same type reads `tab.updated` events and `tab.list` rows.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct TabState {
    pub tab_id: u64,
    pub url: Option<String>,
    pub title: Option<String>,
    pub loading: Option<bool>,
    pub progress: Option<f64>,
    pub can_go_back: Option<bool>,
    pub can_go_forward: Option<bool>,
    /// A number that changes when the tab arrives at another page, and stays
    /// when the page it is on is replaced (a redirect, a rewritten address).
    /// Zero until the first page has arrived.
    pub entry: Option<u64>,
    /// The address of the page the tab is on; `url` runs ahead while loading.
    pub entry_url: Option<String>,
    /// The page holds the camera (muted or not).
    pub camera: Option<bool>,
    /// The page holds the microphone (muted or not).
    pub microphone: Option<bool>,
}

/// A download, running or past, as the app reports it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Download {
    pub download_id: u64,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub path: String,
    /// `active`, `done`, `failed` or `cancelled`.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub received: u64,
    /// Zero while the size is not known.
    #[serde(default)]
    pub total: u64,
    /// Seconds since 1970.
    #[serde(default)]
    pub started: u64,
    pub error: Option<String>,
}

impl Download {
    pub fn is_active(&self) -> bool {
        self.state == "active"
    }
}

/// One request of a page, as the list shows it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct NetRequest {
    pub id: u64,
    pub method: String,
    pub url: String,
    /// Zero while there is no answer, and for answers WebKit does not describe.
    pub status: u16,
    /// `doc`, `fetch`, `xhr`, `js`, `css`, `img`, `font`, `media`, `ws`, `ping` or `other`.
    pub kind: String,
    /// `pending`, `done` or `failed`.
    pub state: String,
    /// Bytes on the wire, when known.
    pub size: Option<u64>,
    /// It was answered from the cache, with nothing on the wire.
    pub cached: bool,
    /// Milliseconds, when known.
    pub duration: Option<f64>,
    /// Milliseconds since 1970.
    pub started: f64,
    pub mime: String,
    pub error: Option<String>,
}

/// A message on a WebSocket.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct NetFrame {
    /// Sent by the page, as opposed to received.
    pub out: bool,
    pub data: String,
}

/// Everything known about one request.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct NetRecord {
    #[serde(flatten)]
    pub request: NetRequest,
    pub status_text: String,
    pub protocol: String,
    /// Size of the body once decoded, when known.
    pub body_size: Option<u64>,
    /// How much is known: `full` (seen by the page's own script call),
    /// `document` (seen by the app) or `timing` (only what WebKit measures).
    pub detail: String,
    pub request_headers: Vec<(String, String)>,
    pub request_body: Option<String>,
    /// Said in place of a body that is not text.
    pub request_note: Option<String>,
    pub response_headers: Vec<(String, String)>,
    pub response_body: Option<String>,
    pub response_note: Option<String>,
    pub response_truncated: bool,
    pub frames: Vec<NetFrame>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    /// Seconds since 1970; absent for a cookie that ends with the session.
    pub expires: Option<f64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: String,
}

/// A key of local or session storage.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct StorageItem {
    pub key: String,
    /// Cut short when `size` says it is longer.
    pub value: String,
    /// Length of the whole value, in characters.
    pub size: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ObjectStore {
    pub name: String,
    pub count: u64,
    /// The first records, as JSON text.
    pub sample: Vec<StorageItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Database {
    pub name: String,
    pub version: u64,
    pub stores: Vec<ObjectStore>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct CacheInfo {
    pub name: String,
    pub count: u64,
    /// Addresses of the first entries.
    pub urls: Vec<String>,
}

/// A site that has data in the profile.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Site {
    /// The registrable domain, as WebKit groups data: `example.com`.
    pub name: String,
    /// What it stores besides cookies, in words: "Local Storage", "IndexedDB".
    pub kinds: Vec<String>,
}

/// The reply to `storage.get`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct StorageData {
    pub sites: Vec<Site>,
    /// Every cookie of the profile.
    pub cookies: Vec<Cookie>,
    /// The origin of the tab's page; empty when it has none.
    pub origin: String,
    /// The site (as in `sites`) the tab's page belongs to.
    pub site: String,
    pub local: Vec<StorageItem>,
    pub session: Vec<StorageItem>,
    pub databases: Vec<Database>,
    pub caches: Vec<CacheInfo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "event")]
pub enum Event {
    #[serde(rename = "tab.created")]
    TabCreated {
        tab_id: u64,
        opener_tab_id: Option<u64>,
        #[serde(default)]
        url: String,
        #[serde(default)]
        background: bool,
    },
    #[serde(rename = "tab.updated")]
    TabUpdated(TabState),
    #[serde(rename = "tab.closed")]
    TabClosed { tab_id: u64 },
    #[serde(rename = "tab.crashed")]
    TabCrashed { tab_id: u64 },
    #[serde(rename = "focus.changed")]
    FocusChanged { mode: Mode },
    /// One download started, progressed or ended.
    #[serde(rename = "download.updated")]
    DownloadUpdated(Download),
    /// Downloads were taken off the list; this is the whole list, oldest first.
    #[serde(rename = "downloads.changed")]
    DownloadsChanged { downloads: Vec<Download> },
    /// Another app asked for an address to be opened (a clicked link).
    #[serde(rename = "open-url")]
    OpenUrl { url: String },
    #[serde(rename = "command")]
    Command {
        name: String,
        arg: Option<i64>,
        /// For `find`: what to search for.
        #[serde(default)]
        text: Option<String>,
    },
    /// What a search found: how many matches (negative when not known) and
    /// which of them the page is on, counting from 1.
    #[serde(rename = "find.result")]
    FindResult {
        tab_id: u64,
        #[serde(default)]
        text: String,
        #[serde(default)]
        count: i64,
        #[serde(default)]
        index: u64,
    },
    /// Requests of the watched tab that are new or have changed. With
    /// `reset`, they replace the list: the tab went to another page.
    #[serde(rename = "net.updated")]
    NetUpdated {
        tab_id: u64,
        #[serde(default)]
        reset: bool,
        /// The tab's page was loaded before recording began, so calls it
        /// makes through its own reference to `fetch` are not seen.
        #[serde(default)]
        late: bool,
        #[serde(default)]
        requests: Vec<NetRequest>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    pub error: Option<String>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Event(Event),
    Response(Response),
}

pub fn decode(line: &str) -> Result<Incoming, String> {
    let value: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("not an object".into());
    }
    if value.get("event").is_some() {
        let event: Event = serde_json::from_value(value).map_err(|e| e.to_string())?;
        return Ok(Incoming::Event(event));
    }
    let id = value.get("id").and_then(Value::as_u64).ok_or("response without id")?;
    let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let error = value.get("error").and_then(Value::as_str).map(str::to_string);
    Ok(Incoming::Response(Response { id, ok, error, body: value }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enc(req: Request) -> Value {
        serde_json::from_str(&encode(5, &req)).unwrap()
    }

    #[test]
    fn encodes_requests() {
        assert_eq!(enc(Request::Hello { protocol: 1 }), json!({"id": 5, "method": "hello", "protocol": 1}));
        assert_eq!(
            enc(Request::ViewportSet { col: 28, row: 1, cols: 100, rows: 40 }),
            json!({"id": 5, "method": "viewport.set", "col": 28, "row": 1, "cols": 100, "rows": 40})
        );
        assert_eq!(enc(Request::TabList), json!({"id": 5, "method": "tab.list"}));
        assert_eq!(
            enc(Request::TabCreate { url: Some("https://a.b".into()), input: None, activate: true, profile: None }),
            json!({"id": 5, "method": "tab.create", "url": "https://a.b", "activate": true})
        );
        assert_eq!(
            enc(Request::TabCreate { url: None, input: Some("a.b".into()), activate: false, profile: Some("work".into()) }),
            json!({"id": 5, "method": "tab.create", "input": "a.b", "activate": false, "profile": "work"})
        );
        assert_eq!(
            enc(Request::TabNavigate { tab_id: 3, input: "swift concurrency".into() }),
            json!({"id": 5, "method": "tab.navigate", "tab_id": 3, "input": "swift concurrency"})
        );
        assert_eq!(enc(Request::FocusSet { mode: Mode::Page }), json!({"id": 5, "method": "focus.set", "mode": "page"}));
        assert_eq!(enc(Request::Shutdown), json!({"id": 5, "method": "shutdown"}));
        assert_eq!(enc(Request::NetWatch { tab_id: Some(3) }), json!({"id": 5, "method": "net.watch", "tab_id": 3}));
        assert_eq!(enc(Request::NetWatch { tab_id: None }), json!({"id": 5, "method": "net.watch"}));
        assert_eq!(enc(Request::NetGet { tab_id: 3, request_id: 9 }), json!({"id": 5, "method": "net.get", "tab_id": 3, "request_id": 9}));
        assert_eq!(enc(Request::StorageGet { tab_id: 3 }), json!({"id": 5, "method": "storage.get", "tab_id": 3}));
        assert_eq!(
            enc(Request::StorageDelete { tab_id: 3, kind: "cookie".into(), site: None, name: Some("sid".into()), domain: Some(".a.b".into()), path: Some("/".into()), all: false }),
            json!({"id": 5, "method": "storage.delete", "tab_id": 3, "kind": "cookie", "name": "sid", "domain": ".a.b", "path": "/"})
        );
        assert_eq!(
            enc(Request::StorageDelete { tab_id: 3, kind: "site".into(), site: Some("a.b".into()), name: None, domain: None, path: None, all: true }),
            json!({"id": 5, "method": "storage.delete", "tab_id": 3, "kind": "site", "site": "a.b", "all": true})
        );
    }

    #[test]
    fn decodes_responses() {
        let Incoming::Response(r) = decode(r#"{"id":3,"ok":true,"tab_id":9}"#).unwrap() else { panic!() };
        assert_eq!((r.id, r.ok), (3, true));
        assert_eq!(r.body["tab_id"], 9);
        let Incoming::Response(r) = decode(r#"{"id":4,"ok":false,"error":"no such tab"}"#).unwrap() else { panic!() };
        assert!(!r.ok);
        assert_eq!(r.error.as_deref(), Some("no such tab"));
    }

    #[test]
    fn decodes_events() {
        let e = decode(r#"{"event":"tab.updated","tab_id":2,"title":"Hi","loading":true,"progress":0.5,"new_field":1}"#).unwrap();
        let Incoming::Event(Event::TabUpdated(s)) = e else { panic!() };
        assert_eq!(s.tab_id, 2);
        assert_eq!(s.title.as_deref(), Some("Hi"));
        assert_eq!(s.loading, Some(true));
        assert_eq!(s.url, None);

        assert_eq!(
            decode(r#"{"event":"tab.created","tab_id":7,"opener_tab_id":2,"url":"https://x.y"}"#).unwrap(),
            Incoming::Event(Event::TabCreated { tab_id: 7, opener_tab_id: Some(2), url: "https://x.y".into(), background: false })
        );
        assert_eq!(
            decode(r#"{"event":"focus.changed","mode":"page"}"#).unwrap(),
            Incoming::Event(Event::FocusChanged { mode: Mode::Page })
        );
        assert_eq!(
            decode(r#"{"event":"command","name":"select-tab","arg":3}"#).unwrap(),
            Incoming::Event(Event::Command { name: "select-tab".into(), arg: Some(3), text: None })
        );
        assert_eq!(decode(r#"{"event":"something.new","x":1}"#).unwrap(), Incoming::Event(Event::Unknown));

        let e = decode(r#"{"event":"net.updated","tab_id":2,"requests":[{"id":4,"method":"GET","url":"https://a.b/","status":200,"kind":"doc","state":"done","size":120,"started":1.5,"later":true}]}"#).unwrap();
        let Incoming::Event(Event::NetUpdated { tab_id, reset, requests, .. }) = e else { panic!() };
        assert_eq!((tab_id, reset, requests.len()), (2, false, 1));
        assert_eq!((requests[0].status, requests[0].size, requests[0].duration), (200, Some(120), None));
    }

    #[test]
    fn decodes_a_request_in_full() {
        let record: NetRecord = serde_json::from_value(json!({
            "id": 4, "url": "https://a.b/", "status": 200, "status_text": "OK", "detail": "full",
            "request_headers": [["Accept", "*/*"]], "response_body": "{}", "frames": [{"out": true, "data": "hi"}]
        }))
        .unwrap();
        assert_eq!((record.request.id, record.request.status, record.status_text.as_str()), (4, 200, "OK"));
        assert_eq!(record.request_headers, vec![("Accept".to_string(), "*/*".to_string())]);
        assert_eq!(record.frames[0], NetFrame { out: true, data: "hi".into() });
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode("nope").is_err());
        assert!(decode("[1,2]").is_err());
        assert!(decode(r#"{"ok":true}"#).is_err());
    }
}
