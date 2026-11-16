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
    Command { name: String, arg: Option<i64> },
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
            Incoming::Event(Event::Command { name: "select-tab".into(), arg: Some(3) })
        );
        assert_eq!(decode(r#"{"event":"something.new","x":1}"#).unwrap(), Incoming::Event(Event::Unknown));
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode("nope").is_err());
        assert!(decode("[1,2]").is_err());
        assert!(decode(r#"{"ok":true}"#).is_err());
    }
}
