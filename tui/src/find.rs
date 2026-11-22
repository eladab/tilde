//! Find in page. The bar takes the place of the status line; the page does
//! the searching and reports how many matches there are and which is shown.

use crate::app::{App, UrlEdit};
use crate::protocol::{Mode, Request};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The app counts no further than this.
pub const LIMIT: i64 = 1000;

#[derive(Debug, Clone, PartialEq)]
pub struct Find {
    pub edit: UrlEdit,
    /// The tab being searched.
    pub tab_id: u64,
    /// What the page found for the text as it stands.
    pub result: Option<FindResult>,
    /// The page had the keys when the bar opened, and gets them back.
    return_to_page: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindResult {
    /// How many matches; negative when the page cannot say.
    pub count: i64,
    /// The match the page is on, counting from 1; 0 when it is on none.
    pub index: u64,
}

impl App {
    /// Whether keys typed in the terminal go to the find bar.
    pub fn find_has_keys(&self) -> bool {
        self.find.is_some() && self.mode == Mode::Nav && self.edit.is_none() && !self.overlay_open()
    }

    /// Opens the find bar, or goes back to it. `text` is searched for at
    /// once; without it the last search is offered, ready to be typed over.
    pub fn open_find(&mut self, text: Option<String>) {
        let Some(tab_id) = self.current().and_then(|t| t.tab_id) else {
            self.message = Some("There is no page to search.".into());
            return;
        };
        self.cancel_edit();
        self.message = None;
        let from_page = self.mode == Mode::Page;
        self.request(Request::FocusSet { mode: Mode::Nav });
        let given = text.is_some();
        let reopened = self.find.as_ref().is_some_and(|f| f.tab_id == tab_id);
        let text = text.unwrap_or_else(|| match &self.find {
            Some(find) if reopened => find.edit.text.clone(),
            _ => self.last_find.clone(),
        });
        let return_to_page = from_page || self.find.as_ref().is_some_and(|f| reopened && f.return_to_page);
        let result = self.find.as_ref().filter(|f| reopened && f.edit.text == text).and_then(|f| f.result);
        let mut edit = UrlEdit::holding(&text);
        edit.replace_on_type = !text.is_empty();
        self.find = Some(Find { edit, tab_id, result, return_to_page });
        if !text.is_empty() && (given || !reopened) {
            self.find_changed();
        }
    }

    /// Closes the find bar and takes the marks off the page.
    pub fn close_find(&mut self) {
        let Some(find) = self.find.take() else { return };
        self.request(Request::FindClose { tab_id: find.tab_id });
        if find.return_to_page && self.mode == Mode::Nav && !self.overlay_open() {
            self.request(Request::FocusSet { mode: Mode::Page });
        }
    }

    /// Closes the bar when the tab it searches is no longer the one showing.
    pub(crate) fn find_follows_tab(&mut self) {
        let showing = self.current().and_then(|t| t.tab_id);
        if let Some(find) = self.find.as_ref().filter(|f| Some(f.tab_id) != showing) {
            let tab_id = find.tab_id;
            self.find = None;
            if self.index_of_tab(tab_id).is_some() || self.stashed_tab(tab_id).is_some() {
                self.request(Request::FindClose { tab_id });
            }
        }
    }

    /// The text changed: search for it from where the page is.
    fn find_changed(&mut self) {
        let Some(find) = self.find.as_mut() else { return };
        find.result = None;
        let (tab_id, text) = (find.tab_id, find.edit.text.clone());
        self.last_find = text.clone();
        if text.is_empty() {
            self.request(Request::FindClose { tab_id });
        } else {
            self.request(Request::FindSet { tab_id, text, backwards: false, fresh: true });
        }
    }

    /// Goes to the next match, or the previous one. Without the bar, the
    /// last search is taken up again.
    pub fn find_step(&mut self, backwards: bool) {
        self.find_follows_tab();
        if self.find.is_none() {
            let Some(tab_id) = self.current().and_then(|t| t.tab_id) else { return };
            if self.last_find.is_empty() {
                return self.open_find(None);
            }
            // The keys stay where they are: this is a step, not an edit.
            let edit = UrlEdit::holding(&self.last_find);
            self.find = Some(Find { edit, tab_id, result: None, return_to_page: false });
        }
        let Some(find) = self.find.as_mut() else { return };
        find.edit.replace_on_type = false;
        if !find.edit.text.is_empty() {
            let (tab_id, text) = (find.tab_id, find.edit.text.clone());
            self.request(Request::FindSet { tab_id, text, backwards, fresh: false });
        }
    }

    pub(crate) fn find_result(&mut self, tab_id: u64, text: &str, count: i64, index: u64) {
        if let Some(find) = self.find.as_mut().filter(|f| f.tab_id == tab_id && f.edit.text == text) {
            find.result = Some(FindResult { count, index });
        }
    }

    /// The page being searched went elsewhere: what was found is gone.
    pub(crate) fn find_page_changed(&mut self, tab_id: u64) {
        if let Some(find) = self.find.as_mut().filter(|f| f.tab_id == tab_id) {
            find.result = None;
        }
    }

    pub(crate) fn handle_find_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Esc => return self.close_find(),
            KeyCode::Char('c') if ctrl => return self.close_find(),
            KeyCode::Enter if shift => return self.find_step(true),
            KeyCode::Enter | KeyCode::Down => return self.find_step(false),
            KeyCode::Up | KeyCode::BackTab => return self.find_step(true),
            KeyCode::Char('g') if ctrl => return self.find_step(false),
            _ => {}
        }
        let Some(find) = self.find.as_mut() else { return };
        let before = find.edit.text.clone();
        find.edit.key(key);
        if find.edit.text != before {
            self.find_changed();
        }
    }

    pub(crate) fn find_paste(&mut self, text: &str) {
        let Some(find) = self.find.as_mut() else { return };
        find.edit.insert(text);
        self.find_changed();
    }
}
