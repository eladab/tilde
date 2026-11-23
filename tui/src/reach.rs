//! Reaching into the page from the keyboard, and looking after tabs that are
//! not in use: scrolling, following links by typed labels ("hints"), muting,
//! content blocking, reopening a closed tab, putting idle tabs to sleep, and
//! telling the app where the icons of sites go.

use crate::app::{App, Pending, SideRow, TabEntry};
use crate::protocol::{IconCell, Mode, Request, Response};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a tab is left alone before its page is let go, unless
/// `settings.json` says otherwise.
pub const DEFAULT_DISCARD_MINUTES: u64 = 30;
/// How often idle tabs are looked for.
const DISCARD_CHECK: Duration = Duration::from_secs(10);
/// How many closed tabs can be reopened.
const CLOSED_LIMIT: usize = 25;

/// Labels are on the page, waiting for one to be typed.
#[derive(Debug, Clone, PartialEq)]
pub struct Hints {
    pub tab_id: u64,
    pub typed: String,
    /// A link is opened in a new tab behind this one.
    pub new_tab: bool,
    /// How many labels still match; unknown until the page has answered.
    pub count: Option<u64>,
    /// The page had the keys when the labels went up, and gets them back.
    return_to_page: bool,
}

/// A tab that was closed, and can be opened again as it was.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedTab {
    pub url: String,
    pub title: String,
    pub name: Option<String>,
    /// The name the app keeps the tab's state under.
    pub state: Option<String>,
}

/// After how long an idle tab is put to sleep, read from `settings.json`
/// beside the session: `{"discard_after_minutes": 30}`. Zero means never.
pub fn discard_after(session: Option<&Path>) -> Option<Duration> {
    let minutes = session
        .and_then(|path| std::fs::read_to_string(path.with_file_name("settings.json")).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value["discard_after_minutes"].as_u64())
        .unwrap_or(DEFAULT_DISCARD_MINUTES);
    (minutes > 0).then(|| Duration::from_secs(minutes * 60))
}

/// The part of an address that names its site: scheme and host.
fn site_of(url: &str) -> Option<&str> {
    let after = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host_len = after.find(['/', '?', '#']).unwrap_or(after.len());
    (host_len > 0).then(|| &url[..url.len() - after.len() + host_len])
}

impl App {
    // ----- scrolling -----

    /// Scrolls the page from NAV mode, without giving it the keys.
    pub(crate) fn scroll_page(&mut self, how: &str) {
        if let Some(tab_id) = self.current_tab_id().filter(|_| self.page_visible()) {
            self.request(Request::PageScroll { tab_id, how: how.to_string() });
        }
    }

    // ----- hints -----

    /// Whether keys typed in the terminal are letters of a label.
    pub fn hints_have_keys(&self) -> bool {
        self.hints.is_some() && self.mode == Mode::Nav && self.edit.is_none() && !self.overlay_open()
    }

    /// Puts a label on everything in the page that can be followed.
    pub fn start_hints(&mut self, new_tab: bool) {
        let Some(tab_id) = self.current_tab_id().filter(|_| self.page_visible()) else {
            self.message = Some("There is no page to follow a link in.".into());
            return;
        };
        self.cancel_edit();
        self.stop_hints();
        self.message = None;
        let return_to_page = self.mode == Mode::Page;
        if return_to_page {
            self.request(Request::FocusSet { mode: Mode::Nav });
        }
        self.hints = Some(Hints { tab_id, typed: String::new(), new_tab, count: None, return_to_page });
        self.send(Request::HintsStart { tab_id }, Pending::HintsStart { tab_id });
    }

    /// Takes the labels off the page.
    pub fn stop_hints(&mut self) {
        let Some(hints) = self.hints.take() else { return };
        if self.index_of_tab(hints.tab_id).is_some() || self.stashed_tab(hints.tab_id).is_some() {
            self.request(Request::HintsStop { tab_id: hints.tab_id });
        }
        if hints.return_to_page && self.mode == Mode::Nav && !self.overlay_open() {
            self.focus_page();
        }
    }

    /// The labels belong to the tab that was showing; they go with it.
    pub(crate) fn hints_follow_tab(&mut self) {
        let showing = self.current_tab_id();
        if self.hints.as_ref().is_some_and(|h| Some(h.tab_id) != showing) {
            self.drop_hints();
        }
    }

    /// Takes the labels off the page and leaves the keys where they are.
    pub(crate) fn drop_hints(&mut self) {
        if let Some(hints) = self.hints.as_mut() {
            hints.return_to_page = false;
        }
        self.stop_hints();
    }

    pub(crate) fn handle_hint_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(hints) = self.hints.as_mut() else { return };
        match key.code {
            KeyCode::Esc => return self.stop_hints(),
            KeyCode::Char('c') if ctrl => return self.stop_hints(),
            KeyCode::Backspace if hints.typed.is_empty() => return self.stop_hints(),
            KeyCode::Backspace => {
                hints.typed.pop();
            }
            KeyCode::Char(c) if c.is_ascii_alphabetic() && !ctrl => hints.typed.push(c.to_ascii_lowercase()),
            _ => return,
        }
        let (tab_id, typed, new_tab) = (hints.tab_id, hints.typed.clone(), hints.new_tab);
        self.send(Request::HintsType { tab_id, typed, new_tab }, Pending::HintsType { tab_id });
    }

    pub(crate) fn hints_started(&mut self, tab_id: u64, response: Response) {
        let Some(hints) = self.hints.as_mut().filter(|h| h.tab_id == tab_id) else { return };
        match response.body["count"].as_u64().filter(|_| response.ok) {
            Some(count) if count > 0 => hints.count = Some(count),
            _ => {
                self.stop_hints();
                self.message = Some("Nothing on screen to follow.".into());
            }
        }
    }

    pub(crate) fn hints_typed(&mut self, tab_id: u64, response: Response) {
        // An answer to a letter typed before the label was followed is stale.
        let Some(hints) = self.hints.as_mut().filter(|h| h.tab_id == tab_id) else { return };
        let count = response.body["count"].as_u64().unwrap_or(0);
        if response.body["done"].as_bool() == Some(true) {
            let hints = self.hints.take().expect("checked above");
            let wants_keys = response.body["focus"].as_bool() == Some(true);
            if let Some(url) = response.body["url"].as_str().filter(|u| !u.is_empty()) {
                self.open_in_background(url.to_string());
            }
            if wants_keys || hints.return_to_page {
                self.focus_page();
            }
        } else if count == 0 || !response.ok {
            self.stop_hints();
            self.message = Some("No such label.".into());
        } else {
            hints.count = Some(count);
        }
    }

    /// Opens an address in a new tab after the selected one, loading it
    /// without showing it.
    pub(crate) fn open_in_background(&mut self, url: String) {
        let key = self.new_key();
        let at = if self.tabs.is_empty() { 0 } else { (self.selected + 1).max(self.pinned_count()) };
        self.tabs.insert(at, TabEntry { key, url: url.clone(), loading: true, pending: true, ..Default::default() });
        let profile = self.engine_profile();
        self.send(Request::TabCreate { url: Some(url), input: None, activate: false, profile, state: None }, Pending::CreateQuiet { key });
        self.session_dirty = true;
        self.clamp_scroll();
    }

    // ----- sound and blocking -----

    pub fn toggle_mute(&mut self) {
        let Some(entry) = self.current() else { return };
        match entry.tab_id {
            Some(tab_id) => self.send(Request::TabMute { tab_id, muted: !entry.muted }, Pending::Notify),
            None => self.message = Some("This tab has no page to mute.".into()),
        }
    }

    pub fn toggle_blocking(&mut self) {
        match self.current_tab_id() {
            Some(tab_id) => self.send(Request::BlockingToggle { tab_id }, Pending::Blocking),
            None => self.message = Some("There is no page here.".into()),
        }
    }

    pub(crate) fn blocking_toggled(&mut self, response: Response) {
        if !response.ok {
            self.message = response.error;
            return;
        }
        let site = response.body["site"].as_str().unwrap_or("this site");
        self.message = Some(if response.body["active"].as_bool() == Some(true) {
            format!("Content blocking is on again for {site}.")
        } else {
            format!("Content blocking is off for {site}.")
        });
    }

    // ----- closed tabs -----

    /// Remembers a tab that is being closed, so that it can be reopened.
    pub(crate) fn remember_closed(&mut self, entry: &TabEntry) {
        if entry.url.is_empty() {
            return;
        }
        self.closed.push(ClosedTab { url: entry.url.clone(), title: entry.title.clone(), name: entry.name.clone(), state: entry.state.clone() });
        if self.closed.len() > CLOSED_LIMIT {
            self.closed.remove(0);
        }
    }

    /// Opens the tab closed last, as it was: its pages back and forward too.
    pub fn reopen_tab(&mut self) {
        match self.closed.len().checked_sub(1) {
            Some(last) => self.reopen_closed(last),
            None => self.message = Some("No closed tab to reopen.".into()),
        }
    }

    pub(crate) fn reopen_closed(&mut self, index: usize) {
        if index >= self.closed.len() {
            return;
        }
        let closed = self.closed.remove(index);
        self.close_overlays();
        self.cancel_edit();
        let key = self.new_key();
        let at = if self.tabs.is_empty() { 0 } else { (self.selected + 1).max(self.pinned_count()) };
        let entry = TabEntry { key, url: closed.url.clone(), title: closed.title, name: closed.name, state: closed.state, recorded_url: closed.url, ..Default::default() };
        self.tabs.insert(at, entry);
        self.selected = at;
        self.message = None;
        self.session_dirty = true;
        self.ensure_active();
    }

    // ----- sleep -----

    /// Notes which tabs are out of sight, and lets go of the pages of those
    /// that have been for long enough. The tab stays where it is, and its
    /// page comes back as it was when the tab is next shown.
    pub(crate) fn rest_idle_tabs(&mut self, now: Instant) {
        let shown = self.current().map(|t| t.key);
        for entry in self.tabs.iter_mut().chain(self.spaces.iter_mut().flat_map(|s| s.tabs.iter_mut())) {
            if Some(entry.key) == shown {
                entry.left_at = None;
            } else if entry.left_at.is_none() {
                entry.left_at = Some(now);
            }
        }
        let Some(after) = self.discard_after else { return };
        if !self.ready || self.dev.open || self.last_discard_check.is_some_and(|at| now.duration_since(at) < DISCARD_CHECK) {
            return;
        }
        self.last_discard_check = Some(now);
        self.put_to_sleep(|entry| entry.left_at.is_some_and(|at| now.duration_since(at) >= after));
    }

    /// Puts every tab but the one showing to sleep now, pinned tabs and
    /// tabs in use apart.
    pub fn sleep_other_tabs(&mut self) {
        let shown = self.current().map(|t| t.key);
        let count = self.put_to_sleep(|entry| Some(entry.key) != shown);
        self.message = Some(match count {
            0 => "No other tab to put to sleep.".to_string(),
            1 => "1 tab put to sleep.".to_string(),
            n => format!("{n} tabs put to sleep."),
        });
    }

    /// Lets go of the pages of the tabs `idle` picks, and says how many.
    fn put_to_sleep(&mut self, idle: impl Fn(&TabEntry) -> bool) -> usize {
        let mut asleep = Vec::new();
        for entry in self.tabs.iter_mut().chain(self.spaces.iter_mut().flat_map(|s| s.tabs.iter_mut())) {
            // Pinned tabs are the ones kept open on purpose (mail, chat), and
            // a tab that is heard or is on a call is in use.
            let busy = entry.pinned || entry.audible || entry.camera || entry.microphone || entry.loading || entry.pending || entry.crashed;
            if let Some(tab_id) = entry.tab_id.filter(|_| !busy && entry.state.is_some() && idle(entry)) {
                entry.sleep();
                asleep.push(tab_id);
            }
        }
        let count = asleep.len();
        for tab_id in asleep {
            if self.active_sent == Some(tab_id) {
                self.active_sent = None;
            }
            self.request(Request::TabDiscard { tab_id });
            self.session_dirty = true;
        }
        count
    }

    /// Tells the app which kept states still belong to a tab.
    pub(crate) fn keep_states(&mut self) {
        let tokens: Vec<String> =
            self.tabs.iter().chain(self.spaces.iter().flat_map(|s| s.tabs.iter())).filter_map(|t| t.state.clone()).collect();
        self.request(Request::StateKeep { tokens });
    }

    // ----- icons -----

    /// The cells of the sidebar where the icons of sites go, top to bottom.
    fn icon_cells(&self) -> Vec<IconCell> {
        let list = self.layout.tab_list;
        if list.width < 12 {
            return Vec::new();
        }
        let rows = self.side_rows();
        let mut cells = Vec::new();
        for row in 0..list.height as usize {
            let Some(SideRow::Tab(index)) = rows.get(self.scroll + row).copied() else { continue };
            let Some(tab) = self.tabs.get(index) else { continue };
            // The spinner and the crash mark are drawn where the icon would be.
            if tab.loading || tab.crashed {
                continue;
            }
            if let Some(site) = site_of(&tab.url) {
                cells.push(IconCell { col: list.x + 1, row: list.y + row as u16, url: site.to_string() });
            }
        }
        cells
    }

    pub(crate) fn sync_icons(&mut self) {
        if !self.ready {
            return;
        }
        let cells = self.icon_cells();
        if cells != self.icons_sent {
            self.icons_sent = cells.clone();
            self.request(Request::IconsSet { icons: cells });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::site_of;

    #[test]
    fn sites_of_addresses() {
        assert_eq!(site_of("https://a.example/x?y"), Some("https://a.example"));
        assert_eq!(site_of("http://localhost:8080"), Some("http://localhost:8080"));
        assert_eq!(site_of("about:blank"), None);
        assert_eq!(site_of("https:///x"), None);
    }
}
