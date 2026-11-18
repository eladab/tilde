//! Spaces: separate sets of tabs, each with its own pinned tabs, colour and
//! profile. The current space's tabs live in `App::tabs`; the others keep
//! theirs in `Space::tabs` until they become current again. Tabs of every space
//! stay loaded in the app.

use crate::app::{App, TabEntry, HOME_URL};
use crate::art::{ART, BACKGROUNDS};
use crate::protocol::{Mode, Request, TabState};
use crate::session::{self, Session, SessionSpace, SessionTab};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The colours a space can have: name and RGB.
pub const COLORS: &[(&str, (u8, u8, u8))] = &[
    ("blue", (122, 162, 247)),
    ("green", (158, 206, 106)),
    ("yellow", (224, 175, 104)),
    ("orange", (255, 158, 100)),
    ("red", (247, 118, 142)),
    ("pink", (245, 150, 205)),
    ("purple", (187, 154, 247)),
    ("teal", (115, 218, 202)),
];

pub const DEFAULT_PROFILE: &str = "default";

#[derive(Debug, Clone, Default)]
pub struct Space {
    pub name: String,
    /// Index into `COLORS`.
    pub color: usize,
    /// Index into `art::BACKGROUNDS`.
    pub background: usize,
    /// 0 for none, otherwise a piece of `art::ART` (one-based).
    pub art: usize,
    /// Tabs of spaces with the same profile share cookies and logins.
    pub profile: String,
    /// Empty while this space is the current one (see the module comment).
    pub tabs: Vec<TabEntry>,
    pub selected: usize,
}

impl Space {
    pub fn home() -> Self {
        Self { name: "Home".into(), color: 0, background: 0, art: 0, profile: DEFAULT_PROFILE.into(), tabs: Vec::new(), selected: 0 }
    }
}

/// Profile names are lower-case words joined by dashes, so that "Work" and
/// "work " are the same profile.
pub fn profile_slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Name,
    Color,
    Background,
    Art,
    Profile,
}

impl FormField {
    const ORDER: [FormField; 5] = [FormField::Name, FormField::Color, FormField::Background, FormField::Art, FormField::Profile];

    fn step(self, delta: isize) -> FormField {
        let at = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0) as isize;
        Self::ORDER[(at + delta).rem_euclid(Self::ORDER.len() as isize) as usize]
    }
}

/// The space editor.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceForm {
    /// Editing the current space rather than creating one.
    pub editing: bool,
    pub field: FormField,
    pub name: String,
    pub color: usize,
    pub background: usize,
    pub art: usize,
    /// Empty means "a profile of its own, named after the space".
    pub profile: String,
    pub error: Option<String>,
}

impl SpaceForm {
    /// The profile this form will produce.
    pub fn resolved_profile(&self) -> String {
        let typed = profile_slug(&self.profile);
        if typed.is_empty() {
            profile_slug(&self.name)
        } else {
            typed
        }
    }
}

fn session_tabs(tabs: &[TabEntry], selected: usize) -> (Vec<SessionTab>, usize) {
    let mut active = 0;
    let mut out = Vec::new();
    for (index, entry) in tabs.iter().enumerate() {
        // A tab that never got an address is not worth remembering.
        if entry.url.is_empty() {
            continue;
        }
        if index == selected {
            active = out.len();
        }
        out.push(SessionTab {
            url: entry.url.clone(),
            title: entry.title.clone(),
            tab_id: entry.tab_id,
            pinned: entry.pinned,
            pin_url: entry.pin_url.clone(),
            pin_title: entry.pin_title.clone(),
            name: entry.name.clone(),
        });
    }
    (out, active)
}

impl App {
    pub fn current_space(&self) -> &Space {
        &self.spaces[self.space]
    }

    /// Colour, background and art to draw with: the space's, or, while the
    /// space editor is open, the ones being chosen, so they preview live.
    pub fn theme(&self) -> (usize, usize, usize) {
        match &self.form {
            Some(form) => (form.color, form.background, form.art),
            None => {
                let space = self.current_space();
                (space.color, space.background, space.art)
            }
        }
    }

    /// The profile to create tabs with; `None` is the app's default store.
    pub(crate) fn engine_profile(&self) -> Option<String> {
        let profile = &self.current_space().profile;
        (profile != DEFAULT_PROFILE && !profile.is_empty()).then(|| profile.clone())
    }

    /// Finds a tab that belongs to a space other than the current one.
    pub(crate) fn stashed_tab(&self, tab_id: u64) -> Option<(usize, usize)> {
        self.spaces.iter().enumerate().find_map(|(l, space)| {
            space.tabs.iter().position(|t| t.tab_id == Some(tab_id)).map(|i| (l, i))
        })
    }

    /// Something other than the page is drawn in the page area.
    pub fn overlay_open(&self) -> bool {
        self.help || self.form.is_some() || self.confirm_delete || self.view.is_some()
    }

    pub(crate) fn close_overlays(&mut self) {
        if !self.overlay_open() {
            return;
        }
        self.form = None;
        self.confirm_delete = false;
        if self.view.is_some() {
            self.close_view();
        } else if self.help {
            self.close_help();
        } else {
            self.ensure_active();
        }
    }

    // ----- switching -----

    pub fn switch_space(&mut self, index: usize) {
        if index >= self.spaces.len() || index == self.space {
            return;
        }
        self.close_overlays();
        self.cancel_edit();
        let from = self.space;
        self.spaces[from].tabs = std::mem::take(&mut self.tabs);
        self.spaces[from].selected = self.selected;
        self.space = index;
        self.tabs = std::mem::take(&mut self.spaces[index].tabs);
        self.selected = self.spaces[index].selected.min(self.tabs.len().saturating_sub(1));
        self.scroll = 0;
        self.message = None;
        self.session_dirty = true;
        if self.tabs.is_empty() {
            // A space is never empty: start it with a blank tab and the address field.
            self.request(Request::FocusSet { mode: Mode::Nav });
            self.new_tab();
        } else {
            self.ensure_active();
        }
    }

    pub fn step_space(&mut self, delta: i64) {
        let count = self.spaces.len() as i64;
        if count < 2 {
            self.message = Some("There is only one space. Press N to make another.".into());
            return;
        }
        self.switch_space((self.space as i64 + delta).rem_euclid(count) as usize);
    }

    // ----- the editor -----

    pub fn open_space_form(&mut self, editing: bool) {
        self.cancel_edit();
        self.help = false;
        self.confirm_delete = false;
        self.message = None;
        self.form = Some(if editing {
            let space = self.current_space();
            SpaceForm {
                editing,
                field: FormField::Name,
                name: space.name.clone(),
                color: space.color,
                background: space.background,
                art: space.art,
                profile: space.profile.clone(),
                error: None,
            }
        } else {
            // Suggest a colour no space uses yet.
            let color = (0..COLORS.len()).find(|c| self.spaces.iter().all(|l| l.color != *c)).unwrap_or(self.spaces.len() % COLORS.len());
            SpaceForm { editing, field: FormField::Name, name: String::new(), color, background: 0, art: 0, profile: String::new(), error: None }
        });
        self.ensure_active();
    }

    pub(crate) fn handle_form_key(&mut self, key: KeyEvent) {
        let Some(form) = self.form.as_mut() else { return };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        form.error = None;
        match key.code {
            KeyCode::Esc => {
                self.form = None;
                self.ensure_active();
            }
            KeyCode::Char('c') if ctrl => {
                self.form = None;
                self.ensure_active();
            }
            KeyCode::Enter => self.submit_space_form(),
            KeyCode::Tab | KeyCode::Down => form.field = form.field.step(1),
            KeyCode::BackTab | KeyCode::Up => form.field = form.field.step(-1),
            // The three pickers: ← and → (or space) move through the choices.
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if matches!(form.field, FormField::Color | FormField::Background | FormField::Art) =>
            {
                let (value, count) = match form.field {
                    FormField::Color => (&mut form.color, COLORS.len()),
                    FormField::Background => (&mut form.background, BACKGROUNDS.len()),
                    _ => (&mut form.art, ART.len() + 1), // plus "none"
                };
                *value = if key.code == KeyCode::Left { (*value + count - 1) % count } else { (*value + 1) % count };
            }
            KeyCode::Char('u') if ctrl => match form.field {
                FormField::Name => form.name.clear(),
                FormField::Profile => form.profile.clear(),
                _ => {}
            },
            KeyCode::Backspace => match form.field {
                FormField::Name => {
                    form.name.pop();
                }
                FormField::Profile => {
                    form.profile.pop();
                }
                _ => {}
            },
            // Option+← and Option+→ arrive as Alt+b and Alt+f: not text.
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) && !c.is_control() => match form.field {
                FormField::Name if form.name.chars().count() < 40 => form.name.push(c),
                FormField::Profile if form.profile.chars().count() < 40 => form.profile.push(c),
                _ => {}
            },
            _ => {}
        }
    }

    pub(crate) fn handle_paste_into_form(&mut self, text: &str) {
        let Some(form) = self.form.as_mut() else { return };
        let clean: String = text.chars().filter(|c| !c.is_control()).take(40).collect();
        match form.field {
            FormField::Name => form.name.push_str(&clean),
            FormField::Profile => form.profile.push_str(&clean),
            _ => {}
        }
    }

    fn submit_space_form(&mut self) {
        let Some(form) = self.form.clone() else { return };
        let name = form.name.trim().to_string();
        if name.is_empty() {
            if let Some(f) = self.form.as_mut() {
                f.error = Some("A space needs a name.".into());
                f.field = FormField::Name;
            }
            return;
        }
        let profile = form.resolved_profile();
        let profile = if profile.is_empty() { DEFAULT_PROFILE.to_string() } else { profile };
        self.form = None;
        self.session_dirty = true;

        if form.editing {
            let changed_profile = self.spaces[self.space].profile != profile;
            let old_name = self.spaces[self.space].name.clone();
            self.history.rename_space(&old_name, &name);
            let space = &mut self.spaces[self.space];
            space.name = name;
            space.color = form.color;
            space.background = form.background;
            space.art = form.art;
            space.profile = profile;
            if changed_profile {
                // Pages cannot change profile in place: close them, and let
                // them open again, lazily, in the new one.
                self.unload_current_tabs();
                self.message = Some("Profile changed: this space's tabs were reloaded in it.".into());
            }
            self.ensure_active();
        } else {
            let at = self.space + 1;
            self.spaces.insert(
                at,
                Space { name, color: form.color, background: form.background, art: form.art, profile, tabs: Vec::new(), selected: 0 },
            );
            self.switch_space(at);
        }
    }

    /// Closes the page of every tab in the current space, keeping the tabs.
    fn unload_current_tabs(&mut self) {
        for index in 0..self.tabs.len() {
            let entry = &self.tabs[index];
            if let Some(tab_id) = entry.tab_id {
                if self.active_sent == Some(tab_id) {
                    self.active_sent = None;
                }
                self.request(Request::TabClose { tab_id });
            } else if entry.pending {
                self.orphans.insert(entry.key);
            }
            // Keep the address the tab was on, also for pinned tabs.
            let url = self.tabs[index].url.clone();
            let title = self.tabs[index].title.clone();
            self.tabs[index].unload();
            self.tabs[index].url = url;
            self.tabs[index].title = title;
        }
    }

    // ----- deleting -----

    pub fn ask_delete_space(&mut self) {
        self.cancel_edit();
        if self.spaces.len() < 2 {
            self.message = Some("The last space cannot be deleted.".into());
            return;
        }
        self.help = false;
        self.form = None;
        self.message = None;
        self.confirm_delete = true;
        self.ensure_active();
    }

    pub(crate) fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => self.delete_current_space(),
            _ => {
                self.confirm_delete = false;
                self.ensure_active();
            }
        }
    }

    fn delete_current_space(&mut self) {
        self.confirm_delete = false;
        if self.spaces.len() < 2 {
            return;
        }
        for entry in std::mem::take(&mut self.tabs) {
            if let Some(tab_id) = entry.tab_id {
                if self.active_sent == Some(tab_id) {
                    self.active_sent = None;
                }
                self.request(Request::TabClose { tab_id });
            } else if entry.pending {
                self.orphans.insert(entry.key);
            }
        }
        let removed = self.spaces.remove(self.space);
        self.space = self.space.min(self.spaces.len() - 1);
        self.tabs = std::mem::take(&mut self.spaces[self.space].tabs);
        self.selected = self.spaces[self.space].selected.min(self.tabs.len().saturating_sub(1));
        self.scroll = 0;
        self.session_dirty = true;
        self.message = Some(format!("Deleted space \"{}\".", removed.name));
        if self.tabs.is_empty() {
            self.new_tab();
        } else {
            self.ensure_active();
        }
    }

    // ----- session -----

    pub fn session(&self) -> Session {
        let spaces = self
            .spaces
            .iter()
            .enumerate()
            .map(|(index, space)| {
                let (tabs, active) = if index == self.space {
                    session_tabs(&self.tabs, self.selected)
                } else {
                    session_tabs(&space.tabs, space.selected)
                };
                SessionSpace {
                    name: space.name.clone(),
                    color: space.color,
                    background: space.background,
                    art: space.art,
                    profile: space.profile.clone(),
                    active,
                    tabs,
                }
            })
            .collect();
        Session {
            instance: self.instance,
            current: self.space,
            sidebar_hidden: !self.sidebar_visible,
            devtools_below: self.dev.dock == crate::devtools::Dock::Bottom,
            devtools_size: if self.dev.percent == 45 { 0 } else { self.dev.percent },
            spaces,
            ..Default::default()
        }
    }

    /// Builds spaces and tabs from the session file and whatever the app still has loaded.
    pub(crate) fn restore(&mut self, live: Vec<TabState>, live_active: Option<u64>) {
        let saved = self.session_path.as_deref().and_then(session::load).unwrap_or_default();
        let same_instance = saved.instance.is_some() && saved.instance == self.instance;
        let mut unclaimed: Vec<TabState> = live;
        self.sidebar_visible = !saved.sidebar_hidden;
        if saved.devtools_below {
            self.dev.dock = crate::devtools::Dock::Bottom;
        }
        if saved.devtools_size != 0 {
            self.dev.percent = saved.devtools_size.clamp(20, 80);
        }
        let current = saved.current;

        let mut spaces = Vec::new();
        for saved_space in saved.into_spaces() {
            let mut tabs = Vec::new();
            let mut selected_key = None;
            for (index, tab) in saved_space.tabs.iter().enumerate() {
                if tab.url.is_empty() {
                    continue;
                }
                let mut entry = TabEntry {
                    key: self.new_key(),
                    url: tab.url.clone(),
                    title: tab.title.clone(),
                    pinned: tab.pinned,
                    pin_url: if tab.pinned { tab.pin_url.clone().or_else(|| Some(tab.url.clone())) } else { None },
                    pin_title: tab.pin_title.clone().filter(|t| tab.pinned && !t.is_empty()),
                    name: tab.name.clone().filter(|n| !n.is_empty()),
                    ..Default::default()
                };
                // Tabs the app still has loaded are adopted, not opened again.
                if same_instance {
                    if let Some(pos) = tab.tab_id.and_then(|id| unclaimed.iter().position(|s| s.tab_id == id)) {
                        let state = unclaimed.remove(pos);
                        entry.tab_id = Some(state.tab_id);
                        entry.apply(&state);
                    }
                }
                if index == saved_space.active {
                    selected_key = Some(entry.key);
                }
                tabs.push(entry);
            }
            // Pinned tabs come first; the selection stays on the same tab.
            tabs.sort_by_key(|t| !t.pinned);
            let selected = selected_key.and_then(|key| tabs.iter().position(|t| t.key == key)).unwrap_or(0);
            let profile = profile_slug(&saved_space.profile);
            spaces.push(Space {
                name: if saved_space.name.trim().is_empty() { "Space".into() } else { saved_space.name.clone() },
                color: saved_space.color % COLORS.len(),
                // An index from a newer version falls back to the plain look.
                background: if saved_space.background < BACKGROUNDS.len() { saved_space.background } else { 0 },
                art: if saved_space.art <= ART.len() { saved_space.art } else { 0 },
                profile: if profile.is_empty() { DEFAULT_PROFILE.into() } else { profile },
                tabs,
                selected,
            });
        }
        if spaces.is_empty() {
            spaces.push(Space::home());
        }

        self.space = current.min(spaces.len() - 1);
        self.spaces = spaces;
        self.tabs = std::mem::take(&mut self.spaces[self.space].tabs);
        self.selected = self.spaces[self.space].selected;
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
        self.selected = self.selected.min(self.tabs.len() - 1);

        // Pages that were already open are not visited again by being restored.
        for entry in self.tabs.iter_mut().chain(self.spaces.iter_mut().flat_map(|s| s.tabs.iter_mut())) {
            entry.recorded_url = entry.url.clone();
        }
        self.active_sent = live_active;
        self.ready = true;
        self.session_dirty = true;
        self.ensure_active();
    }
}

#[cfg(test)]
mod tests {
    use super::profile_slug;

    #[test]
    fn profile_names_are_normalised() {
        assert_eq!(profile_slug("Work"), "work");
        assert_eq!(profile_slug("  Side Project!  "), "side-project");
        assert_eq!(profile_slug("a__b"), "a-b");
        assert_eq!(profile_slug("default"), "default");
        assert_eq!(profile_slug("  "), "");
        assert_eq!(profile_slug("Ünïcode 1"), "ünïcode-1");
    }
}
