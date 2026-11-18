//! The dev tools pane: the requests a page makes and what sites store. It is
//! drawn beside or below the page, which stays on screen.

use crate::app::{contains, App, Pending, UrlEdit};
use crate::history;
use crate::protocol::{Cookie, Mode, NetRecord, NetRequest, Request, Response, StorageData, StorageItem};
use crate::resolve::display_host;
use crate::ui::fit;
use crate::views::size;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// From this width on, the pane shows a list and the selected item's
/// details side by side.
pub const WIDE: u16 = 104;
/// The most lines of a body that are laid out.
const MAX_BODY_ROWS: usize = 4000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dock {
    #[default]
    Right,
    Bottom,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Network,
    Storage,
}

impl Tool {
    pub const ALL: [Tool; 2] = [Tool::Network, Tool::Storage];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Network => "Network",
            Tool::Storage => "Storage",
        }
    }
}

/// Divides the page area between the page and the pane.
pub fn split_page(page: Rect, dock: Dock, percent: u16) -> (Rect, Rect) {
    let share = |whole: u16| (u32::from(whole) * u32::from(percent.clamp(20, 80)) / 100) as u16;
    match dock {
        Dock::Right => {
            // The page keeps at least 16 columns; a pane narrower than 24 is no use.
            let most = page.width.saturating_sub(16);
            if most < 24 {
                return (page, Rect::default());
            }
            let width = share(page.width).clamp(40.min(most), most);
            (
                Rect::new(page.x, page.y, page.width - width, page.height),
                Rect::new(page.x + page.width - width, page.y, width, page.height),
            )
        }
        Dock::Bottom => {
            let most = page.height.saturating_sub(4);
            if most < 7 {
                return (page, Rect::default());
            }
            let height = share(page.height).clamp(9.min(most), most);
            (
                Rect::new(page.x, page.y, page.width, page.height - height),
                Rect::new(page.x, page.y + page.height - height, page.width, height),
            )
        }
    }
}

// ----- text for the detail side -----

/// What a piece of text is, which decides its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Text,
    Dim,
    Key,
    Head,
    Good,
    Warn,
    Bad,
    Accent,
}

pub type Row = Vec<(String, Tone)>;

/// Lays pieces out on lines of `width` cells, breaking between words where
/// it can and inside them where it must. Lines after the first begin `hang`
/// cells in.
fn wrap_spans(spans: &[(String, Tone)], width: usize, hang: usize) -> Vec<Row> {
    let width = width.max(8);
    let hang = hang.min(width / 2);
    let mut rows = Vec::new();
    let mut row: Row = Vec::new();
    let mut used = 0;
    let push = |row: &mut Row, text: &str, tone: Tone| match row.last_mut() {
        Some((last, last_tone)) if *last_tone == tone => last.push_str(text),
        _ => row.push((text.to_string(), tone)),
    };
    for (text, tone) in spans {
        let clean: String = text.chars().map(|c| if c == '\t' { ' ' } else { c }).filter(|c| !c.is_control()).collect();
        // A word with the spaces that follow it. Addresses and the like
        // may also be broken after their punctuation.
        for word in clean.split_inclusive([' ', '/', '?', '&', ',', ';', '=']) {
            let word_w = word.trim_end().width();
            if used + word_w > width && used > hang && word_w <= width - hang {
                rows.push(std::mem::take(&mut row));
                push(&mut row, &" ".repeat(hang), Tone::Text);
                used = hang;
            }
            let mut piece = String::new();
            for c in word.chars() {
                let w = c.width().unwrap_or(0);
                if used + w > width {
                    if c == ' ' {
                        // Spaces at the end of a line are not carried over.
                        continue;
                    }
                    push(&mut row, &std::mem::take(&mut piece), *tone);
                    rows.push(std::mem::take(&mut row));
                    push(&mut row, &" ".repeat(hang), Tone::Text);
                    used = hang;
                }
                piece.push(c);
                used += w;
            }
            push(&mut row, &piece, *tone);
        }
    }
    rows.push(row);
    rows
}

/// One line of pretty-printed JSON, coloured by what it holds.
fn json_spans(line: &str) -> Vec<(String, Tone)> {
    let indent = line.len() - line.trim_start().len();
    let (lead, rest) = line.split_at(indent);
    let mut spans = vec![(lead.to_string(), Tone::Text)];
    let value_tone = |value: &str| match value.chars().next() {
        Some('"') => Tone::Good,
        Some('{' | '[' | '}' | ']') => Tone::Dim,
        Some(c) if c.is_ascii_digit() || c == '-' => Tone::Warn,
        _ => Tone::Accent,
    };
    // `"key": value`, where the key itself may hold escaped quotes.
    if rest.starts_with('"') {
        let mut end = None;
        let mut escaped = false;
        for (i, c) in rest.char_indices().skip(1) {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        if let Some(end) = end.filter(|end| rest[end + 1..].starts_with(": ")) {
            spans.push((rest[..=end].to_string(), Tone::Key));
            spans.push((": ".to_string(), Tone::Dim));
            let value = &rest[end + 3..];
            spans.push((value.to_string(), value_tone(value)));
            return spans;
        }
    }
    spans.push((rest.to_string(), value_tone(rest)));
    spans
}

/// The text re-indented when it is JSON, and whether it was.
fn pretty(text: &str) -> (String, bool) {
    let trimmed = text.trim();
    if trimmed.starts_with(['{', '[']) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Ok(out) = serde_json::to_string_pretty(&value) {
                return (out, true);
            }
        }
    }
    (text.to_string(), false)
}

struct Doc {
    rows: Vec<Row>,
    width: usize,
}

impl Doc {
    fn new(width: usize) -> Self {
        Self { rows: Vec::new(), width: width.max(12) }
    }

    fn blank(&mut self) {
        if self.rows.last().is_some_and(|row| !row.is_empty()) {
            self.rows.push(Vec::new());
        }
    }

    fn heading(&mut self, text: &str) {
        self.blank();
        self.rows.push(vec![(text.to_string(), Tone::Head)]);
    }

    /// Names and values: in two columns when the names are short enough
    /// for that, otherwise each value straight after its name.
    fn pairs(&mut self, pairs: &[(String, String, Tone)]) {
        let longest = pairs.iter().map(|(key, _, _)| key.width()).max().unwrap_or(0);
        if longest > 28.min(self.width / 3) {
            for (key, value, tone) in pairs {
                let spans = [(format!("  {key}"), Tone::Key), (": ".to_string(), Tone::Dim), (value.clone(), *tone)];
                self.rows.extend(wrap_spans(&spans, self.width, 6));
            }
            return;
        }
        let key_w = longest.max(4);
        for (key, value, tone) in pairs {
            let key = format!("  {:<key_w$}  ", fit(key, key_w));
            let hang = key.width();
            self.rows.extend(wrap_spans(&[(key, Tone::Key), (value.clone(), *tone)], self.width, hang));
        }
    }

    fn note(&mut self, text: &str) {
        self.rows.extend(wrap_spans(&[(format!("  {text}"), Tone::Dim)], self.width, 2));
    }

    /// A body: JSON is re-indented and coloured, anything else is shown as it is.
    fn body(&mut self, text: &str) {
        let (text, json) = pretty(text);
        let total = text.lines().count();
        for line in text.lines().take(MAX_BODY_ROWS) {
            let indent = line.len() - line.trim_start().len();
            let line = format!("  {line}");
            let spans = if json { json_spans(&line) } else { vec![(line, Tone::Text)] };
            self.rows.extend(wrap_spans(&spans, self.width, indent + 4));
        }
        if total > MAX_BODY_ROWS {
            self.note(&format!("… {} more lines. c copies all of it.", total - MAX_BODY_ROWS));
        }
    }
}

// ----- addresses -----

/// Host, path and query of an address.
fn url_parts(url: &str) -> (&str, &str, &str) {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let rest = rest.split('#').next().unwrap_or(rest);
    let (before, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, path) = match before.find('/') {
        Some(at) => (&before[..at], &before[at..]),
        None => (before, ""),
    };
    (host, path, query)
}

/// What a request is called in the list: the last part of its path, with
/// its query, or its host when it asks for the root.
pub fn short_name(url: &str) -> String {
    if url.len() >= 5 && url[..5].eq_ignore_ascii_case("data:") {
        return "data:".to_string() + &url[5..url.len().min(40)].chars().take(24).collect::<String>();
    }
    let (host, path, query) = url_parts(url);
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let mut name = if last.is_empty() { format!("{host}/") } else { last.to_string() };
    if !query.is_empty() {
        name.push('?');
        name.push_str(query);
    }
    name
}

pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() && hex(bytes[i + 1]).is_some() && hex(bytes[i + 2]).is_some() => {
                out.push((hex(bytes[i + 1]).unwrap() * 16 + hex(bytes[i + 2]).unwrap()) as u8);
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a=1&b=2` as decoded names and values.
pub fn form_pairs(text: &str) -> Vec<(String, String)> {
    text.split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

/// "120 ms", "1.24 s", "2.1 min"
pub fn millis(ms: f64) -> String {
    if ms < 999.5 {
        format!("{ms:.0} ms")
    } else if ms < 60_000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else {
        format!("{:.1} min", ms / 60_000.0)
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

// ----- network -----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NetFilter {
    #[default]
    All,
    Fetch,
    Doc,
    Js,
    Css,
    Img,
    Media,
    Ws,
    Other,
}

impl NetFilter {
    pub const ALL: [NetFilter; 9] = [
        NetFilter::All,
        NetFilter::Fetch,
        NetFilter::Doc,
        NetFilter::Js,
        NetFilter::Css,
        NetFilter::Img,
        NetFilter::Media,
        NetFilter::Ws,
        NetFilter::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NetFilter::All => "All",
            NetFilter::Fetch => "Fetch",
            NetFilter::Doc => "Doc",
            NetFilter::Js => "JS",
            NetFilter::Css => "CSS",
            NetFilter::Img => "Img",
            NetFilter::Media => "Media",
            NetFilter::Ws => "WS",
            NetFilter::Other => "Other",
        }
    }

    pub fn matches(self, kind: &str) -> bool {
        match self {
            NetFilter::All => true,
            NetFilter::Fetch => matches!(kind, "fetch" | "xhr"),
            NetFilter::Doc => kind == "doc",
            NetFilter::Js => kind == "js",
            NetFilter::Css => kind == "css",
            NetFilter::Img => kind == "img",
            NetFilter::Media => kind == "media",
            NetFilter::Ws => kind == "ws",
            NetFilter::Other => !matches!(kind, "fetch" | "xhr" | "doc" | "js" | "css" | "img" | "media" | "ws"),
        }
    }
}

/// The parts of a request's details.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NetTab {
    #[default]
    Headers,
    Payload,
    Response,
}

impl NetTab {
    pub const ALL: [NetTab; 3] = [NetTab::Headers, NetTab::Payload, NetTab::Response];

    pub fn label(self) -> &'static str {
        match self {
            NetTab::Headers => "Headers",
            NetTab::Payload => "Payload",
            NetTab::Response => "Response",
        }
    }
}

#[derive(Debug, Default)]
pub struct NetState {
    /// The watched tab's requests, in the order they began.
    pub requests: Vec<NetRequest>,
    /// The selected request, by its id.
    pub selected: Option<u64>,
    /// First line of the list on screen.
    pub scroll: usize,
    /// The selection stays on the newest request.
    pub follow: bool,
    pub filter: NetFilter,
    /// Only requests whose address holds this are listed.
    pub search: String,
    /// Requests are kept when the tab goes to another page.
    pub preserve: bool,
    /// The page was loaded before recording began: only a reload records it in full.
    pub late: bool,
    /// The keys go to the details, which in a narrow pane take the list's place.
    pub detail_open: bool,
    pub tab: NetTab,
    pub detail: Option<NetRecord>,
    /// The request whose details were last asked for.
    detail_asked: Option<u64>,
    version: u64,
}

impl NetState {
    /// Indices of the requests the filter and the search let through.
    pub fn visible(&self) -> Vec<usize> {
        let needle = self.search.to_lowercase();
        self.requests
            .iter()
            .enumerate()
            .filter(|(_, r)| self.filter.matches(&r.kind) && (needle.is_empty() || r.url.to_lowercase().contains(&needle)))
            .map(|(index, _)| index)
            .collect()
    }

    pub fn selected_request(&self) -> Option<&NetRequest> {
        self.selected.and_then(|id| self.requests.iter().find(|r| r.id == id))
    }

    fn forget(&mut self) {
        self.requests.clear();
        self.selected = None;
        self.scroll = 0;
        self.detail = None;
        self.detail_asked = None;
        self.detail_open = false;
        self.late = false;
        self.version += 1;
    }

    fn merge(&mut self, request: NetRequest) {
        if let Some(known) = self.requests.iter_mut().rev().find(|r| r.id == request.id) {
            *known = request;
        } else {
            let at = self.requests.partition_point(|r| (r.started, r.id) <= (request.started, request.id));
            self.requests.insert(at, request);
        }
    }
}

// ----- storage -----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StorageKind {
    #[default]
    Cookies,
    Local,
    Session,
    IndexedDb,
    Cache,
}

impl StorageKind {
    pub const ALL: [StorageKind; 5] =
        [StorageKind::Cookies, StorageKind::Local, StorageKind::Session, StorageKind::IndexedDb, StorageKind::Cache];

    pub fn label(self) -> &'static str {
        match self {
            StorageKind::Cookies => "Cookies",
            StorageKind::Local => "Local",
            StorageKind::Session => "Session",
            StorageKind::IndexedDb => "IndexedDB",
            StorageKind::Cache => "Cache",
        }
    }

    /// The name in full, for sentences.
    pub fn long(self) -> &'static str {
        match self {
            StorageKind::Cookies => "cookies",
            StorageKind::Local => "local storage",
            StorageKind::Session => "session storage",
            StorageKind::IndexedDb => "IndexedDB databases",
            StorageKind::Cache => "caches",
        }
    }

    /// "3 cookies", "1 local storage key"
    pub fn counted(self, count: usize) -> String {
        let (one, many) = match self {
            StorageKind::Cookies => ("cookie", "cookies"),
            StorageKind::Local => ("local storage key", "local storage keys"),
            StorageKind::Session => ("session storage key", "session storage keys"),
            StorageKind::IndexedDb => ("IndexedDB database", "IndexedDB databases"),
            StorageKind::Cache => ("cache", "caches"),
        };
        format!("{count} {}", if count == 1 { one } else { many })
    }

    fn wire(self) -> &'static str {
        match self {
            StorageKind::Cookies => "cookie",
            StorageKind::Local => "local",
            StorageKind::Session => "session",
            StorageKind::IndexedDb => "indexeddb",
            StorageKind::Cache => "cache",
        }
    }
}

/// A site in the site list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SiteRow {
    pub name: String,
    pub cookies: usize,
    /// What else it stores, as WebKit names it.
    pub kinds: Vec<String>,
    /// The tab's page belongs to it.
    pub current: bool,
}

/// Where a line of the storage list comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemRef {
    Cookie(usize),
    Local(usize),
    Session(usize),
    /// A database and one of its object stores, if it has any.
    Store(usize, Option<usize>),
    Cache(usize),
}

/// A line of the storage list.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemRow {
    pub name: String,
    pub value: String,
    /// A cookie's domain.
    pub extra: String,
    /// A cookie's expiry.
    pub when: String,
    pub source: ItemRef,
}

#[derive(Debug, Default)]
pub struct StorageState {
    pub data: Option<StorageData>,
    pub loading: bool,
    /// The data should be read again.
    pub(crate) stale: bool,
    /// The site of each cookie, in step with `data.cookies`.
    cookie_sites: Vec<String>,
    /// Every site with data, the page's own first.
    pub sites: Vec<SiteRow>,
    /// The site chosen in the site list; `None` follows the tab's page.
    pub site: Option<String>,
    pub kind: StorageKind,
    pub selected: usize,
    pub scroll: usize,
    pub search: String,
    pub detail_open: bool,
    /// The site list is showing.
    pub picking: bool,
    pub pick_selected: usize,
    pub pick_scroll: usize,
    pub pick_search: String,
    version: u64,
}

impl StorageState {
    fn set(&mut self, data: StorageData) {
        let mut names: HashSet<String> = data.sites.iter().map(|s| s.name.to_lowercase()).collect();
        if !data.site.is_empty() {
            names.insert(data.site.to_lowercase());
        }
        let mut counts: HashMap<String, usize> = HashMap::new();
        self.cookie_sites = data
            .cookies
            .iter()
            .map(|cookie| {
                let domain = cookie.domain.trim_start_matches('.').to_lowercase();
                // The site is the longest name a cookie's domain ends in.
                let mut rest = domain.as_str();
                let site = loop {
                    if names.contains(rest) {
                        break rest.to_string();
                    }
                    match rest.split_once('.') {
                        Some((_, tail)) => rest = tail,
                        None => break domain.clone(),
                    }
                };
                *counts.entry(site.clone()).or_default() += 1;
                site
            })
            .collect();
        names.extend(counts.keys().cloned());
        let kinds: HashMap<String, Vec<String>> = data.sites.iter().map(|s| (s.name.to_lowercase(), s.kinds.clone())).collect();
        let page = data.site.to_lowercase();
        let mut sites: Vec<SiteRow> = names
            .into_iter()
            .map(|name| SiteRow {
                cookies: counts.get(&name).copied().unwrap_or(0),
                kinds: kinds.get(&name).cloned().unwrap_or_default(),
                current: name == page,
                name,
            })
            .collect();
        sites.sort_by(|a, b| b.current.cmp(&a.current).then_with(|| a.name.cmp(&b.name)));
        self.sites = sites;
        if self.site.as_ref().is_some_and(|site| !self.sites.iter().any(|s| s.name == *site)) {
            self.site = None;
        }
        self.data = Some(data);
        self.version += 1;
    }

    /// The site whose storage is showing.
    pub fn site_name(&self) -> String {
        self.site.clone().or_else(|| self.data.as_ref().map(|d| d.site.to_lowercase())).unwrap_or_default()
    }

    /// Whether the showing site is the one the tab's page belongs to, so
    /// that more than its cookies can be read.
    pub fn on_page_site(&self) -> bool {
        self.data.as_ref().is_some_and(|d| !d.site.is_empty() && d.site.to_lowercase() == self.site_name())
    }

    /// How many items the showing site has of a kind, when that is known.
    pub fn count(&self, kind: StorageKind) -> Option<usize> {
        let data = self.data.as_ref()?;
        let site = self.site_name();
        if kind == StorageKind::Cookies {
            return Some(self.cookie_sites.iter().filter(|s| **s == site).count());
        }
        if !self.on_page_site() {
            return None;
        }
        Some(match kind {
            StorageKind::Local => data.local.len(),
            StorageKind::Session => data.session.len(),
            StorageKind::IndexedDb => data.databases.len(),
            StorageKind::Cache => data.caches.len(),
            StorageKind::Cookies => 0,
        })
    }

    /// The lines of the list: the showing site's items of the showing kind.
    pub fn items(&self, now: u64) -> Vec<ItemRow> {
        let Some(data) = &self.data else { return Vec::new() };
        let site = self.site_name();
        let stored = |items: &[StorageItem], make: fn(usize) -> ItemRef| -> Vec<ItemRow> {
            items
                .iter()
                .enumerate()
                .map(|(index, item)| ItemRow {
                    name: item.key.clone(),
                    value: item.value.clone(),
                    extra: String::new(),
                    when: String::new(),
                    source: make(index),
                })
                .collect()
        };
        let mut rows: Vec<ItemRow> = match self.kind {
            StorageKind::Cookies => {
                let mut rows: Vec<ItemRow> = data
                    .cookies
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| self.cookie_sites.get(*index) == Some(&site))
                    .map(|(index, cookie)| ItemRow {
                        name: cookie.name.clone(),
                        value: cookie.value.clone(),
                        extra: cookie.domain.clone(),
                        when: expiry(cookie, now, false),
                        source: ItemRef::Cookie(index),
                    })
                    .collect();
                rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.extra.cmp(&b.extra)));
                rows
            }
            _ if !self.on_page_site() => Vec::new(),
            StorageKind::Local => stored(&data.local, ItemRef::Local),
            StorageKind::Session => stored(&data.session, ItemRef::Session),
            StorageKind::IndexedDb => data
                .databases
                .iter()
                .enumerate()
                .flat_map(|(d, database)| {
                    let row = |name: String, value: String, store: Option<usize>| ItemRow {
                        name,
                        value,
                        extra: String::new(),
                        when: String::new(),
                        source: ItemRef::Store(d, store),
                    };
                    if database.stores.is_empty() {
                        return vec![row(database.name.clone(), "no object stores".into(), None)];
                    }
                    database
                        .stores
                        .iter()
                        .enumerate()
                        .map(|(s, store)| {
                            let records = format!("{} record{}", store.count, if store.count == 1 { "" } else { "s" });
                            row(format!("{} › {}", database.name, store.name), records, Some(s))
                        })
                        .collect()
                })
                .collect(),
            StorageKind::Cache => data
                .caches
                .iter()
                .enumerate()
                .map(|(index, cache)| ItemRow {
                    name: cache.name.clone(),
                    value: format!("{} entr{}", cache.count, if cache.count == 1 { "y" } else { "ies" }),
                    extra: String::new(),
                    when: String::new(),
                    source: ItemRef::Cache(index),
                })
                .collect(),
        };
        let needle = self.search.to_lowercase();
        if !needle.is_empty() {
            rows.retain(|row| row.name.to_lowercase().contains(&needle) || row.value.to_lowercase().contains(&needle));
        }
        rows
    }

    /// The sites the site list shows.
    pub fn picker_sites(&self) -> Vec<&SiteRow> {
        let needle = self.pick_search.to_lowercase();
        self.sites.iter().filter(|site| needle.is_empty() || site.name.contains(&needle)).collect()
    }
}

/// When a cookie ends: "Session", "Expired", or a date.
fn expiry(cookie: &Cookie, now: u64, long: bool) -> String {
    match cookie.expires {
        None => "Session".to_string(),
        Some(at) if at <= now as f64 => "Expired".to_string(),
        Some(at) => {
            let date = history::full_date(at as u64);
            if !long {
                return date;
            }
            let days = ((at - now as f64) / 86_400.0).floor() as u64;
            match days {
                0 => format!("{date}, {} (today)", history::clock(at as u64)),
                1 => format!("{date} (tomorrow)"),
                _ => format!("{date} (in {days} days)"),
            }
        }
    }
}

// ----- the pane -----

/// A question the pane is waiting on a y for.
#[derive(Debug, Clone, PartialEq)]
pub enum Confirm {
    /// Delete every item of the showing kind for the showing site.
    ClearKind,
    /// Delete everything a site stores.
    ClearSite(String),
}

#[derive(Debug)]
pub struct DevTools {
    pub open: bool,
    /// The pane has the keys (while the TUI has them at all).
    pub focused: bool,
    pub dock: Dock,
    /// The share of the page area the pane takes.
    pub percent: u16,
    pub tool: Tool,
    pub net: NetState,
    pub storage: StorageState,
    /// The text of the detail side, laid out for its width.
    pub rows: Vec<Row>,
    /// What `rows` was laid out from, and which item it is about.
    rows_key: String,
    rows_item: String,
    pub detail_scroll: usize,
    /// The filter field while it is being typed in.
    pub search: Option<UrlEdit>,
    pub confirm: Option<Confirm>,
    /// What the app was last told to watch: nothing at all, or a tab (or no
    /// tab) with capturing on.
    pub(crate) watch_sent: Option<Option<u64>>,
}

impl Default for DevTools {
    fn default() -> Self {
        Self {
            open: false,
            focused: false,
            dock: Dock::Right,
            percent: 45,
            tool: Tool::Network,
            net: NetState::default(),
            storage: StorageState::default(),
            rows: Vec::new(),
            rows_key: String::new(),
            rows_item: String::new(),
            detail_scroll: 0,
            search: None,
            confirm: None,
            watch_sent: None,
        }
    }
}

/// Moves a selection by `delta` within `len` items.
fn stepped(selected: usize, delta: i64, len: usize) -> usize {
    (selected as i64).saturating_add(delta).clamp(0, len.saturating_sub(1) as i64) as usize
}

/// The scroll offset that keeps line `selected` within `height` lines.
fn scrolled(scroll: usize, selected: usize, len: usize, height: usize) -> usize {
    if height == 0 {
        return 0;
    }
    let mut scroll = scroll;
    if selected < scroll {
        scroll = selected;
    } else if selected >= scroll + height {
        scroll = selected + 1 - height;
    }
    scroll.min(len.saturating_sub(height))
}

impl App {
    // ----- where things are -----

    /// The pane without the rule that divides it from the page.
    pub fn dev_inner(&self) -> Rect {
        let pane = self.layout.devtools;
        if pane.width == 0 || pane.height == 0 {
            return Rect::default();
        }
        match self.dev.dock {
            Dock::Right => Rect::new(pane.x + 1, pane.y, pane.width - 1, pane.height),
            Dock::Bottom => pane,
        }
    }

    /// Where each tool's name is in the pane's top line.
    pub fn dev_tool_tabs(&self) -> Vec<(Tool, Rect)> {
        let inner = self.dev_inner();
        let mut x = inner.x + 1;
        let mut tabs = Vec::new();
        for tool in Tool::ALL {
            let width = tool.label().width() as u16 + 2;
            if x + width > inner.x + inner.width {
                break;
            }
            tabs.push((tool, Rect::new(x, inner.y, width, inner.height.min(1))));
            x += width + 1;
        }
        tabs
    }

    /// The lines at the bottom of the pane that name its keys: as many as
    /// the tool's longest set of keys needs, up to three.
    pub fn dev_keys_rect(&self) -> Rect {
        let inner = self.dev_inner();
        let keys = match self.dev.tool {
            Tool::Network => crate::keys::NET_LIST,
            Tool::Storage => crate::keys::STORAGE_LIST,
        };
        // A short pane keeps its lines for the list.
        let rows = key_rows(keys, inner.width).min(3).min((inner.height / 6).max(1)).min(inner.height.saturating_sub(4));
        Rect::new(inner.x, inner.y + inner.height - rows, inner.width, rows)
    }

    /// Everything between the top line and the keys.
    pub fn dev_body(&self) -> Rect {
        let inner = self.dev_inner();
        let keys = self.dev_keys_rect().height;
        Rect::new(inner.x, inner.y + inner.height.min(1), inner.width, inner.height.saturating_sub(1 + keys))
    }

    pub fn dev_wide(&self) -> bool {
        self.dev_inner().width >= WIDE
    }

    /// The details have the keys.
    pub fn dev_detail_open(&self) -> bool {
        match self.dev.tool {
            Tool::Network => self.dev.net.detail_open,
            Tool::Storage => self.dev.storage.detail_open && !self.dev.storage.picking,
        }
    }

    /// The column of the list, and the area of the details when they are
    /// on screen. In a wide pane they share it; in a narrow one the
    /// details take the list's place while they are open.
    pub fn dev_columns(&self) -> (Rect, Option<Rect>) {
        let body = self.dev_body();
        if self.dev_wide() {
            let left = (body.width * 45 / 100).max(48);
            // One column between them holds the rule.
            return (Rect::new(body.x, body.y, left, body.height), Some(Rect::new(body.x + left + 1, body.y, body.width - left - 1, body.height)));
        }
        if self.dev_detail_open() {
            (Rect::default(), Some(body))
        } else {
            (body, None)
        }
    }

    /// Lines of the list column above the list itself.
    fn dev_list_head(&self) -> u16 {
        match self.dev.tool {
            // The filters, the column names, and the notice of a page recorded in part.
            Tool::Network => 2 + u16::from(self.dev.net.late),
            // The site list has only its title.
            Tool::Storage if self.dev.storage.picking => 1,
            // The site, the kinds, then the column names.
            Tool::Storage => 3,
        }
    }

    /// The lines the list's items are drawn on.
    pub fn dev_list(&self) -> Rect {
        let (column, _) = self.dev_columns();
        let head = self.dev_list_head().min(column.height);
        Rect::new(column.x, column.y + head, column.width, column.height - head)
    }

    /// The lines the details' text is drawn on, under their title.
    pub fn dev_detail_text(&self) -> Rect {
        let Some(area) = self.dev_columns().1 else { return Rect::default() };
        let head = match self.dev.tool {
            Tool::Network => 3,
            Tool::Storage => 2,
        }
        .min(area.height);
        Rect::new(area.x + 1, area.y + head, area.width.saturating_sub(2), area.height - head)
    }

    /// Where the filters of the network list are: each with its column and width.
    pub fn net_chips(&self) -> Vec<(NetFilter, u16, u16)> {
        let (column, _) = self.dev_columns();
        let mut x = column.x + 1;
        let end = column.x + column.width;
        let mut chips = Vec::new();
        for filter in NetFilter::ALL {
            let width = filter.label().width() as u16 + 2;
            if x + width > end {
                // They do not all fit: the line names the current one instead.
                return Vec::new();
            }
            chips.push((filter, x, width));
            x += width;
        }
        chips
    }

    /// Where the kinds of storage are named: each with its column, width
    /// and name. The names lose their counts, then their length, as the
    /// column gets narrower.
    pub fn storage_chips(&self) -> Vec<(StorageKind, u16, u16, String)> {
        let (column, _) = self.dev_columns();
        let end = column.x + column.width;
        let label = |kind: StorageKind, level: u8| {
            let name = if level % 2 == 1 && kind == StorageKind::IndexedDb { "IDB" } else { kind.label() };
            match self.dev.storage.count(kind) {
                Some(count) if level < 2 => format!("{name} {count}"),
                _ => name.to_string(),
            }
        };
        let mut chips = Vec::new();
        for level in 0..4 {
            chips.clear();
            let mut x = column.x + 1;
            for kind in StorageKind::ALL {
                let label = label(kind, level);
                let width = label.width() as u16 + 2;
                if x + width > end {
                    break;
                }
                chips.push((kind, x, width, label));
                x += width;
            }
            if chips.len() == StorageKind::ALL.len() {
                break;
            }
        }
        chips
    }

    /// Where the parts of a request's details are named.
    pub fn net_tabs(&self) -> Vec<(NetTab, u16, u16)> {
        let Some(area) = self.dev_columns().1 else { return Vec::new() };
        let mut x = area.x + 1;
        NetTab::ALL
            .iter()
            .map(|tab| {
                let width = tab.label().width() as u16 + 2;
                let at = x;
                x += width;
                (*tab, at, width)
            })
            .filter(|(_, at, width)| at + width <= area.x + area.width)
            .collect()
    }

    // ----- opening and closing -----

    /// The pane is on screen and the keys typed in the TUI are its own.
    pub fn dev_has_keys(&self) -> bool {
        self.dev.open && self.dev.focused && self.layout.devtools.width > 0
    }

    pub fn open_devtools(&mut self) {
        self.cancel_edit();
        if !self.dev.open {
            self.dev.open = true;
            self.dev.storage.stale = true;
        }
        self.dev.focused = true;
        self.message = None;
        self.request(Request::FocusSet { mode: Mode::Nav });
    }

    pub fn close_devtools(&mut self) {
        let preserve = self.dev.net.preserve;
        self.dev.open = false;
        self.dev.focused = false;
        self.dev.search = None;
        self.dev.confirm = None;
        self.dev.net = NetState { preserve, filter: self.dev.net.filter, ..Default::default() };
        self.dev.storage = StorageState { kind: self.dev.storage.kind, ..Default::default() };
        self.dev.rows.clear();
        self.dev.rows_key.clear();
        self.dev.rows_item.clear();
    }

    /// Opens the pane, gives it the keys if it lacks them, or closes it.
    pub fn toggle_devtools(&mut self) {
        if self.dev.open && self.dev.focused && self.mode == Mode::Nav {
            self.close_devtools();
        } else {
            self.open_devtools();
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        if self.dev.tool == tool {
            return;
        }
        self.dev.tool = tool;
        self.dev.search = None;
        self.dev.confirm = None;
        self.dev.detail_scroll = 0;
        if tool == Tool::Storage {
            self.dev.storage.stale = true;
        }
    }

    fn toggle_dock(&mut self) {
        self.dev.dock = match self.dev.dock {
            Dock::Right => Dock::Bottom,
            Dock::Bottom => Dock::Right,
        };
        self.session_dirty = true;
    }

    fn resize_pane(&mut self, delta: i16) {
        self.dev.percent = (self.dev.percent as i16 + delta).clamp(20, 80) as u16;
        self.session_dirty = true;
    }

    // ----- keeping the app and the pane in step -----

    /// Tells the app what the pane needs, when that has changed: which
    /// tab's requests to report, the storage, the selected request's details.
    pub(crate) fn dev_sync(&mut self) {
        if !self.ready {
            return;
        }
        let tab_id = self.current_tab_id();
        let want = self.dev.open.then_some(tab_id);
        if want != self.dev.watch_sent {
            self.dev.watch_sent = want;
            self.dev.net.forget();
            self.dev.storage.data = None;
            self.dev.storage.loading = false;
            self.dev.storage.stale = true;
            match want {
                Some(tab_id) => self.send(Request::NetWatch { tab_id }, Pending::NetWatch { tab_id }),
                None => self.request(Request::NetStop),
            }
        }
        if !self.dev.open {
            return;
        }
        let Some(tab_id) = tab_id else { return };
        match self.dev.tool {
            Tool::Storage => {
                if self.dev.storage.stale && !self.dev.storage.loading {
                    self.dev.storage.stale = false;
                    self.dev.storage.loading = true;
                    self.send(Request::StorageGet { tab_id }, Pending::Storage { tab_id });
                }
            }
            Tool::Network => {
                let on_screen = self.dev_wide() || self.dev.net.detail_open;
                let want = self.dev.net.selected.filter(|_| on_screen);
                if want != self.dev.net.detail_asked {
                    self.dev.net.detail_asked = want;
                    if let Some(request_id) = want {
                        self.send(Request::NetGet { tab_id, request_id }, Pending::NetDetail { tab_id, request_id });
                    }
                }
            }
        }
    }

    fn watching(&self, tab_id: Option<u64>) -> bool {
        self.dev.open && self.dev.watch_sent == Some(tab_id)
    }

    pub(crate) fn net_watched(&mut self, tab_id: Option<u64>, response: Response) {
        if !self.watching(tab_id) {
            return;
        }
        if !response.ok {
            self.message = Some(format!("Cannot watch requests: {}", response.error.unwrap_or_default()));
            return;
        }
        let requests: Vec<NetRequest> = serde_json::from_value(response.body["requests"].clone()).unwrap_or_default();
        self.dev.net.forget();
        self.dev.net.late = response.body["late"].as_bool().unwrap_or(false);
        for request in requests {
            self.dev.net.merge(request);
        }
        self.net_settle();
    }

    pub(crate) fn net_updated(&mut self, tab_id: u64, reset: bool, late: bool, requests: Vec<NetRequest>) {
        if !self.watching(Some(tab_id)) {
            return;
        }
        if reset {
            let open = self.dev.net.detail_open;
            self.dev.net.forget();
            // In a wide pane the details stay in focus across pages.
            self.dev.net.detail_open = open && self.dev_wide();
        }
        self.dev.net.late = late;
        for request in requests {
            if self.dev.net.selected == Some(request.id) {
                // Its details are out of date.
                self.dev.net.detail_asked = None;
            }
            self.dev.net.merge(request);
        }
        self.net_settle();
    }

    pub(crate) fn net_detail_arrived(&mut self, tab_id: u64, request_id: u64, response: Response) {
        if !self.watching(Some(tab_id)) || self.dev.net.selected != Some(request_id) || !response.ok {
            return;
        }
        if let Ok(record) = serde_json::from_value::<NetRecord>(response.body["request"].clone()) {
            self.dev.net.detail = Some(record);
            self.dev.net.version += 1;
        }
    }

    pub(crate) fn storage_arrived(&mut self, tab_id: u64, response: Response) {
        self.dev.storage.loading = false;
        if !self.watching(Some(tab_id)) {
            return;
        }
        if !response.ok {
            self.message = Some(format!("Cannot read storage: {}", response.error.unwrap_or_default()));
            return;
        }
        if let Ok(data) = serde_json::from_value::<StorageData>(response.body.clone()) {
            self.dev.storage.set(data);
        }
    }

    /// Keeps the network selection on a listed request and on screen.
    fn net_settle(&mut self) {
        let visible = self.dev.net.visible();
        let height = self.dev_list().height as usize;
        let net = &mut self.dev.net;
        let position = net.selected.and_then(|id| visible.iter().position(|&i| net.requests[i].id == id));
        let position = match position {
            _ if visible.is_empty() => {
                net.selected = None;
                net.scroll = 0;
                net.detail_open = false;
                return;
            }
            Some(_) if net.follow => visible.len() - 1,
            Some(position) => position,
            None if net.follow => visible.len() - 1,
            None => 0,
        };
        let id = net.requests[visible[position]].id;
        if net.selected != Some(id) {
            net.selected = Some(id);
            self.dev.detail_scroll = 0;
        }
        self.dev.net.scroll = scrolled(self.dev.net.scroll, position, visible.len(), height);
    }

    fn storage_settle(&mut self) {
        let height = self.dev_list().height as usize;
        let now = self.now();
        let storage = &mut self.dev.storage;
        if storage.picking {
            let len = storage.picker_sites().len();
            storage.pick_selected = storage.pick_selected.min(len.saturating_sub(1));
            storage.pick_scroll = scrolled(storage.pick_scroll, storage.pick_selected, len, height);
            return;
        }
        let len = storage.items(now).len();
        storage.selected = storage.selected.min(len.saturating_sub(1));
        storage.scroll = scrolled(storage.scroll, storage.selected, len, height);
        if len == 0 {
            storage.detail_open = false;
        }
    }

    /// Called before every draw: settles selections for the size the pane
    /// has now, and lays out the text of the details.
    pub(crate) fn dev_prepare(&mut self) {
        if !self.dev.open || self.layout.devtools.width == 0 {
            return;
        }
        match self.dev.tool {
            Tool::Network => self.net_settle(),
            Tool::Storage => self.storage_settle(),
        }
        let text = self.dev_detail_text();
        if text.width == 0 || text.height == 0 {
            return;
        }
        let width = text.width as usize;
        // The item the details are about, and the state of what is known of it.
        let (item, version) = match self.dev.tool {
            Tool::Network => {
                let net = &self.dev.net;
                (format!("net/{:?}/{:?}", net.selected, net.tab), net.version)
            }
            Tool::Storage => {
                let storage = &self.dev.storage;
                (format!("storage/{}/{:?}/{}/{}", storage.site_name(), storage.kind, storage.selected, storage.search), storage.version)
            }
        };
        let key = format!("{item}/{width}/{version}");
        if key != self.dev.rows_key {
            // The same item with more known of it, or in another width,
            // keeps its place; another item starts at its top.
            if item != self.dev.rows_item {
                self.dev.detail_scroll = 0;
            }
            self.dev.rows = match self.dev.tool {
                Tool::Network => self.net_rows(width),
                Tool::Storage => self.storage_rows(width),
            };
            self.dev.rows_key = key;
            self.dev.rows_item = item;
        }
        self.dev.detail_scroll = self.dev.detail_scroll.min(self.dev.rows.len().saturating_sub(text.height as usize));
    }

    // ----- the text of the details -----

    fn net_rows(&self, width: usize) -> Vec<Row> {
        let net = &self.dev.net;
        let Some(request) = net.selected_request() else { return Vec::new() };
        let mut doc = Doc::new(width);
        let Some(record) = net.detail.as_ref().filter(|d| d.request.id == request.id) else {
            doc.note("Loading…");
            return doc.rows;
        };
        let request = &record.request;
        match net.tab {
            NetTab::Headers => {
                doc.heading("General");
                let status_tone = status_tone(request);
                let mut general = vec![
                    ("URL".to_string(), request.url.clone(), Tone::Text),
                    ("Method".to_string(), if request.method.is_empty() { "Not recorded".to_string() } else { request.method.clone() }, Tone::Text),
                    ("Status".to_string(), status_text(record), status_tone),
                ];
                let kind = if request.mime.is_empty() { kind_name(&request.kind).to_string() } else { format!("{} · {}", kind_name(&request.kind), request.mime) };
                general.push(("Type".to_string(), kind, Tone::Text));
                match (request.size, record.body_size) {
                    (None, Some(body)) if request.cached => general.push(("Size".to_string(), format!("from the cache · {} decoded", size(body)), Tone::Text)),
                    (None, None) if request.cached => general.push(("Size".to_string(), "from the cache".to_string(), Tone::Text)),
                    (Some(wire), Some(body)) if body != wire => general.push(("Size".to_string(), format!("{} transferred · {} decoded", size(wire), size(body)), Tone::Text)),
                    (Some(wire), _) => general.push(("Size".to_string(), size(wire), Tone::Text)),
                    (None, Some(body)) => general.push(("Size".to_string(), format!("{} decoded", size(body)), Tone::Text)),
                    (None, None) => {}
                }
                let mut time = Vec::new();
                if let Some(duration) = request.duration {
                    time.push(millis(duration));
                }
                if request.started > 0.0 {
                    time.push(format!("started {}", history::clock_seconds((request.started / 1000.0) as u64)));
                }
                if !record.protocol.is_empty() {
                    time.push(record.protocol.clone());
                }
                if !time.is_empty() {
                    general.push(("Time".to_string(), time.join(" · "), Tone::Text));
                }
                if let Some(error) = &request.error {
                    general.push(("Error".to_string(), error.clone(), Tone::Bad));
                }
                doc.pairs(&general);

                let headers = |doc: &mut Doc, title: &str, headers: &[(String, String)]| {
                    if headers.is_empty() {
                        return;
                    }
                    doc.heading(&format!("{title} ({})", headers.len()));
                    let pairs: Vec<_> = headers.iter().map(|(k, v)| (k.clone(), v.clone(), Tone::Text)).collect();
                    doc.pairs(&pairs);
                };
                headers(&mut doc, "Response headers", &record.response_headers);
                headers(&mut doc, "Request headers", &record.request_headers);
                doc.blank();
                match record.detail.as_str() {
                    "full" => doc.note("Request headers are the ones the page's script set. Those WebKit adds itself (cookies, user agent) cannot be seen, and a response from another origin shows only the headers its server exposes."),
                    "document" => doc.note("The request headers WebKit adds itself (cookies, user agent) cannot be seen."),
                    _ if scripted(request) => doc.note(UNRECORDED),
                    _ => doc.note("WebKit reports only the timing and size of requests a page does not make from script, so there are no headers to show."),
                }
            }
            NetTab::Payload => {
                let (_, _, query) = url_parts(&request.url);
                let query = form_pairs(query);
                if !query.is_empty() {
                    doc.heading(&format!("Query parameters ({})", query.len()));
                    let pairs: Vec<_> = query.into_iter().map(|(k, v)| (k, v, Tone::Text)).collect();
                    doc.pairs(&pairs);
                }
                let form = header(&record.request_headers, "content-type").is_some_and(|t| t.to_lowercase().contains("x-www-form-urlencoded"));
                match &record.request_body {
                    Some(body) if form && !body.is_empty() => {
                        let fields = form_pairs(body);
                        doc.heading(&format!("Form data ({})", fields.len()));
                        let pairs: Vec<_> = fields.into_iter().map(|(k, v)| (k, v, Tone::Text)).collect();
                        doc.pairs(&pairs);
                    }
                    Some(body) if !body.is_empty() => {
                        doc.heading("Request body");
                        doc.body(body);
                    }
                    _ => {}
                }
                if let Some(note) = &record.request_note {
                    if record.request_body.as_ref().is_some_and(|b| !b.is_empty()) {
                        doc.blank();
                    } else {
                        doc.heading("Request body");
                    }
                    doc.note(note);
                }
                if record.detail != "full" && scripted(request) {
                    doc.blank();
                    doc.note(UNRECORDED);
                } else if doc.rows.is_empty() {
                    doc.note(if record.detail == "full" { "This request has no query parameters and no body." } else { "This request has no query parameters. A body, if it sent one, cannot be seen." });
                }
            }
            NetTab::Response => {
                if request.kind == "ws" {
                    doc.heading(&format!("Messages ({})", record.frames.len()));
                    if record.frames.is_empty() {
                        doc.note("None yet.");
                    }
                    for frame in &record.frames {
                        let (arrow, tone) = if frame.out { ("  ↑ ", Tone::Accent) } else { ("  ↓ ", Tone::Good) };
                        doc.rows.extend(wrap_spans(&[(arrow.to_string(), tone), (frame.data.clone(), Tone::Text)], width, 4));
                    }
                } else if let Some(body) = record.response_body.as_ref().filter(|b| !b.is_empty()) {
                    doc.body(body);
                    if record.response_truncated {
                        doc.blank();
                        doc.note("The rest of this body was not kept: it is longer than Tilde records.");
                    }
                } else if let Some(note) = &record.response_note {
                    doc.note(note);
                } else if request.state == "pending" {
                    doc.note("Waiting for the response…");
                } else if record.detail == "full" {
                    doc.note("The response has no body.");
                } else if scripted(request) {
                    doc.note(UNRECORDED);
                } else {
                    doc.note("The body cannot be seen. WebKit shows Tilde the content of what a page fetches from script (fetch, XMLHttpRequest), not of documents, images, styles or scripts it loads itself.");
                }
            }
        }
        doc.rows
    }

    /// What `c` copies from a request's details: the part that is showing.
    fn net_copy_text(&self) -> Option<(String, &'static str)> {
        let net = &self.dev.net;
        let record = net.detail.as_ref().filter(|d| Some(d.request.id) == net.selected)?;
        let lines = |headers: &[(String, String)]| headers.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n");
        match net.tab {
            NetTab::Headers => {
                let text = format!(
                    "{} {}\n{}\n\n{}\n{}",
                    record.request.method,
                    record.request.url,
                    lines(&record.request_headers),
                    status_text(record),
                    lines(&record.response_headers)
                );
                Some((text, "headers"))
            }
            NetTab::Payload => record.request_body.clone().filter(|b| !b.is_empty()).map(|b| (b, "request body")),
            NetTab::Response if record.request.kind == "ws" => {
                let text = record.frames.iter().map(|f| format!("{} {}", if f.out { "↑" } else { "↓" }, f.data)).collect::<Vec<_>>().join("\n");
                Some((text, "messages")).filter(|(t, _)| !t.is_empty())
            }
            NetTab::Response => record.response_body.clone().filter(|b| !b.is_empty()).map(|b| (b, "response body")),
        }
    }

    /// The selected request as a `curl` command.
    fn net_curl(&self) -> Option<String> {
        let net = &self.dev.net;
        let request = net.selected_request()?;
        let mut command = format!("curl {}", shell_quote(&request.url));
        if request.method != "GET" && !request.method.is_empty() {
            command.push_str(&format!(" -X {}", request.method));
        }
        if let Some(record) = net.detail.as_ref().filter(|d| d.request.id == request.id) {
            for (key, value) in &record.request_headers {
                command.push_str(&format!(" \\\n  -H {}", shell_quote(&format!("{key}: {value}"))));
            }
            if let Some(body) = record.request_body.as_ref().filter(|b| !b.is_empty()) {
                command.push_str(&format!(" \\\n  --data-raw {}", shell_quote(body)));
            }
        }
        Some(command)
    }

    /// The selected line of the storage list.
    pub fn storage_selected(&self) -> Option<ItemRow> {
        self.dev.storage.items(self.now()).into_iter().nth(self.dev.storage.selected)
    }

    fn storage_rows(&self, width: usize) -> Vec<Row> {
        let storage = &self.dev.storage;
        let (Some(data), Some(item)) = (&storage.data, self.storage_selected()) else { return Vec::new() };
        let mut doc = Doc::new(width);
        let yes_no = |on: bool| if on { ("Yes".to_string(), Tone::Good) } else { ("No".to_string(), Tone::Dim) };
        let value = |doc: &mut Doc, item: &StorageItem, title: &str| {
            doc.heading(title);
            doc.body(&item.value);
            if item.size > item.value.chars().count() as u64 {
                doc.blank();
                doc.note(&format!("Cut short: the whole value is {} characters.", item.size));
            }
        };
        match item.source {
            ItemRef::Cookie(index) => {
                let Some(cookie) = data.cookies.get(index) else { return Vec::new() };
                let (secure, secure_tone) = yes_no(cookie.secure);
                let (http, http_tone) = yes_no(cookie.http_only);
                let same_site = if cookie.same_site.is_empty() { "Not set".to_string() } else { cookie.same_site.clone() };
                let decoded = percent_decode(&cookie.value.replace('+', "%2B"));
                doc.pairs(&[
                    ("Name".to_string(), cookie.name.clone(), Tone::Text),
                    ("Domain".to_string(), cookie.domain.clone(), Tone::Text),
                    ("Path".to_string(), cookie.path.clone(), Tone::Text),
                    ("Expires".to_string(), expiry(cookie, self.now(), true), Tone::Text),
                    ("Size".to_string(), size((cookie.name.len() + cookie.value.len()) as u64), Tone::Text),
                    ("Secure".to_string(), secure, secure_tone),
                    ("HttpOnly".to_string(), http, http_tone),
                    ("SameSite".to_string(), same_site, Tone::Text),
                ]);
                doc.heading("Value");
                if cookie.value.is_empty() {
                    doc.note("Empty.");
                } else {
                    doc.body(&cookie.value);
                }
                if decoded != cookie.value {
                    doc.heading("Decoded");
                    doc.body(&decoded);
                }
            }
            ItemRef::Local(index) | ItemRef::Session(index) => {
                let items = if matches!(item.source, ItemRef::Local(_)) { &data.local } else { &data.session };
                let Some(stored) = items.get(index) else { return Vec::new() };
                doc.pairs(&[
                    ("Key".to_string(), stored.key.clone(), Tone::Text),
                    ("Origin".to_string(), data.origin.clone(), Tone::Text),
                    ("Size".to_string(), size(stored.size), Tone::Text),
                ]);
                value(&mut doc, stored, "Value");
            }
            ItemRef::Store(d, store) => {
                let Some(database) = data.databases.get(d) else { return Vec::new() };
                let mut pairs = vec![
                    ("Database".to_string(), database.name.clone(), Tone::Text),
                    ("Version".to_string(), database.version.to_string(), Tone::Text),
                    ("Origin".to_string(), data.origin.clone(), Tone::Text),
                ];
                let store = store.and_then(|s| database.stores.get(s));
                if let Some(store) = store {
                    pairs.push(("Store".to_string(), store.name.clone(), Tone::Text));
                    pairs.push(("Records".to_string(), store.count.to_string(), Tone::Text));
                }
                doc.pairs(&pairs);
                if let Some(store) = store {
                    for record in &store.sample {
                        value(&mut doc, record, &record.key);
                    }
                    if store.count > store.sample.len() as u64 {
                        doc.blank();
                        doc.note(&format!("The first {} of {} records.", store.sample.len(), store.count));
                    }
                }
            }
            ItemRef::Cache(index) => {
                let Some(cache) = data.caches.get(index) else { return Vec::new() };
                doc.pairs(&[
                    ("Cache".to_string(), cache.name.clone(), Tone::Text),
                    ("Origin".to_string(), data.origin.clone(), Tone::Text),
                    ("Entries".to_string(), cache.count.to_string(), Tone::Text),
                ]);
                if !cache.urls.is_empty() {
                    doc.heading("Entries");
                    for url in &cache.urls {
                        doc.rows.extend(wrap_spans(&[(format!("  {url}"), Tone::Text)], width, 4));
                    }
                    if cache.count > cache.urls.len() as u64 {
                        doc.blank();
                        doc.note(&format!("The first {} of {} entries.", cache.urls.len(), cache.count));
                    }
                }
            }
        }
        doc.rows
    }

    /// The value `c` copies from the storage list.
    fn storage_copy_text(&self) -> Option<String> {
        let data = self.dev.storage.data.as_ref()?;
        Some(match self.storage_selected()?.source {
            ItemRef::Cookie(index) => data.cookies.get(index)?.value.clone(),
            ItemRef::Local(index) => data.local.get(index)?.value.clone(),
            ItemRef::Session(index) => data.session.get(index)?.value.clone(),
            ItemRef::Store(d, store) => {
                let database = data.databases.get(d)?;
                let store = database.stores.get(store?)?;
                store.sample.iter().map(|r| format!("{}: {}", r.key, r.value)).collect::<Vec<_>>().join("\n")
            }
            ItemRef::Cache(index) => data.caches.get(index)?.urls.join("\n"),
        })
    }

    // ----- keys -----

    fn copy(&mut self, text: String, what: &str) {
        self.request(Request::ClipboardSet { text });
        self.message = Some(format!("Copied the {what}."));
    }

    /// Cmd+C, and `c`: copies what the selection is about.
    pub(crate) fn dev_copy(&mut self) {
        if let Some(edit) = &self.dev.search {
            let text = edit.text.clone();
            return self.request(Request::ClipboardSet { text });
        }
        match self.dev.tool {
            Tool::Network if self.dev.net.detail_open => match self.net_copy_text() {
                Some((text, what)) => self.copy(text, what),
                None => self.message = Some("There is nothing here to copy.".into()),
            },
            Tool::Network => {
                if let Some(url) = self.dev.net.selected_request().map(|r| r.url.clone()) {
                    self.copy(url, "address");
                }
            }
            Tool::Storage if self.dev.storage.picking => {
                let name = self.dev.storage.picker_sites().get(self.dev.storage.pick_selected).map(|s| s.name.clone());
                if let Some(name) = name {
                    self.copy(name, "site's name");
                }
            }
            Tool::Storage => {
                if let Some(text) = self.storage_copy_text() {
                    self.copy(text, "value");
                }
            }
        }
    }

    pub(crate) fn dev_paste(&mut self, text: &str) {
        if let Some(edit) = self.dev.search.as_mut() {
            edit.insert(text);
            self.search_changed();
        }
    }

    /// The text the filter field edits, for the list that is showing.
    fn search_text(&mut self) -> &mut String {
        match self.dev.tool {
            Tool::Network => &mut self.dev.net.search,
            Tool::Storage if self.dev.storage.picking => &mut self.dev.storage.pick_search,
            Tool::Storage => &mut self.dev.storage.search,
        }
    }

    fn search_changed(&mut self) {
        let text = self.dev.search.as_ref().map(|e| e.text.clone()).unwrap_or_default();
        *self.search_text() = text;
        match self.dev.tool {
            Tool::Network => self.dev.net.scroll = 0,
            Tool::Storage if self.dev.storage.picking => self.dev.storage.pick_selected = 0,
            Tool::Storage => self.dev.storage.selected = 0,
        }
    }

    fn start_search(&mut self) {
        let text = self.search_text().clone();
        self.dev.search = Some(UrlEdit::holding(&text));
    }

    pub(crate) fn handle_dev_key(&mut self, key: KeyEvent) {
        if let Some(confirm) = self.dev.confirm.take() {
            if key.code == KeyCode::Char('y') {
                self.dev_confirmed(confirm);
            }
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.dev.search.is_some() {
            match key.code {
                KeyCode::Enter => self.dev.search = None,
                KeyCode::Esc => {
                    self.dev.search = None;
                    self.search_text().clear();
                }
                KeyCode::Char('c') if ctrl => {
                    self.dev.search = None;
                    self.search_text().clear();
                }
                _ => {
                    if let Some(edit) = self.dev.search.as_mut() {
                        edit.key(key);
                    }
                    self.search_changed();
                }
            }
            return;
        }
        self.message = None;
        match key.code {
            KeyCode::Char('c') if ctrl => return self.close_devtools(),
            KeyCode::Char('q') | KeyCode::Char('I') => return self.close_devtools(),
            KeyCode::Tab | KeyCode::BackTab => {
                let next = if self.dev.tool == Tool::Network { Tool::Storage } else { Tool::Network };
                return self.set_tool(next);
            }
            KeyCode::Char('1') => return self.set_tool(Tool::Network),
            KeyCode::Char('2') => return self.set_tool(Tool::Storage),
            KeyCode::Char('m') => return self.toggle_dock(),
            KeyCode::Char('+') | KeyCode::Char('=') => return self.resize_pane(5),
            KeyCode::Char('-') => return self.resize_pane(-5),
            KeyCode::Char('?') => return self.open_help(),
            KeyCode::Char('/') => return self.start_search(),
            KeyCode::Char('c') => return self.dev_copy(),
            _ => {}
        }
        // The details scroll the same way in both tools.
        if self.dev_detail_open() {
            let page = self.dev_detail_text().height.max(1) as i64;
            let delta = match key.code {
                KeyCode::Char('j') | KeyCode::Down => Some(1),
                KeyCode::Char('k') | KeyCode::Up => Some(-1),
                KeyCode::Char('d') if ctrl => Some(page),
                KeyCode::PageDown | KeyCode::Char(' ') => Some(page),
                KeyCode::Char('u') if ctrl => Some(-page),
                KeyCode::PageUp => Some(-page),
                KeyCode::Char('g') | KeyCode::Home => Some(i64::MIN / 2),
                KeyCode::Char('G') | KeyCode::End => Some(i64::MAX / 2),
                _ => None,
            };
            if let Some(delta) = delta {
                // `dev_prepare` keeps it within the text.
                self.dev.detail_scroll = (self.dev.detail_scroll as i64).saturating_add(delta).clamp(0, self.dev.rows.len() as i64) as usize;
                return;
            }
        }
        match self.dev.tool {
            Tool::Network => self.net_key(key),
            Tool::Storage => self.storage_key(key),
        }
    }

    /// How far a key moves the selection of a list, if it moves it.
    fn list_delta(&self, key: KeyEvent) -> Option<i64> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.dev_list().height.max(1) as i64;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => Some(1),
            KeyCode::Char('k') | KeyCode::Up => Some(-1),
            KeyCode::Char('d') if ctrl => Some(page),
            KeyCode::PageDown => Some(page),
            KeyCode::Char('u') if ctrl => Some(-page),
            KeyCode::PageUp => Some(-page),
            KeyCode::Char('g') | KeyCode::Home => Some(i64::MIN / 2),
            KeyCode::Char('G') | KeyCode::End => Some(i64::MAX / 2),
            _ => None,
        }
    }

    fn net_move(&mut self, delta: i64) {
        let visible = self.dev.net.visible();
        if visible.is_empty() {
            return;
        }
        let net = &mut self.dev.net;
        let position = net.selected.and_then(|id| visible.iter().position(|&i| net.requests[i].id == id)).unwrap_or(0);
        let position = stepped(position, delta, visible.len());
        // At the end, the selection keeps up with new requests.
        net.follow = delta > 1_000_000;
        net.selected = Some(net.requests[visible[position]].id);
        self.dev.detail_scroll = 0;
        self.net_settle();
    }

    fn net_key(&mut self, key: KeyEvent) {
        if self.dev.net.detail_open {
            match key.code {
                KeyCode::Esc | KeyCode::Backspace => self.dev.net.detail_open = false,
                KeyCode::Char('h') | KeyCode::Left => self.net_step_tab(-1),
                KeyCode::Char('l') | KeyCode::Right => self.net_step_tab(1),
                KeyCode::Char('J') => self.net_move(1),
                KeyCode::Char('K') => self.net_move(-1),
                KeyCode::Char('C') => self.net_copy_curl(),
                KeyCode::Char('r') => self.reload(),
                _ => {}
            }
            return;
        }
        if let Some(delta) = self.list_delta(key) {
            return self.net_move(delta);
        }
        match key.code {
            KeyCode::Esc if !self.dev.net.search.is_empty() => self.dev.net.search.clear(),
            KeyCode::Esc if self.dev.net.filter != NetFilter::All => self.dev.net.filter = NetFilter::All,
            KeyCode::Esc => self.dev.focused = false,
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right if self.dev.net.selected.is_some() => self.dev.net.detail_open = true,
            KeyCode::Char('f') => self.net_step_filter(1),
            KeyCode::Char('F') => self.net_step_filter(-1),
            KeyCode::Char('x') => {
                if let Some(Some(tab_id)) = self.dev.watch_sent {
                    self.request(Request::NetClear { tab_id });
                }
                self.dev.net.forget();
            }
            KeyCode::Char('p') => {
                self.dev.net.preserve = !self.dev.net.preserve;
                self.request(Request::NetPreserve { enabled: self.dev.net.preserve });
                self.message = Some(if self.dev.net.preserve { "Requests are kept when the tab goes to another page." } else { "Requests are cleared when the tab goes to another page." }.into());
            }
            KeyCode::Char('C') => self.net_copy_curl(),
            KeyCode::Char('r') => self.reload(),
            _ => {}
        }
    }

    fn net_copy_curl(&mut self) {
        if let Some(command) = self.net_curl() {
            self.copy(command, "request as a curl command");
        }
    }

    fn net_step_tab(&mut self, delta: i64) {
        let at = NetTab::ALL.iter().position(|t| *t == self.dev.net.tab).unwrap_or(0) as i64;
        self.dev.net.tab = NetTab::ALL[(at + delta).rem_euclid(NetTab::ALL.len() as i64) as usize];
        self.dev.detail_scroll = 0;
    }

    fn net_step_filter(&mut self, delta: i64) {
        let at = NetFilter::ALL.iter().position(|f| *f == self.dev.net.filter).unwrap_or(0) as i64;
        self.dev.net.filter = NetFilter::ALL[(at + delta).rem_euclid(NetFilter::ALL.len() as i64) as usize];
        self.dev.net.scroll = 0;
    }

    fn storage_step_kind(&mut self, delta: i64) {
        let at = StorageKind::ALL.iter().position(|k| *k == self.dev.storage.kind).unwrap_or(0) as i64;
        self.set_storage_kind(StorageKind::ALL[(at + delta).rem_euclid(StorageKind::ALL.len() as i64) as usize]);
    }

    fn set_storage_kind(&mut self, kind: StorageKind) {
        let storage = &mut self.dev.storage;
        if storage.kind != kind {
            storage.kind = kind;
            storage.selected = 0;
            storage.scroll = 0;
            storage.detail_open = false;
        }
    }

    fn storage_move(&mut self, delta: i64) {
        let len = self.dev.storage.items(self.now()).len();
        self.dev.storage.selected = stepped(self.dev.storage.selected, delta, len);
        self.storage_settle();
    }

    fn storage_key(&mut self, key: KeyEvent) {
        if self.dev.storage.picking {
            return self.picker_key(key);
        }
        if self.dev.storage.detail_open {
            match key.code {
                KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => self.dev.storage.detail_open = false,
                KeyCode::Char('J') => self.storage_move(1),
                KeyCode::Char('K') => self.storage_move(-1),
                KeyCode::Char('x') | KeyCode::Delete => self.storage_delete_selected(),
                KeyCode::Char('r') => self.dev.storage.stale = true,
                _ => {}
            }
            return;
        }
        if let Some(delta) = self.list_delta(key) {
            return self.storage_move(delta);
        }
        match key.code {
            KeyCode::Esc if !self.dev.storage.search.is_empty() => self.dev.storage.search.clear(),
            KeyCode::Esc => self.dev.focused = false,
            KeyCode::Enter if self.storage_selected().is_some() => self.dev.storage.detail_open = true,
            KeyCode::Char('h') | KeyCode::Left => self.storage_step_kind(-1),
            KeyCode::Char('l') | KeyCode::Right => self.storage_step_kind(1),
            KeyCode::Char('s') => {
                let storage = &mut self.dev.storage;
                let site = storage.site_name();
                storage.picking = true;
                storage.pick_search.clear();
                storage.pick_selected = storage.sites.iter().position(|s| s.name == site).unwrap_or(0);
            }
            KeyCode::Char('r') => self.dev.storage.stale = true,
            KeyCode::Char('x') | KeyCode::Delete => self.storage_delete_selected(),
            KeyCode::Char('X') if self.dev.storage.count(self.dev.storage.kind).unwrap_or(0) > 0 => self.dev.confirm = Some(Confirm::ClearKind),
            _ => {}
        }
    }

    fn picker_key(&mut self, key: KeyEvent) {
        if let Some(delta) = self.list_delta(key) {
            let len = self.dev.storage.picker_sites().len();
            self.dev.storage.pick_selected = stepped(self.dev.storage.pick_selected, delta, len);
            return self.storage_settle();
        }
        let chosen = self.dev.storage.picker_sites().get(self.dev.storage.pick_selected).map(|s| (s.name.clone(), s.current));
        match key.code {
            KeyCode::Esc if !self.dev.storage.pick_search.is_empty() => self.dev.storage.pick_search.clear(),
            KeyCode::Esc | KeyCode::Char('s') => self.dev.storage.picking = false,
            KeyCode::Enter => {
                if let Some((name, current)) = chosen {
                    let storage = &mut self.dev.storage;
                    // The page's own site is followed as the tab moves on.
                    storage.site = (!current).then_some(name);
                    storage.picking = false;
                    storage.selected = 0;
                    storage.scroll = 0;
                    storage.search.clear();
                    storage.detail_open = false;
                }
            }
            KeyCode::Char('r') => self.dev.storage.stale = true,
            KeyCode::Char('X') => {
                if let Some((name, _)) = chosen {
                    self.dev.confirm = Some(Confirm::ClearSite(name));
                }
            }
            _ => {}
        }
    }

    /// What the pane asks before deleting more than one thing.
    pub fn dev_question(&self) -> Option<String> {
        let storage = &self.dev.storage;
        Some(match self.dev.confirm.as_ref()? {
            Confirm::ClearKind => {
                let count = storage.count(storage.kind).unwrap_or(0);
                format!("Delete {} of {}?", storage.kind.counted(count), storage.site_name())
            }
            Confirm::ClearSite(site) => format!("Delete everything {site} stores, cookies included?"),
        })
    }

    fn storage_delete(&mut self, kind: &str, site: Option<String>, name: Option<String>, domain: Option<String>, path: Option<String>, all: bool) {
        let Some(Some(tab_id)) = self.dev.watch_sent else { return };
        self.send(Request::StorageDelete { tab_id, kind: kind.to_string(), site, name, domain, path, all }, Pending::StorageChanged);
    }

    fn storage_delete_selected(&mut self) {
        let Some(item) = self.storage_selected() else { return };
        let Some(data) = &self.dev.storage.data else { return };
        let kind = self.dev.storage.kind.wire();
        match item.source {
            ItemRef::Cookie(index) => {
                let Some(cookie) = data.cookies.get(index).cloned() else { return };
                self.message = Some(format!("Deleted the cookie {}.", cookie.name));
                self.storage_delete(kind, None, Some(cookie.name), Some(cookie.domain), Some(cookie.path), false);
            }
            ItemRef::Local(index) | ItemRef::Session(index) => {
                let items = if matches!(item.source, ItemRef::Local(_)) { &data.local } else { &data.session };
                let Some(key) = items.get(index).map(|i| i.key.clone()) else { return };
                self.message = Some(format!("Deleted {key}."));
                self.storage_delete(kind, None, Some(key), None, None, false);
            }
            ItemRef::Store(d, _) => {
                let Some(name) = data.databases.get(d).map(|d| d.name.clone()) else { return };
                self.message = Some(format!("Deleted the database {name}."));
                self.storage_delete(kind, None, Some(name), None, None, false);
            }
            ItemRef::Cache(index) => {
                let Some(name) = data.caches.get(index).map(|c| c.name.clone()) else { return };
                self.message = Some(format!("Deleted the cache {name}."));
                self.storage_delete(kind, None, Some(name), None, None, false);
            }
        }
    }

    fn dev_confirmed(&mut self, confirm: Confirm) {
        match confirm {
            Confirm::ClearKind => {
                let storage = &self.dev.storage;
                let (kind, site) = (storage.kind, storage.site_name());
                self.message = Some(format!("Deleted the {} of {site}.", kind.long()));
                self.storage_delete(kind.wire(), Some(site), None, None, None, true);
                self.dev.storage.detail_open = false;
            }
            Confirm::ClearSite(site) => {
                self.message = Some(format!("Deleted everything {site} stored."));
                self.storage_delete("site", Some(site), None, None, None, true);
            }
        }
    }

    // ----- mouse -----

    pub(crate) fn dev_mouse(&mut self, kind: MouseEventKind, col: u16, row: u16) {
        let (column, detail) = self.dev_columns();
        let over_detail = detail.is_some_and(|area| contains(area, col, row));
        match kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let delta: i64 = if kind == MouseEventKind::ScrollDown { 3 } else { -3 };
                if over_detail {
                    self.dev.detail_scroll = (self.dev.detail_scroll as i64 + delta).max(0) as usize;
                } else if contains(column, col, row) {
                    match self.dev.tool {
                        Tool::Network => self.net_move(delta),
                        Tool::Storage if self.dev.storage.picking => {
                            let len = self.dev.storage.picker_sites().len();
                            self.dev.storage.pick_selected = stepped(self.dev.storage.pick_selected, delta, len);
                            self.storage_settle();
                        }
                        Tool::Storage => self.storage_move(delta),
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.dev.focused = true;
                self.dev.confirm = None;
                self.dev.search = None;
                self.cancel_edit();
                if let Some((tool, _)) = self.dev_tool_tabs().into_iter().find(|(_, rect)| contains(*rect, col, row)) {
                    return self.set_tool(tool);
                }
                match self.dev.tool {
                    Tool::Network => self.net_click(col, row, over_detail),
                    Tool::Storage => self.storage_click(col, row, over_detail),
                }
            }
            _ => {}
        }
    }

    fn net_click(&mut self, col: u16, row: u16, over_detail: bool) {
        let (column, detail) = self.dev_columns();
        if over_detail {
            self.dev.net.detail_open = true;
            if detail.is_some_and(|area| row == area.y + 1) {
                if let Some((tab, _, _)) = self.net_tabs().into_iter().find(|(_, x, width)| col >= *x && col < x + width) {
                    self.dev.net.tab = tab;
                    self.dev.detail_scroll = 0;
                }
            }
            return;
        }
        if self.dev.net.late && column.height > 2 && row == column.y + 1 {
            // The notice offers a reload.
            return self.reload();
        }
        if column.height > 0 && row == column.y {
            if let Some((filter, _, _)) = self.net_chips().into_iter().find(|(_, x, width)| col >= *x && col < x + width) {
                self.dev.net.filter = filter;
                self.dev.net.scroll = 0;
            }
            return;
        }
        let list = self.dev_list();
        if !contains(list, col, row) {
            return;
        }
        let visible = self.dev.net.visible();
        let Some(&index) = visible.get(self.dev.net.scroll + (row - list.y) as usize) else { return };
        let id = self.dev.net.requests[index].id;
        if self.dev.net.selected == Some(id) {
            // A second click opens what the first selected.
            self.dev.net.detail_open = true;
        } else {
            self.dev.net.selected = Some(id);
            self.dev.net.follow = false;
            self.dev.net.detail_open = false;
            self.dev.detail_scroll = 0;
        }
    }

    fn storage_click(&mut self, col: u16, row: u16, over_detail: bool) {
        let (column, _) = self.dev_columns();
        let list = self.dev_list();
        if self.dev.storage.picking {
            if contains(list, col, row) {
                let line = self.dev.storage.pick_scroll + (row - list.y) as usize;
                if line < self.dev.storage.picker_sites().len() {
                    if line == self.dev.storage.pick_selected {
                        self.picker_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                    } else {
                        self.dev.storage.pick_selected = line;
                    }
                }
            }
            return;
        }
        if over_detail {
            self.dev.storage.detail_open = self.storage_selected().is_some();
            return;
        }
        if column.height > 0 && row == column.y {
            // The line that names the site opens the site list.
            return self.storage_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        }
        if column.height > 1 && row == column.y + 1 {
            if let Some((kind, ..)) = self.storage_chips().into_iter().find(|(_, x, width, _)| col >= *x && col < x + width) {
                self.set_storage_kind(kind);
            }
            return;
        }
        if !contains(list, col, row) {
            return;
        }
        let line = self.dev.storage.scroll + (row - list.y) as usize;
        if line >= self.dev.storage.items(self.now()).len() {
            return;
        }
        if line == self.dev.storage.selected {
            self.dev.storage.detail_open = true;
        } else {
            self.dev.storage.selected = line;
            self.dev.storage.detail_open = false;
        }
    }
}

/// How many lines a set of keys takes when laid out in `width` cells.
pub fn key_rows(keys: &[(&str, &str)], width: u16) -> u16 {
    let end = width.saturating_sub(2);
    let (mut x, mut rows) = (0u16, 1u16);
    for (key, what) in keys {
        let item = (key.width() + 1 + what.width()) as u16;
        if x + item > end && x > 0 {
            x = 0;
            rows += 1;
        }
        x += item + 3;
    }
    rows
}

/// Said of a call from script that only WebKit's timing tells of.
const UNRECORDED: &str = "Only the timing of this request is known. It was made before dev tools were opened, or through a reference to fetch that the page took before then, or by a worker. Press r to reload the page: what it asks for after that is recorded in full, with headers and bodies.";

/// Whether a request was made by a script's own call.
fn scripted(request: &NetRequest) -> bool {
    matches!(request.kind.as_str(), "fetch" | "xhr" | "ping")
}

/// "200 OK", "Failed", "Pending"
pub fn status_text(record: &NetRecord) -> String {
    let request = &record.request;
    match (request.state.as_str(), request.status) {
        ("failed", _) => "Failed".to_string(),
        (_, 0) if request.state == "pending" => "Pending".to_string(),
        (_, 0) => "Not reported".to_string(),
        (_, status) if record.status_text.is_empty() => status.to_string(),
        (_, status) => format!("{status} {}", record.status_text),
    }
}

pub fn status_tone(request: &NetRequest) -> Tone {
    match (request.state.as_str(), request.status) {
        ("failed", _) => Tone::Bad,
        (_, 500..) => Tone::Bad,
        (_, 400..=499) => Tone::Warn,
        (_, 300..=399) => Tone::Accent,
        (_, 200..=299) => Tone::Good,
        _ => Tone::Dim,
    }
}

/// What the list's status column says of a request.
pub fn status_label(request: &NetRequest) -> String {
    match (request.state.as_str(), request.status) {
        ("failed", _) => "ERR".to_string(),
        ("pending", 0) => "···".to_string(),
        (_, 0) => " — ".to_string(),
        (_, status) => status.to_string(),
    }
}

fn kind_name(kind: &str) -> &str {
    match kind {
        "doc" => "Document",
        "fetch" => "Fetch",
        "xhr" => "XMLHttpRequest",
        "js" => "Script",
        "css" => "Stylesheet",
        "img" => "Image",
        "font" => "Font",
        "media" => "Media",
        "ws" => "WebSocket",
        "ping" => "Beacon",
        _ => "Other",
    }
}

/// The host of a request, when it is not the page's own.
pub fn foreign_host(url: &str, page: &str) -> Option<String> {
    let host = display_host(url);
    (!host.is_empty() && host != display_host(page) && !url.starts_with("data:")).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_requests_by_the_end_of_their_path() {
        assert_eq!(short_name("https://a.example/"), "a.example/");
        assert_eq!(short_name("https://a.example"), "a.example/");
        assert_eq!(short_name("https://a.example/css/main.css"), "main.css");
        assert_eq!(short_name("https://a.example/api/search?q=1#top"), "search?q=1");
        assert_eq!(short_name("https://a.example/api/users/"), "users");
        assert_eq!(short_name("https://a.example/?page=2"), "a.example/?page=2");
    }

    #[test]
    fn decodes_queries() {
        assert_eq!(form_pairs("q=swift+concurrency&lang=en%2Dgb&flag"), vec![
            ("q".to_string(), "swift concurrency".to_string()),
            ("lang".to_string(), "en-gb".to_string()),
            ("flag".to_string(), String::new()),
        ]);
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%E2%9C%93"), "✓");
    }

    #[test]
    fn wraps_by_width_with_a_hanging_indent() {
        let rows = wrap_spans(&[("  key  ".into(), Tone::Key), ("abcdefghij".into(), Tone::Text)], 12, 7);
        let text: Vec<String> = rows.iter().map(|row| row.iter().map(|(t, _)| t.as_str()).collect()).collect();
        assert_eq!(text, vec!["  key  abcde", "      fghij"]);
    }

    #[test]
    fn colours_json_lines() {
        let spans = json_spans(r#"  "na\"me": "x","#);
        assert_eq!(spans[1], (r#""na\"me""#.to_string(), Tone::Key));
        assert_eq!(spans[3], (r#""x","#.to_string(), Tone::Good));
        assert_eq!(json_spans("  12,")[1].1, Tone::Warn);
        assert_eq!(json_spans("  true")[1].1, Tone::Accent);
    }

    #[test]
    fn splits_the_page_area() {
        let page = Rect::new(28, 1, 92, 38);
        let (view, pane) = split_page(page, Dock::Right, 45);
        assert_eq!((view.width, pane.width, pane.x), (51, 41, 79));
        let (view, pane) = split_page(page, Dock::Bottom, 45);
        assert_eq!((view.height, pane.height, pane.y), (21, 17, 22));
        // Too small for both: the page keeps it all.
        assert_eq!(split_page(Rect::new(0, 0, 30, 8), Dock::Right, 45).1, Rect::default());
        assert_eq!(split_page(Rect::new(0, 0, 30, 8), Dock::Bottom, 45).1, Rect::default());
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(millis(12.4), "12 ms");
        assert_eq!(millis(1240.0), "1.24 s");
        assert_eq!(millis(126_000.0), "2.1 min");
    }
}
