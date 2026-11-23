//! The Downloads and History views. Like the key reference they are drawn in
//! place of the page, and both belong to the whole browser, not to a space.

use crate::app::{App, Pending, TabEntry};
use crate::history::{self, Row};
use crate::protocol::{Download, Mode, Request, TabState};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Downloads,
    History,
}

impl App {
    pub(crate) fn now(&self) -> u64 {
        self.now_override
            .unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
    }

    // ----- opening and closing -----

    /// Shows a view, or closes it if it is the one showing.
    pub fn toggle_view(&mut self, view: View) {
        if self.view == Some(view) {
            self.close_view();
        } else {
            self.open_view(view);
        }
    }

    pub fn open_view(&mut self, view: View) {
        self.cancel_edit();
        self.form = None;
        self.confirm_delete = false;
        self.help = false;
        // Coming from another view or the key reference, keep where that came from.
        let came_from = self.view_return.take().or(self.help_return.take());
        self.view_return = came_from.or(Some(self.mode).filter(|m| *m == Mode::Page));
        self.view = Some(view);
        self.history_filter.clear();
        self.history_typing = false;
        self.view_selected = 0;
        self.view_scroll = 0;
        self.confirm_clear = false;
        self.message = None;
        if view == View::Downloads {
            self.downloads_unseen = false;
        }
        self.request(Request::FocusSet { mode: Mode::Nav });
        self.ensure_active();
    }

    pub fn close_view(&mut self) {
        if self.view.take().is_none() {
            return;
        }
        self.history_filter.clear();
        self.history_typing = false;
        self.confirm_clear = false;
        self.ensure_active();
        if self.view_return.take() == Some(Mode::Page) {
            self.focus_page();
        }
    }

    // ----- the list -----

    /// The cells the view's list occupies: below its heading, above the
    /// line of keys at the bottom, inside a margin.
    pub fn view_list(&self) -> Rect {
        let view = self.layout.viewport;
        let margin = 2.min(view.width / 2);
        Rect::new(view.x + margin, view.y + 3.min(view.height), view.width - 2 * margin, view.height.saturating_sub(6))
    }

    pub fn history_rows(&self) -> Vec<Row> {
        self.history.rows(self.now(), &self.history_filter)
    }

    /// The visits the History view lists, as indices from the newest: all
    /// of them, or those the filter leaves.
    fn history_items(&self) -> Vec<usize> {
        self.history_rows().into_iter().filter_map(|row| if let Row::Item(index) = row { Some(index) } else { None }).collect()
    }

    /// The filter changed: the selection goes to the first visit it leaves.
    fn history_filter_changed(&mut self) {
        self.view_selected = self.history_items().first().copied().unwrap_or(0);
        self.view_scroll = 0;
        self.clamp_view();
    }

    pub(crate) fn history_filter_paste(&mut self, text: &str) {
        self.history_filter.extend(text.chars().filter(|c| !c.is_control()));
        self.history_filter_changed();
    }

    /// Keys while the filter is being typed.
    fn handle_filter_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.history_typing = false;
                self.history_filter.clear();
            }
            KeyCode::Char('c') if ctrl => {
                self.history_typing = false;
                self.history_filter.clear();
            }
            // The filter stays; the keys go back to the list.
            KeyCode::Enter | KeyCode::Down | KeyCode::Tab => {
                self.history_typing = false;
                return;
            }
            KeyCode::Backspace if self.history_filter.is_empty() => self.history_typing = false,
            KeyCode::Backspace => {
                self.history_filter.pop();
            }
            KeyCode::Char('u') if ctrl => self.history_filter.clear(),
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => self.history_filter.push(c),
            _ => return,
        }
        self.history_filter_changed();
    }

    /// The download on line `index` of the view; the newest is first.
    pub fn download_at(&self, index: usize) -> Option<&Download> {
        self.downloads.len().checked_sub(index + 1).and_then(|i| self.downloads.get(i))
    }

    fn view_len(&self) -> usize {
        match self.view {
            Some(View::Downloads) => self.downloads.len(),
            Some(View::History) => self.history.len(),
            None => 0,
        }
    }

    /// Keeps the selection inside the list and on screen.
    pub(crate) fn clamp_view(&mut self) {
        let len = self.view_len();
        self.view_selected = self.view_selected.min(len.saturating_sub(1));
        if self.view == Some(View::History) && !self.history_filter.is_empty() {
            // The selection stays on a visit the filter leaves.
            let items = self.history_items();
            if !items.contains(&self.view_selected) {
                self.view_selected = items.first().copied().unwrap_or(0);
            }
        }
        let height = self.view_list().height as usize;
        if height == 0 || len == 0 {
            self.view_scroll = 0;
            return;
        }
        let (row, total) = match self.view {
            Some(View::History) => {
                let rows = self.history_rows();
                (rows.iter().position(|r| *r == Row::Item(self.view_selected)).unwrap_or(0), rows.len())
            }
            _ => (self.view_selected, len),
        };
        // In the history, keep an item's heading in sight when it is the first of its group.
        let top = if self.view == Some(View::History) { row.saturating_sub(1) } else { row };
        if top < self.view_scroll {
            self.view_scroll = top;
        } else if row >= self.view_scroll + height {
            self.view_scroll = row + 1 - height;
        }
        self.view_scroll = self.view_scroll.min(total.saturating_sub(height));
    }

    fn move_in_view(&mut self, delta: i64) {
        if self.view == Some(View::History) && !self.history_filter.is_empty() {
            let items = self.history_items();
            let at = items.iter().position(|i| *i == self.view_selected).unwrap_or(0) as i64;
            let to = (at + delta).clamp(0, (items.len() as i64 - 1).max(0)) as usize;
            self.view_selected = items.get(to).copied().unwrap_or(0);
            self.confirm_clear = false;
            return self.clamp_view();
        }
        let last = self.view_len().saturating_sub(1) as i64;
        self.view_selected = (self.view_selected as i64 + delta).clamp(0, last.max(0)) as usize;
        self.confirm_clear = false;
        self.clamp_view();
    }

    // ----- keys and mouse -----

    pub(crate) fn handle_view_key(&mut self, key: KeyEvent) {
        let Some(view) = self.view else { return };
        if self.confirm_clear {
            self.confirm_clear = false;
            if key.code == KeyCode::Char('y') {
                self.history.clear();
                self.view_selected = 0;
                self.view_scroll = 0;
            }
            return;
        }
        self.message = None;
        if self.history_typing && view == View::History {
            return self.handle_filter_key(key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.view_list().height.max(1) as i64;
        match key.code {
            KeyCode::Char('/') if view == View::History => {
                self.history_typing = true;
                return;
            }
            // Escape first lets go of a filter, then of the view.
            KeyCode::Esc if view == View::History && !self.history_filter.is_empty() => {
                self.history_filter.clear();
                return self.history_filter_changed();
            }
            KeyCode::Esc | KeyCode::Char('q') => return self.close_view(),
            KeyCode::Char('c') if ctrl => return self.close_view(),
            KeyCode::Char('?') => return self.open_help(),
            KeyCode::Char('d') if !ctrl => return self.toggle_view(View::Downloads),
            KeyCode::Char('y') => return self.toggle_view(View::History),
            KeyCode::Char('j') | KeyCode::Down => return self.move_in_view(1),
            KeyCode::Char('k') | KeyCode::Up => return self.move_in_view(-1),
            KeyCode::Char('d') | KeyCode::PageDown => return self.move_in_view(page),
            KeyCode::PageUp => return self.move_in_view(-page),
            KeyCode::Char('u') if ctrl => return self.move_in_view(-page),
            KeyCode::Char('g') | KeyCode::Home => return self.move_in_view(i64::MIN / 2),
            KeyCode::Char('G') | KeyCode::End => return self.move_in_view(i64::MAX / 2),
            _ => {}
        }
        match view {
            View::History => match key.code {
                KeyCode::Enter => self.open_visit(false),
                KeyCode::Char('s') => self.open_visit(true),
                KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                    // With a filter, the selection moves to the visit that
                    // takes this one's line.
                    let items = self.history_items();
                    let Some(at) = items.iter().position(|i| *i == self.view_selected) else { return };
                    self.history.remove(self.view_selected);
                    let left = self.history_items();
                    self.view_selected = left.get(at).or(left.last()).copied().unwrap_or(0);
                    self.clamp_view();
                }
                KeyCode::Char('C') if !self.history.is_empty() => self.confirm_clear = true,
                _ => {}
            },
            View::Downloads => {
                let Some(download) = self.download_at(self.view_selected).cloned() else { return };
                let download_id = download.download_id;
                match key.code {
                    KeyCode::Enter | KeyCode::Char('o') => self.send(Request::DownloadOpen { download_id }, Pending::Notify),
                    KeyCode::Char('f') => self.send(Request::DownloadReveal { download_id }, Pending::Notify),
                    // A running download is stopped first; a second press takes it off the list.
                    KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace if download.is_active() => {
                        self.request(Request::DownloadCancel { download_id })
                    }
                    KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => self.request(Request::DownloadRemove { download_id }),
                    KeyCode::Char('C') => self.request(Request::DownloadClear),
                    _ => {}
                }
            }
        }
    }

    /// A click on a line selects it; a click on the selected line opens it.
    pub(crate) fn click_in_view(&mut self, col: u16, row: u16) {
        let list = self.view_list();
        if col < list.x || col >= list.x + list.width || row < list.y || row >= list.y + list.height {
            return;
        }
        let line = self.view_scroll + (row - list.y) as usize;
        let index = match self.view {
            Some(View::History) => match self.history_rows().get(line) {
                Some(Row::Item(index)) => *index,
                _ => return,
            },
            Some(View::Downloads) if line < self.downloads.len() => line,
            _ => return,
        };
        self.confirm_clear = false;
        if index != self.view_selected {
            self.view_selected = index;
            self.clamp_view();
        } else if self.view == Some(View::History) {
            self.open_visit(false);
        } else if let Some(download_id) = self.download_at(index).map(|d| d.download_id) {
            self.send(Request::DownloadOpen { download_id }, Pending::Notify);
        }
    }

    // ----- history -----

    /// Opens the selected visit in a new tab: in the current space, or in the
    /// space it was visited in.
    fn open_visit(&mut self, in_its_space: bool) {
        let Some(visit) = self.history.get(self.view_selected).cloned() else { return };
        if in_its_space && self.current_space().name != visit.space {
            let Some(index) = self.spaces.iter().position(|s| s.name == visit.space) else {
                self.message = Some(format!("There is no space called \"{}\" any more.", visit.space));
                return;
            };
            self.view_return = None;
            self.switch_space(index);
        }
        self.view = None;
        self.view_return = None;
        self.open_in_new_tab(visit.url, visit.title);
    }

    /// Opens an address in a new tab of the current space, after the selected
    /// tab, and gives its page the keyboard.
    pub(crate) fn open_in_new_tab(&mut self, url: String, title: String) {
        self.cancel_edit();
        let key = self.new_key();
        let at = if self.tabs.is_empty() { 0 } else { (self.selected + 1).max(self.pinned_count()) };
        self.tabs.insert(at, TabEntry { key, url, title, ..Default::default() });
        self.selected = at;
        self.focus_on_create = Some(key);
        self.session_dirty = true;
        self.ensure_active();
    }

    /// Notes in the history what a tab's update says about where it is.
    /// `old_title` is the title the tab had before the update was applied.
    pub(crate) fn track_visit(&mut self, space: usize, index: usize, state: &TabState, old_title: &str) {
        let now = self.now();
        let current = space == self.space;
        let name = self.spaces[space].name.clone();
        let entry = if current { &mut self.tabs[index] } else { &mut self.spaces[space].tabs[index] };
        // While the page is still arriving the title is the previous page's.
        let fresh_title = if entry.loading && entry.title == old_title { String::new() } else { entry.title.clone() };

        // The app says which page the tab is on, and whether it is another
        // page or the same one under a new address.
        if let (Some(number), Some(url)) = (state.entry, state.entry_url.as_deref()) {
            if number == 0 || url.is_empty() {
                return; // nothing has arrived yet
            }
            let known = entry.recorded_entry == Some(number);
            // A tab restored from the session shows a page that was already visited.
            let restored = entry.recorded_entry.is_none() && entry.recorded_url == url;
            entry.recorded_entry = Some(number);
            if !known && !restored {
                entry.recorded_url = url.to_string();
                self.history.visit(url, &fresh_title, &name, now);
                return;
            }
            if entry.recorded_url != url {
                let old = std::mem::replace(&mut entry.recorded_url, url.to_string());
                self.history.readdress(&old, url, &name, now);
            }
            if entry.title != old_title || !entry.loading {
                self.history.retitle(url, &entry.title);
            }
            return;
        }

        // An engine that does not say (the mock): every new address is a visit.
        let reported = state.url.as_deref().is_some_and(|url| !url.is_empty());
        if reported && entry.url != entry.recorded_url {
            entry.recorded_url = entry.url.clone();
            self.history.visit(&entry.url, &fresh_title, &name, now);
        } else if entry.title != old_title || !entry.loading {
            self.history.retitle(&entry.url, &entry.title);
        }
    }

    // ----- downloads -----

    pub(crate) fn download_updated(&mut self, download: Download) {
        let finished = !download.is_active();
        match self.downloads.iter_mut().find(|d| d.download_id == download.download_id) {
            Some(known) => {
                let was_active = known.is_active();
                *known = download;
                if was_active && finished && self.view != Some(View::Downloads) {
                    self.downloads_unseen = true;
                }
            }
            None => {
                self.downloads.push(download);
                // The newest is the first line: keep the selection on its item.
                if self.view == Some(View::Downloads) && self.downloads.len() > 1 {
                    self.view_selected += 1;
                    self.clamp_view();
                }
            }
        }
    }

    pub(crate) fn downloads_replaced(&mut self, downloads: Vec<Download>) {
        self.downloads = downloads;
        if self.downloads.is_empty() {
            self.downloads_unseen = false;
        }
        if self.view == Some(View::Downloads) {
            self.clamp_view();
        }
    }

    /// What the status line says about downloads, if anything: the text and
    /// whether it reports a failure.
    pub fn download_notice(&self) -> Option<(String, bool)> {
        let active: Vec<&Download> = self.downloads.iter().filter(|d| d.is_active()).collect();
        if !active.is_empty() {
            let (received, total) = active.iter().fold((0u64, 0u64), |(r, t), d| (r + d.received, t + d.total));
            let known = active.iter().all(|d| d.total > 0);
            let what = if active.len() == 1 { active[0].filename.clone() } else { format!("{} downloads", active.len()) };
            let progress = if known { format!("{}%", (received * 100 / total.max(1)).min(100)) } else { size(received) };
            return Some((format!("↓ {what} {progress}"), false));
        }
        if !self.downloads_unseen {
            return None;
        }
        let last = self.downloads.last()?;
        match last.state.as_str() {
            "done" => Some((format!("↓ {} ✓", last.filename), false)),
            "failed" => Some((format!("↓ {} failed", last.filename), true)),
            _ => None,
        }
    }

    pub fn history_file(&self) -> Option<std::path::PathBuf> {
        self.session_path.as_deref().map(history::path_beside)
    }
}

/// "3.4 MB"
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 999.95 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}
