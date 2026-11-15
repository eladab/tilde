//! Browser state and behaviour. No terminal I/O happens here: keys and engine
//! messages come in, requests go out through a `Transport`.

use crate::protocol::{self, Event, Incoming, Mode, Request, Response, TabState};
use crate::resolve::display_host;
use crate::session::{self, Session, SessionTab};
use crate::transport::Transport;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

pub const SIDEBAR_WIDTH: u16 = 28;
pub const HOME_URL: &str = "https://webkit.org";

#[derive(Debug, Clone, Default)]
pub struct TabEntry {
    /// Stable local identity; `tab_id` only exists once the app has created the tab.
    pub key: u64,
    pub tab_id: Option<u64>,
    /// A `tab.create` for this entry is in flight.
    pub pending: bool,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub progress: f64,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub crashed: bool,
}

impl TabEntry {
    pub fn label(&self) -> String {
        if !self.title.is_empty() {
            self.title.clone()
        } else if !self.url.is_empty() {
            display_host(&self.url)
        } else {
            "New tab".to_string()
        }
    }

    fn is_blank(&self) -> bool {
        self.tab_id.is_none() && !self.pending && self.url.is_empty()
    }

    fn apply(&mut self, state: &TabState) -> bool {
        let mut changed = false;
        if let Some(url) = &state.url {
            // The app reports an empty URL before the first navigation commits.
            if !url.is_empty() && *url != self.url {
                self.url = url.clone();
                changed = true;
            }
        }
        if let Some(title) = &state.title {
            if *title != self.title && !(title.is_empty() && state.loading == Some(true)) {
                self.title = title.clone();
                changed = true;
            }
        }
        if let Some(loading) = state.loading {
            self.loading = loading;
            if loading {
                self.crashed = false;
            }
        }
        if let Some(progress) = state.progress {
            self.progress = progress;
        }
        if let Some(v) = state.can_go_back {
            self.can_go_back = v;
        }
        if let Some(v) = state.can_go_forward {
            self.can_go_forward = v;
        }
        changed
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UrlEdit {
    pub text: String,
    /// Cursor position in characters.
    pub cursor: usize,
    /// The text is a prefilled URL that the first typed character replaces.
    pub replace_on_type: bool,
    /// Entry to return to if a brand-new tab is cancelled.
    return_to: Option<u64>,
}

impl UrlEdit {
    fn byte_index(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn insert(&mut self, s: &str) {
        if self.replace_on_type {
            self.text.clear();
            self.cursor = 0;
            self.replace_on_type = false;
        }
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        let at = self.byte_index(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }

    fn delete_range(&mut self, from: usize, to: usize) {
        let (a, b) = (self.byte_index(from), self.byte_index(to));
        self.text.replace_range(a..b, "");
        self.cursor = from;
    }

    fn backspace(&mut self) {
        if self.replace_on_type {
            self.text.clear();
            self.cursor = 0;
            self.replace_on_type = false;
        } else if self.cursor > 0 {
            self.delete_range(self.cursor - 1, self.cursor);
        }
    }

    fn delete_word(&mut self) {
        self.replace_on_type = false;
        let chars: Vec<char> = self.text.chars().collect();
        let mut start = self.cursor;
        while start > 0 && chars[start - 1] == ' ' {
            start -= 1;
        }
        while start > 0 && !matches!(chars[start - 1], ' ' | '/' | '.') {
            start -= 1;
        }
        if start == self.cursor && start > 0 {
            start -= 1;
        }
        self.delete_range(start, self.cursor);
    }
}

#[derive(Debug)]
enum Pending {
    Hello,
    TabList,
    Create { key: u64 },
    Plain,
}

/// Where things are on screen, for drawing and for mouse hit-testing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScreenLayout {
    pub sidebar: Rect,
    pub tab_list: Rect,
    pub new_tab: Rect,
    pub toolbar: Rect,
    pub back: Rect,
    pub forward: Rect,
    pub reload: Rect,
    pub url: Rect,
    pub viewport: Rect,
    pub status: Rect,
}

impl ScreenLayout {
    pub fn compute(area: Rect) -> Self {
        let status_h = area.height.min(1);
        let main_h = area.height - status_h;
        let side_w = SIDEBAR_WIDTH.min(area.width);
        let right_x = area.x + side_w;
        let right_w = area.width - side_w;
        let toolbar_h = main_h.min(1);

        let sidebar = Rect::new(area.x, area.y, side_w, main_h);
        // Row 0 is the header, the last row is "+ New tab"; tabs go between.
        let list_h = main_h.saturating_sub(3);
        let tab_list = Rect::new(area.x, area.y + main_h.min(2), side_w.saturating_sub(1), list_h);
        let new_tab = Rect::new(area.x, area.y + main_h.saturating_sub(1), side_w.saturating_sub(1), main_h.min(1));
        let toolbar = Rect::new(right_x, area.y, right_w, toolbar_h);
        let button = |offset: u16| Rect::new(right_x + offset.min(right_w), area.y, 3.min(right_w.saturating_sub(offset)), toolbar_h);
        let url_x = 11.min(right_w);
        Self {
            sidebar,
            tab_list,
            new_tab,
            toolbar,
            back: button(1),
            forward: button(4),
            reload: button(7),
            url: Rect::new(right_x + url_x, area.y, right_w - url_x, toolbar_h),
            viewport: Rect::new(right_x, area.y + toolbar_h, right_w, main_h - toolbar_h),
            status: Rect::new(area.x, area.y + main_h, area.width, status_h),
        }
    }
}

fn contains(rect: Rect, col: u16, row: u16) -> bool {
    rect.width > 0 && rect.height > 0 && col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}

pub struct App {
    pub tabs: Vec<TabEntry>,
    pub selected: usize,
    pub mode: Mode,
    pub edit: Option<UrlEdit>,
    pub message: Option<String>,
    pub should_quit: bool,
    /// Tabs are known and the UI reflects the engine.
    pub ready: bool,
    pub layout: ScreenLayout,
    /// First tab row shown in the sidebar.
    pub scroll: usize,
    list_height: u16,
    pub session_dirty: bool,
    pub session_path: Option<PathBuf>,

    transport: Box<dyn Transport>,
    inbox: VecDeque<String>,
    pending: HashMap<u64, Pending>,
    next_request: u64,
    next_key: u64,
    instance: Option<u64>,
    /// The tab the app is currently showing, as far as we have told it.
    active_sent: Option<u64>,
    visible_sent: Option<bool>,
    viewport_sent: Option<Rect>,
    /// Entries closed while their `tab.create` was still in flight.
    orphans: HashSet<u64>,
    /// Entry that should take page focus once its tab exists.
    focus_on_create: Option<u64>,
}

impl App {
    pub fn new(transport: Box<dyn Transport>, session_path: Option<PathBuf>) -> Self {
        Self {
            tabs: Vec::new(),
            selected: 0,
            mode: Mode::Nav,
            edit: None,
            message: None,
            should_quit: false,
            ready: false,
            layout: ScreenLayout::default(),
            scroll: 0,
            list_height: 0,
            session_dirty: false,
            session_path,
            transport,
            inbox: VecDeque::new(),
            pending: HashMap::new(),
            next_request: 1,
            next_key: 1,
            instance: None,
            active_sent: None,
            visible_sent: None,
            viewport_sent: None,
            orphans: HashSet::new(),
            focus_on_create: None,
        }
    }

    /// Begins the handshake: `hello`, then `tab.list`, then session restore.
    pub fn start(&mut self) {
        self.send(Request::Hello { protocol: protocol::PROTOCOL_VERSION }, Pending::Hello);
        self.pump();
    }

    // ----- plumbing -----

    fn send(&mut self, request: Request, pending: Pending) {
        let id = self.next_request;
        self.next_request += 1;
        self.pending.insert(id, pending);
        let line = protocol::encode(id, &request);
        let replies = self.transport.send(&line);
        self.inbox.extend(replies);
    }

    fn request(&mut self, request: Request) {
        self.send(request, Pending::Plain);
    }

    fn pump(&mut self) {
        while let Some(line) = self.inbox.pop_front() {
            self.dispatch(&line);
        }
    }

    /// A line arrived from the engine.
    pub fn handle_line(&mut self, line: &str) {
        self.dispatch(line);
        self.pump();
    }

    fn dispatch(&mut self, line: &str) {
        match protocol::decode(line) {
            Ok(Incoming::Response(response)) => self.on_response(response),
            Ok(Incoming::Event(event)) => self.on_event(event),
            Err(error) => crate::log(&format!("undecodable line ({error}): {line}")),
        }
    }

    fn new_key(&mut self) -> u64 {
        let key = self.next_key;
        self.next_key += 1;
        key
    }

    pub fn current(&self) -> Option<&TabEntry> {
        self.tabs.get(self.selected)
    }

    fn current_tab_id(&self) -> Option<u64> {
        self.current().and_then(|t| t.tab_id)
    }

    fn index_of_key(&self, key: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.key == key)
    }

    fn index_of_tab(&self, tab_id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.tab_id == Some(tab_id))
    }

    /// Whether the page is on screen (as opposed to a placeholder).
    pub fn page_visible(&self) -> bool {
        self.visible_sent == Some(true)
    }

    // ----- layout -----

    /// Called before every draw with the terminal size.
    pub fn set_area(&mut self, area: Rect) {
        self.layout = ScreenLayout::compute(area);
        let viewport = self.layout.viewport;
        if self.viewport_sent != Some(viewport) && viewport.width > 0 && viewport.height > 0 {
            self.viewport_sent = Some(viewport);
            self.request(Request::ViewportSet { col: viewport.x, row: viewport.y, cols: viewport.width, rows: viewport.height });
            self.pump();
        }
        // Keep the selection in view after a resize, but otherwise leave
        // the sidebar where the user scrolled it.
        if self.list_height != self.layout.tab_list.height {
            self.list_height = self.layout.tab_list.height;
            self.clamp_scroll();
        }
    }

    fn clamp_scroll(&mut self) {
        let height = self.layout.tab_list.height as usize;
        if height == 0 {
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + height {
            self.scroll = self.selected + 1 - height;
        }
        let max = self.tabs.len().saturating_sub(height);
        self.scroll = self.scroll.min(max);
    }

    // ----- engine messages -----

    fn on_response(&mut self, response: Response) {
        let Some(pending) = self.pending.remove(&response.id) else { return };
        match pending {
            Pending::Hello => {
                if !response.ok {
                    self.message = Some(format!("app refused connection: {}", response.error.unwrap_or_default()));
                    return;
                }
                if let Ok(mode) = serde_json::from_value::<Mode>(response.body["mode"].clone()) {
                    self.mode = mode;
                }
                self.instance = response.body["instance"].as_u64();
                self.send(Request::TabList, Pending::TabList);
            }
            Pending::TabList => {
                let live: Vec<TabState> = serde_json::from_value(response.body["tabs"].clone()).unwrap_or_default();
                let active = response.body["active"].as_u64();
                self.restore(live, active);
            }
            Pending::Create { key } => {
                let tab_id = response.body["tab_id"].as_u64();
                if self.orphans.remove(&key) {
                    if let Some(tab_id) = tab_id {
                        self.request(Request::TabClose { tab_id });
                    }
                    return;
                }
                let Some(index) = self.index_of_key(key) else { return };
                self.tabs[index].pending = false;
                match tab_id.filter(|_| response.ok) {
                    Some(tab_id) => {
                        self.tabs[index].tab_id = Some(tab_id);
                        // `tab.create` was sent with activate, so the app shows it now.
                        self.active_sent = Some(tab_id);
                        self.session_dirty = true;
                        self.ensure_active();
                        if self.focus_on_create == Some(key) {
                            self.focus_on_create = None;
                            if self.selected == index {
                                self.request(Request::FocusSet { mode: Mode::Page });
                            }
                        }
                    }
                    None => self.message = Some(format!("could not open tab: {}", response.error.unwrap_or_default())),
                }
            }
            Pending::Plain => {
                if !response.ok {
                    crate::log(&format!("request {} failed: {:?}", response.id, response.error));
                }
            }
        }
    }

    /// Builds the tab list from the session file and whatever the app still has loaded.
    fn restore(&mut self, live: Vec<TabState>, live_active: Option<u64>) {
        let saved = self.session_path.as_deref().and_then(session::load).unwrap_or_default();
        let same_instance = saved.instance.is_some() && saved.instance == self.instance;
        let mut unclaimed: Vec<TabState> = live;
        self.tabs.clear();

        for tab in &saved.tabs {
            if tab.url.is_empty() {
                continue;
            }
            let mut entry = TabEntry { key: self.new_key(), url: tab.url.clone(), title: tab.title.clone(), ..Default::default() };
            if same_instance {
                if let Some(pos) = tab.tab_id.and_then(|id| unclaimed.iter().position(|s| s.tab_id == id)) {
                    let state = unclaimed.remove(pos);
                    entry.tab_id = Some(state.tab_id);
                    entry.apply(&state);
                }
            }
            self.tabs.push(entry);
        }
        // Tabs the app has that the session did not know about.
        for state in unclaimed {
            let mut entry = TabEntry { key: self.new_key(), tab_id: Some(state.tab_id), ..Default::default() };
            entry.apply(&state);
            self.tabs.push(entry);
        }
        if self.tabs.is_empty() {
            let key = self.new_key();
            self.tabs.push(TabEntry { key, url: HOME_URL.to_string(), ..Default::default() });
        }

        self.selected = saved.active.min(self.tabs.len() - 1);
        self.active_sent = live_active;
        self.ready = true;
        self.session_dirty = true;
        self.ensure_active();
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::TabUpdated(state) => {
                if let Some(index) = self.index_of_tab(state.tab_id) {
                    if self.tabs[index].apply(&state) {
                        self.session_dirty = true;
                    }
                }
            }
            Event::TabCreated { tab_id, opener_tab_id, url, background } => {
                if self.index_of_tab(tab_id).is_some() {
                    return;
                }
                let at = opener_tab_id.and_then(|id| self.index_of_tab(id)).map(|i| i + 1).unwrap_or(self.tabs.len());
                let key = self.new_key();
                self.tabs.insert(at, TabEntry { key, tab_id: Some(tab_id), url, loading: true, ..Default::default() });
                if at <= self.selected && self.tabs.len() > 1 {
                    // The insertion pushed the selected entry down one row.
                    self.selected += 1;
                }
                if !background {
                    self.cancel_edit();
                    self.selected = at;
                }
                self.session_dirty = true;
                self.ensure_active();
            }
            Event::TabClosed { tab_id } => {
                if let Some(index) = self.index_of_tab(tab_id) {
                    if self.active_sent == Some(tab_id) {
                        self.active_sent = None;
                    }
                    self.remove_entry(index);
                }
            }
            Event::TabCrashed { tab_id } => {
                if let Some(index) = self.index_of_tab(tab_id) {
                    self.tabs[index].crashed = true;
                    self.tabs[index].loading = false;
                    self.message = Some("This page crashed. Press r to reload.".into());
                }
            }
            Event::FocusChanged { mode } => {
                self.mode = mode;
                if mode == Mode::Page {
                    self.cancel_edit();
                    self.message = None;
                }
            }
            Event::Command { name, arg } => self.on_command(&name, arg),
            Event::Unknown => {}
        }
    }

    fn on_command(&mut self, name: &str, arg: Option<i64>) {
        match name {
            "toggle-mode" => {
                let target = if self.mode == Mode::Nav { Mode::Page } else { Mode::Nav };
                self.request(Request::FocusSet { mode: target });
            }
            "edit-url" => {
                self.request(Request::FocusSet { mode: Mode::Nav });
                self.start_edit();
            }
            "new-tab" => {
                self.request(Request::FocusSet { mode: Mode::Nav });
                self.new_tab();
            }
            "close-tab" => self.close_selected(),
            "reload" => self.reload(),
            "back" => self.back(),
            "forward" => self.forward(),
            "next-tab" => self.step(1, true),
            "prev-tab" => self.step(-1, true),
            "select-tab" => {
                if self.tabs.is_empty() {
                    return;
                }
                let n = arg.unwrap_or(1).max(1) as usize;
                // Like other browsers, 9 means the last tab.
                let index = if n >= 9 { self.tabs.len() - 1 } else { (n - 1).min(self.tabs.len() - 1) };
                self.select(index);
            }
            "quit" => self.quit(),
            other => crate::log(&format!("unknown command {other}")),
        }
    }

    // ----- showing the right tab -----

    /// Makes the app show the selected entry, creating its tab if needed.
    fn ensure_active(&mut self) {
        if !self.ready {
            return;
        }
        self.clamp_scroll();
        let Some(entry) = self.tabs.get(self.selected).cloned() else {
            self.set_page_visible(false);
            return;
        };
        match entry.tab_id {
            Some(tab_id) => {
                if self.active_sent != Some(tab_id) {
                    self.active_sent = Some(tab_id);
                    self.request(Request::TabActivate { tab_id });
                }
                self.set_page_visible(true);
            }
            None => {
                self.set_page_visible(false);
                if !entry.pending && !entry.url.is_empty() {
                    self.tabs[self.selected].pending = true;
                    self.tabs[self.selected].loading = true;
                    self.send(
                        Request::TabCreate { url: Some(entry.url), input: None, activate: true },
                        Pending::Create { key: entry.key },
                    );
                }
            }
        }
    }

    fn set_page_visible(&mut self, visible: bool) {
        if self.visible_sent != Some(visible) {
            self.visible_sent = Some(visible);
            self.request(Request::ViewportVisible { visible });
        }
    }

    // ----- actions -----

    pub fn select(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.selected {
            return;
        }
        // Cancelling an edit can remove a blank tab and shift the indices.
        let key = self.tabs[index].key;
        self.cancel_edit();
        let Some(index) = self.index_of_key(key) else { return };
        self.selected = index;
        self.message = None;
        self.session_dirty = true;
        self.ensure_active();
    }

    fn step(&mut self, delta: i64, wrap: bool) {
        let len = self.tabs.len() as i64;
        if len == 0 {
            return;
        }
        let raw = self.selected as i64 + delta;
        let target = if wrap { raw.rem_euclid(len) } else { raw.clamp(0, len - 1) };
        self.select(target as usize);
    }

    fn move_selected(&mut self, delta: i64) {
        let target = self.selected as i64 + delta;
        if target < 0 || target >= self.tabs.len() as i64 {
            return;
        }
        self.tabs.swap(self.selected, target as usize);
        self.selected = target as usize;
        self.session_dirty = true;
        self.clamp_scroll();
    }

    pub fn new_tab(&mut self) {
        self.cancel_edit();
        let return_to = self.current().map(|t| t.key);
        let key = self.new_key();
        let at = if self.tabs.is_empty() { 0 } else { self.selected + 1 };
        self.tabs.insert(at, TabEntry { key, ..Default::default() });
        self.selected = at;
        self.edit = Some(UrlEdit { return_to, ..Default::default() });
        self.message = None;
        self.ensure_active();
    }

    pub fn start_edit(&mut self) {
        let Some(entry) = self.current() else { return };
        let text = entry.url.clone();
        let cursor = text.chars().count();
        let replace_on_type = !text.is_empty();
        let return_to = self.edit.as_ref().and_then(|e| e.return_to);
        self.edit = Some(UrlEdit { text, cursor, replace_on_type, return_to });
        self.message = None;
    }

    /// Leaves the URL bar; a tab that never got a URL disappears with it.
    pub fn cancel_edit(&mut self) {
        let Some(edit) = self.edit.take() else { return };
        if self.current().is_some_and(TabEntry::is_blank) {
            self.tabs.remove(self.selected);
            let back = edit.return_to.and_then(|key| self.index_of_key(key));
            self.selected = back.unwrap_or(self.selected.saturating_sub(1)).min(self.tabs.len().saturating_sub(1));
            if self.tabs.is_empty() {
                // Never leave the browser with nothing: reopen the home page.
                let key = self.new_key();
                self.tabs.push(TabEntry { key, url: HOME_URL.to_string(), ..Default::default() });
                self.selected = 0;
            }
            self.session_dirty = true;
            self.ensure_active();
        }
    }

    fn submit_edit(&mut self) {
        let Some(edit) = self.edit.clone() else { return };
        let input = edit.text.trim().to_string();
        if input.is_empty() {
            self.cancel_edit();
            return;
        }
        self.edit = None;
        let index = self.selected;
        let Some(entry) = self.tabs.get(index).cloned() else { return };
        match entry.tab_id {
            Some(tab_id) => {
                self.tabs[index].loading = true;
                self.tabs[index].crashed = false;
                self.request(Request::TabNavigate { tab_id, input });
                self.ensure_active();
                self.request(Request::FocusSet { mode: Mode::Page });
            }
            None if entry.pending => {
                // The tab is being created with its old URL; let that finish first.
                self.message = Some("This tab is still opening. Try again in a moment.".into());
            }
            None => {
                self.tabs[index].url = crate::resolve::resolve(&input).unwrap_or(input.clone());
                self.tabs[index].title.clear();
                self.tabs[index].loading = true;
                self.focus_on_create = Some(entry.key);
                self.tabs[index].pending = true;
                self.send(Request::TabCreate { url: None, input: Some(input), activate: true }, Pending::Create { key: entry.key });
            }
        }
        self.session_dirty = true;
    }

    pub fn close_selected(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.edit = None;
        let entry = self.tabs[self.selected].clone();
        if let Some(tab_id) = entry.tab_id {
            if self.active_sent == Some(tab_id) {
                self.active_sent = None;
            }
            self.request(Request::TabClose { tab_id });
        } else if entry.pending {
            self.orphans.insert(entry.key);
        }
        self.remove_entry(self.selected);
    }

    fn remove_entry(&mut self, index: usize) {
        self.tabs.remove(index);
        if index < self.selected {
            self.selected -= 1;
        }
        self.session_dirty = true;
        if self.tabs.is_empty() {
            self.selected = 0;
            self.request(Request::FocusSet { mode: Mode::Nav });
            self.new_tab();
            return;
        }
        self.selected = self.selected.min(self.tabs.len() - 1);
        self.ensure_active();
    }

    fn back(&mut self) {
        if let Some(tab_id) = self.current_tab_id() {
            self.request(Request::TabBack { tab_id });
        }
    }

    fn forward(&mut self) {
        if let Some(tab_id) = self.current_tab_id() {
            self.request(Request::TabForward { tab_id });
        }
    }

    fn reload(&mut self) {
        let Some(entry) = self.current().cloned() else { return };
        let Some(tab_id) = entry.tab_id else { return };
        self.message = None;
        if entry.loading {
            self.request(Request::TabStop { tab_id });
        } else {
            self.tabs[self.selected].crashed = false;
            self.request(Request::TabReload { tab_id });
        }
    }

    fn focus_page(&mut self) {
        if self.current_tab_id().is_some() {
            self.request(Request::FocusSet { mode: Mode::Page });
        }
    }

    pub fn quit(&mut self) {
        self.cancel_edit();
        self.save_session();
        self.request(Request::Shutdown);
        self.should_quit = true;
    }

    // ----- session -----

    pub fn session(&self) -> Session {
        let mut active = 0;
        let mut tabs = Vec::new();
        for (index, entry) in self.tabs.iter().enumerate() {
            if entry.url.is_empty() {
                continue;
            }
            if index == self.selected {
                active = tabs.len();
            }
            tabs.push(SessionTab { url: entry.url.clone(), title: entry.title.clone(), tab_id: entry.tab_id });
        }
        Session { instance: self.instance, active, tabs }
    }

    pub fn save_session(&mut self) {
        self.session_dirty = false;
        if !self.ready {
            return;
        }
        if let Some(path) = &self.session_path {
            if let Err(error) = session::save(path, &self.session()) {
                crate::log(&format!("cannot save session: {error}"));
            }
        }
    }

    // ----- input -----

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if self.edit.is_some() {
            self.handle_edit_key(key);
        } else {
            self.handle_nav_key(key);
        }
        self.pump();
    }

    fn handle_nav_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => self.quit(),
            KeyCode::Char('l') if ctrl => self.start_edit(),
            KeyCode::Char('j') | KeyCode::Down => self.step(1, false),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1, false),
            KeyCode::Enter | KeyCode::Char('i') => self.focus_page(),
            KeyCode::Char('o') => self.start_edit(),
            KeyCode::Char('t') => self.new_tab(),
            KeyCode::Char('x') => self.close_selected(),
            KeyCode::Char('H') => self.back(),
            KeyCode::Char('L') => self.forward(),
            KeyCode::Char('r') => self.reload(),
            KeyCode::Char('J') => self.move_selected(1),
            KeyCode::Char('K') => self.move_selected(-1),
            KeyCode::Char('q') => self.quit(),
            _ => {}
        }
    }

    fn handle_edit_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => return self.submit_edit(),
            KeyCode::Esc => return self.cancel_edit(),
            KeyCode::Char('c') if ctrl => return self.cancel_edit(),
            _ => {}
        }
        let Some(edit) = self.edit.as_mut() else { return };
        let len = edit.len();
        match key.code {
            KeyCode::Char('u') if ctrl => {
                edit.text.clear();
                edit.cursor = 0;
                edit.replace_on_type = false;
            }
            KeyCode::Char('w') if ctrl => edit.delete_word(),
            KeyCode::Char('a') if ctrl => {
                edit.cursor = 0;
                edit.replace_on_type = false;
            }
            KeyCode::Char('e') if ctrl => {
                edit.cursor = len;
                edit.replace_on_type = false;
            }
            KeyCode::Home => {
                edit.cursor = 0;
                edit.replace_on_type = false;
            }
            KeyCode::End => {
                edit.cursor = len;
                edit.replace_on_type = false;
            }
            KeyCode::Left => {
                edit.cursor = edit.cursor.saturating_sub(1);
                edit.replace_on_type = false;
            }
            KeyCode::Right => {
                edit.cursor = (edit.cursor + 1).min(len);
                edit.replace_on_type = false;
            }
            KeyCode::Backspace => edit.backspace(),
            KeyCode::Delete => {
                edit.replace_on_type = false;
                if edit.cursor < len {
                    let at = edit.cursor;
                    edit.delete_range(at, at + 1);
                }
            }
            KeyCode::Char(c) if !ctrl => edit.insert(&c.to_string()),
            _ => {}
        }
    }

    pub fn handle_paste(&mut self, text: &str) {
        if let Some(edit) = self.edit.as_mut() {
            edit.insert(text);
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        let (col, row) = (mouse.column, mouse.row);
        let layout = self.layout;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if contains(layout.tab_list, col, row) {
                    let index = self.scroll + (row - layout.tab_list.y) as usize;
                    if index < self.tabs.len() {
                        if index == self.selected {
                            self.cancel_edit();
                        } else {
                            self.select(index);
                        }
                    }
                } else if contains(layout.new_tab, col, row) {
                    self.new_tab();
                } else if contains(layout.back, col, row) {
                    self.back();
                } else if contains(layout.forward, col, row) {
                    self.forward();
                } else if contains(layout.reload, col, row) {
                    self.reload();
                } else if contains(layout.url, col, row) && self.edit.is_none() {
                    self.start_edit();
                }
            }
            MouseEventKind::ScrollDown if contains(layout.sidebar, col, row) => {
                let max = self.tabs.len().saturating_sub(layout.tab_list.height as usize);
                self.scroll = (self.scroll + 1).min(max);
            }
            MouseEventKind::ScrollUp if contains(layout.sidebar, col, row) => {
                self.scroll = self.scroll.saturating_sub(1);
            }
            _ => {}
        }
        self.pump();
    }
}
