//! Browser state and behaviour. No terminal I/O happens here: keys and engine
//! messages come in, requests go out through a `Transport`.

use crate::protocol::{self, Event, Incoming, Mode, Request, Response, TabState};
use crate::resolve::display_host;
use crate::session;
use crate::spaces::{Space, SpaceForm};
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
    /// Pinned tabs form the top section of the sidebar and survive being closed.
    pub pinned: bool,
    /// The address the tab had when it was pinned; "return to pinned page" goes here.
    pub pin_url: Option<String>,
    /// A name the user gave the tab; shown instead of the page title.
    pub name: Option<String>,
}

impl TabEntry {
    /// Puts a tab whose page is gone back to its not-yet-loaded state.
    pub(crate) fn unload(&mut self) {
        self.tab_id = None;
        self.pending = false;
        self.loading = false;
        self.crashed = false;
        self.can_go_back = false;
        self.can_go_forward = false;
        if let Some(url) = self.pin_url.clone().filter(|_| self.pinned) {
            if self.url != url {
                self.url = url;
                self.title.clear();
            }
        }
    }

    pub fn label(&self) -> String {
        if let Some(name) = &self.name {
            name.clone()
        } else if !self.title.is_empty() {
            self.title.clone()
        } else if !self.url.is_empty() {
            display_host(&self.url)
        } else {
            "New tab".to_string()
        }
    }

    pub(crate) fn is_blank(&self) -> bool {
        self.tab_id.is_none() && !self.pending && self.url.is_empty()
    }

    pub(crate) fn apply(&mut self, state: &TabState) -> bool {
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditKind {
    /// The address of the current tab.
    #[default]
    Url,
    /// The custom name of the current tab.
    Rename,
}

/// The text field in the toolbar, used for addresses and for renaming.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UrlEdit {
    pub kind: EditKind,
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
pub(crate) enum Pending {
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
    pub fn compute(area: Rect, sidebar_visible: bool) -> Self {
        let status_h = area.height.min(1);
        let main_h = area.height - status_h;
        let side_w = if sidebar_visible { SIDEBAR_WIDTH.min(area.width) } else { 0 };
        let right_x = area.x + side_w;
        let right_w = area.width - side_w;
        let toolbar_h = main_h.min(1);

        let sidebar = Rect::new(area.x, area.y, side_w, main_h);
        // Rows 0 and 1 name the space, row 2 is blank, the last row is
        // "+ New tab"; tabs go between.
        let list_h = if side_w == 0 { 0 } else { main_h.saturating_sub(4) };
        let tab_list = Rect::new(area.x, area.y + main_h.min(3), side_w.saturating_sub(1), list_h);
        let new_tab = Rect::new(area.x, area.y + main_h.saturating_sub(1), side_w.saturating_sub(1), if side_w == 0 { 0 } else { main_h.min(1) });
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

/// One row of the sidebar's list: pinned tabs, a rule, then the other tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideRow {
    /// Index into `App::tabs`.
    Tab(usize),
    /// Shown in place of pinned tabs when there are none.
    PinPlaceholder,
    Separator,
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
    /// First row of `side_rows` shown in the sidebar.
    pub scroll: usize,
    pub(crate) list_height: u16,
    pub sidebar_visible: bool,
    /// The key reference is shown in place of the page.
    pub help: bool,
    /// Mode to go back to when the key reference closes.
    pub(crate) help_return: Option<Mode>,
    /// Every space; the current one's tabs live in `tabs` while it is current.
    pub spaces: Vec<Space>,
    /// Index of the current space.
    pub space: usize,
    /// The space editor, shown in place of the page.
    pub form: Option<SpaceForm>,
    /// Waiting for y/n on deleting the current space.
    pub confirm_delete: bool,
    pub session_dirty: bool,
    pub session_path: Option<PathBuf>,

    pub(crate) transport: Box<dyn Transport>,
    pub(crate) inbox: VecDeque<String>,
    pub(crate) pending: HashMap<u64, Pending>,
    pub(crate) next_request: u64,
    pub(crate) next_key: u64,
    pub(crate) instance: Option<u64>,
    /// The tab the app is currently showing, as far as we have told it.
    pub(crate) active_sent: Option<u64>,
    pub(crate) visible_sent: Option<bool>,
    pub(crate) viewport_sent: Option<Rect>,
    /// Entries closed while their `tab.create` was still in flight.
    pub(crate) orphans: HashSet<u64>,
    /// Entry that should take page focus once its tab exists.
    pub(crate) focus_on_create: Option<u64>,
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
            sidebar_visible: true,
            help: false,
            help_return: None,
            spaces: vec![Space::home()],
            space: 0,
            form: None,
            confirm_delete: false,
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

    pub(crate) fn send(&mut self, request: Request, pending: Pending) {
        let id = self.next_request;
        self.next_request += 1;
        self.pending.insert(id, pending);
        let line = protocol::encode(id, &request);
        let replies = self.transport.send(&line);
        self.inbox.extend(replies);
    }

    pub(crate) fn request(&mut self, request: Request) {
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

    pub(crate) fn new_key(&mut self) -> u64 {
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

    pub(crate) fn index_of_key(&self, key: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.key == key)
    }

    pub(crate) fn index_of_tab(&self, tab_id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.tab_id == Some(tab_id))
    }

    /// Whether the page is on screen (as opposed to a placeholder).
    pub fn page_visible(&self) -> bool {
        self.visible_sent == Some(true)
    }

    // ----- layout -----

    /// Called before every draw with the terminal size.
    pub fn set_area(&mut self, area: Rect) {
        self.layout = ScreenLayout::compute(area, self.sidebar_visible);
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

    /// Number of pinned tabs. They always come first in `tabs`.
    pub fn pinned_count(&self) -> usize {
        self.tabs.iter().take_while(|t| t.pinned).count()
    }

    /// The sidebar list from top to bottom.
    pub fn side_rows(&self) -> Vec<SideRow> {
        let pinned = self.pinned_count();
        let mut rows = Vec::with_capacity(self.tabs.len() + 2);
        if pinned == 0 {
            rows.push(SideRow::PinPlaceholder);
        }
        rows.extend((0..pinned).map(SideRow::Tab));
        rows.push(SideRow::Separator);
        rows.extend((pinned..self.tabs.len()).map(SideRow::Tab));
        rows
    }

    /// Scrolls the sidebar just enough to show the selected tab.
    pub(crate) fn clamp_scroll(&mut self) {
        let height = self.layout.tab_list.height as usize;
        if height == 0 {
            return;
        }
        let rows = self.side_rows();
        let row = rows.iter().position(|r| *r == SideRow::Tab(self.selected)).unwrap_or(0);
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + height {
            self.scroll = row + 1 - height;
        }
        // Prefer showing the list from its top whenever the selection allows it.
        if row < height {
            self.scroll = 0;
        }
        self.scroll = self.scroll.min(rows.len().saturating_sub(height));
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
                let Some(index) = self.index_of_key(key) else {
                    // The user moved to another space while the tab was opening.
                    if let Some(entry) = self.spaces.iter_mut().flat_map(|l| l.tabs.iter_mut()).find(|t| t.key == key) {
                        entry.pending = false;
                        entry.tab_id = tab_id.filter(|_| response.ok);
                        if entry.tab_id.is_some() {
                            // The app is showing it; put the current space's tab back.
                            self.active_sent = entry.tab_id;
                            self.session_dirty = true;
                            self.ensure_active();
                        }
                    }
                    return;
                };
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

    fn on_event(&mut self, event: Event) {
        match event {
            Event::TabUpdated(state) => {
                if let Some(index) = self.index_of_tab(state.tab_id) {
                    if self.tabs[index].apply(&state) {
                        self.session_dirty = true;
                    }
                } else if let Some((space, index)) = self.stashed_tab(state.tab_id) {
                    if self.spaces[space].tabs[index].apply(&state) {
                        self.session_dirty = true;
                    }
                }
            }
            Event::TabCreated { tab_id, opener_tab_id, url, background } => {
                if self.index_of_tab(tab_id).is_some() || self.stashed_tab(tab_id).is_some() {
                    return;
                }
                // A tab opened by a page in another space belongs to that space.
                if let Some((space, opener)) = opener_tab_id.and_then(|id| self.stashed_tab(id)) {
                    let key = self.new_key();
                    let home = &mut self.spaces[space];
                    let at = (opener + 1).max(home.tabs.iter().take_while(|t| t.pinned).count());
                    home.tabs.insert(at, TabEntry { key, tab_id: Some(tab_id), url, loading: true, ..Default::default() });
                    if at <= home.selected {
                        home.selected += 1;
                    }
                    self.session_dirty = true;
                    return;
                }
                // Next to its opener, but never inside the pinned section.
                let at = opener_tab_id
                    .and_then(|id| self.index_of_tab(id))
                    .map(|i| i + 1)
                    .unwrap_or(self.tabs.len())
                    .max(self.pinned_count());
                let key = self.new_key();
                self.tabs.insert(at, TabEntry { key, tab_id: Some(tab_id), url, loading: true, ..Default::default() });
                if at <= self.selected && self.tabs.len() > 1 {
                    // The insertion pushed the selected entry down one row.
                    self.selected += 1;
                }
                if !background {
                    let key = self.tabs[at].key;
                    self.close_overlays();
                    self.cancel_edit();
                    self.selected = self.index_of_key(key).unwrap_or(self.selected);
                }
                self.session_dirty = true;
                self.ensure_active();
            }
            Event::TabClosed { tab_id } => {
                if let Some(index) = self.index_of_tab(tab_id) {
                    if self.active_sent == Some(tab_id) {
                        self.active_sent = None;
                    }
                    if self.tabs[index].pinned {
                        self.unload_pinned(index);
                    } else {
                        self.remove_entry(index);
                    }
                } else if let Some((space, index)) = self.stashed_tab(tab_id) {
                    let home = &mut self.spaces[space];
                    if home.tabs[index].pinned {
                        home.tabs[index].unload();
                    } else {
                        home.tabs.remove(index);
                        if index < home.selected {
                            home.selected -= 1;
                        }
                    }
                    self.session_dirty = true;
                }
            }
            Event::TabCrashed { tab_id } => {
                if let Some(index) = self.index_of_tab(tab_id) {
                    self.tabs[index].crashed = true;
                    self.tabs[index].loading = false;
                    self.message = Some("This page crashed. Press r to reload.".into());
                } else if let Some((space, index)) = self.stashed_tab(tab_id) {
                    self.spaces[space].tabs[index].crashed = true;
                    self.spaces[space].tabs[index].loading = false;
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
        if self.overlay_open() && name != "help" && name != "quit" {
            // Any other command dismisses whatever is covering the page first.
            self.help_return = None;
            self.close_overlays();
        }
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
            "toggle-sidebar" => self.toggle_sidebar(),
            "toggle-pin" => self.toggle_pin(),
            "pin-home" => self.pin_home(),
            "rename-tab" => {
                self.request(Request::FocusSet { mode: Mode::Nav });
                self.start_rename();
            }
            "help" => {
                if self.help {
                    self.close_help();
                } else {
                    self.open_help();
                }
            }
            "next-space" => self.step_space(1),
            "prev-space" => self.step_space(-1),
            "select-space" => {
                let n = arg.unwrap_or(1).max(1) as usize;
                if n <= self.spaces.len() {
                    self.switch_space(n - 1);
                }
            }
            "new-space" => {
                self.request(Request::FocusSet { mode: Mode::Nav });
                self.open_space_form(false);
            }
            "edit-space" => {
                self.request(Request::FocusSet { mode: Mode::Nav });
                self.open_space_form(true);
            }
            "delete-space" => {
                self.request(Request::FocusSet { mode: Mode::Nav });
                self.ask_delete_space();
            }
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
    pub(crate) fn ensure_active(&mut self) {
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
                // The key reference is drawn where the page would be.
                self.set_page_visible(!self.overlay_open());
            }
            None => {
                self.set_page_visible(false);
                if !entry.pending && !entry.url.is_empty() {
                    self.tabs[self.selected].pending = true;
                    self.tabs[self.selected].loading = true;
                    let profile = self.engine_profile();
                    self.send(
                        Request::TabCreate { url: Some(entry.url), input: None, activate: true, profile },
                        Pending::Create { key: entry.key },
                    );
                }
            }
        }
    }

    pub(crate) fn set_page_visible(&mut self, visible: bool) {
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

    pub(crate) fn step(&mut self, delta: i64, wrap: bool) {
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
        // Pinned and unpinned tabs are separate sections.
        if self.tabs[target as usize].pinned != self.tabs[self.selected].pinned {
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
        let at = if self.tabs.is_empty() { 0 } else { (self.selected + 1).max(self.pinned_count()) };
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
        self.edit = Some(UrlEdit { kind: EditKind::Url, text, cursor, replace_on_type, return_to });
        self.message = None;
    }

    /// Opens the toolbar field on the tab's name.
    pub fn start_rename(&mut self) {
        self.cancel_edit();
        let Some(entry) = self.current() else { return };
        if entry.is_blank() {
            return;
        }
        let text = entry.label();
        let cursor = text.chars().count();
        self.edit = Some(UrlEdit { kind: EditKind::Rename, text, cursor, replace_on_type: true, return_to: None });
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
        if edit.kind == EditKind::Rename {
            self.edit = None;
            if let Some(entry) = self.tabs.get_mut(self.selected) {
                // An empty name goes back to showing the page title.
                entry.name = if input.is_empty() { None } else { Some(input) };
                self.session_dirty = true;
            }
            return;
        }
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
                let profile = self.engine_profile();
                self.send(Request::TabCreate { url: None, input: Some(input), activate: true, profile }, Pending::Create { key: entry.key });
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
        if entry.pinned {
            // Closing a pinned tab frees its page but keeps the pin.
            self.unload_pinned(self.selected);
            self.message = Some("Pinned tab unloaded. Unpin it with p to remove it.".into());
        } else {
            self.remove_entry(self.selected);
        }
    }

    /// Resets a pinned tab to its unloaded state at its pinned address. If it
    /// was selected, the selection moves on so it is not loaded again at once.
    fn unload_pinned(&mut self, index: usize) {
        self.tabs[index].unload();
        self.session_dirty = true;
        if index != self.selected {
            return;
        }
        let pinned = self.pinned_count();
        if pinned < self.tabs.len() {
            self.selected = pinned;
            self.ensure_active();
        } else {
            self.request(Request::FocusSet { mode: Mode::Nav });
            self.new_tab();
        }
    }

    /// Pins the selected tab at its current address, or unpins it.
    pub fn toggle_pin(&mut self) {
        self.cancel_edit();
        let Some(entry) = self.current().cloned() else { return };
        if entry.is_blank() {
            return;
        }
        let pinned = self.pinned_count();
        let mut entry = self.tabs.remove(self.selected);
        let at = if entry.pinned {
            entry.pinned = false;
            entry.pin_url = None;
            pinned - 1 // first of the unpinned tabs
        } else {
            entry.pinned = true;
            entry.pin_url = Some(entry.url.clone());
            pinned // last of the pinned tabs
        };
        self.tabs.insert(at, entry);
        self.selected = at;
        self.message = None;
        self.session_dirty = true;
        self.clamp_scroll();
    }

    /// Takes a pinned tab back to the address it was pinned at.
    pub fn pin_home(&mut self) {
        let Some(entry) = self.current().cloned() else { return };
        let Some(url) = entry.pin_url.filter(|_| entry.pinned) else {
            self.message = Some("This tab is not pinned.".into());
            return;
        };
        self.message = None;
        match entry.tab_id {
            Some(tab_id) => {
                if entry.url != url {
                    self.tabs[self.selected].loading = true;
                    self.request(Request::TabNavigate { tab_id, input: url });
                }
            }
            None if !entry.pending => {
                self.tabs[self.selected].url = url;
                self.ensure_active();
            }
            None => {}
        }
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
        self.session_dirty = true;
        // The next `set_area` recomputes the layout and moves the page.
    }

    pub fn open_help(&mut self) {
        if self.help {
            return;
        }
        self.cancel_edit();
        self.form = None;
        self.confirm_delete = false;
        self.help_return = Some(self.mode).filter(|m| *m == Mode::Page);
        self.help = true;
        self.message = None;
        self.request(Request::FocusSet { mode: Mode::Nav });
        self.ensure_active();
    }

    pub fn close_help(&mut self) {
        if !self.help {
            return;
        }
        self.help = false;
        self.ensure_active();
        if self.help_return.take() == Some(Mode::Page) {
            self.focus_page();
        }
    }

    pub(crate) fn remove_entry(&mut self, index: usize) {
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

    pub(crate) fn focus_page(&mut self) {
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
        if self.help {
            // Any key puts the page back.
            self.close_help();
        } else if self.form.is_some() {
            self.handle_form_key(key);
        } else if self.confirm_delete {
            self.handle_confirm_key(key);
        } else if self.edit.is_some() {
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
            KeyCode::Char('b') => self.toggle_sidebar(),
            KeyCode::Char('p') => self.toggle_pin(),
            KeyCode::Char('P') => self.pin_home(),
            KeyCode::Char('n') => self.start_rename(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('h') | KeyCode::Left => self.step_space(-1),
            KeyCode::Char('l') | KeyCode::Right => self.step_space(1),
            KeyCode::Char('N') => self.open_space_form(false),
            KeyCode::Char('e') => self.open_space_form(true),
            KeyCode::Char('D') => self.ask_delete_space(),
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
        if self.form.is_some() {
            self.handle_paste_into_form(text);
        } else if let Some(edit) = self.edit.as_mut() {
            edit.insert(text);
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        let (col, row) = (mouse.column, mouse.row);
        let layout = self.layout;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if self.help => self.close_help(),
            // The space editor and the delete question are keyboard-only.
            MouseEventKind::Down(MouseButton::Left) if self.overlay_open() => {}
            MouseEventKind::Down(MouseButton::Left) => {
                if layout.sidebar.width > 2 && row == layout.sidebar.y + 1 && col > layout.sidebar.x && col < layout.sidebar.x + layout.sidebar.width - 1 {
                    // One dot per space, two columns apart.
                    let index = ((col - layout.sidebar.x - 1) / 2) as usize;
                    if (col - layout.sidebar.x - 1) % 2 == 0 && index < self.spaces.len() {
                        self.switch_space(index);
                    }
                } else if contains(layout.tab_list, col, row) {
                    let rows = self.side_rows();
                    if let Some(SideRow::Tab(index)) = rows.get(self.scroll + (row - layout.tab_list.y) as usize).copied() {
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
                let max = self.side_rows().len().saturating_sub(layout.tab_list.height as usize);
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
