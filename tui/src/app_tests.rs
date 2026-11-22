//! Behaviour tests: the real `App` driven against the mock engine.

use crate::app::{App, EditKind, SideRow, HOME_URL};
use crate::mock::MockEngine;
use crate::protocol::Mode;
use crate::spaces::FormField;
use crate::session::{self, Session, SessionSpace, SessionTab};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

type Engine = Rc<RefCell<MockEngine>>;

fn start_with(engine: Engine, session: Option<PathBuf>) -> App {
    let mut app = App::new(Box::new(engine), session);
    app.start();
    app.set_area(Rect::new(0, 0, 120, 40));
    app
}

fn start() -> (App, Engine) {
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    (start_with(engine.clone(), None), engine)
}

fn key(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

/// Lets the selection rest long enough for a tab that is not loaded to open.
fn rest(app: &mut App) {
    app.tick(std::time::Instant::now() + crate::app::LOAD_DELAY);
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, c);
    }
}

fn command(app: &mut App, name: &str) {
    app.handle_line(&format!(r#"{{"event":"command","name":"{name}"}}"#));
}

fn open(app: &mut App, input: &str) {
    key(app, 't');
    type_text(app, input);
    press(app, KeyCode::Enter);
}

fn urls(app: &App) -> Vec<&str> {
    app.tabs.iter().map(|t| t.url.as_str()).collect()
}

fn temp_session(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("tilde-app-test-{}-{name}.json", std::process::id()))
}

#[test]
fn first_launch_opens_home_page() {
    let (app, engine) = start();
    assert!(app.ready);
    assert_eq!(urls(&app), vec![HOME_URL]);
    let tab_id = app.tabs[0].tab_id.expect("tab created");
    let engine = engine.borrow();
    assert_eq!(engine.active, Some(tab_id));
    assert_eq!(engine.viewport, Some((28, 1, 92, 38)));
    assert!(engine.visible);
    assert_eq!(app.tabs[0].title, "webkit.org");
}

#[test]
fn new_tab_resolves_urls_and_searches() {
    let (mut app, engine) = start();
    key(&mut app, 't');
    assert!(app.edit.is_some());
    assert_eq!(app.tabs.len(), 2);
    assert!(!engine.borrow().visible, "blank tab hides the page");
    type_text(&mut app, "example.com");
    press(&mut app, KeyCode::Enter);
    assert!(app.edit.is_none());
    assert_eq!(app.tabs[1].url, "https://example.com");
    assert_eq!(engine.borrow().active, app.tabs[1].tab_id);
    assert!(engine.borrow().visible);
    assert_eq!(app.mode, Mode::Page, "submitting a URL moves to the page");

    command(&mut app, "new-tab");
    assert_eq!(app.mode, Mode::Nav);
    type_text(&mut app, "swift concurrency");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.tabs[2].url, "https://duckduckgo.com/?q=swift%20concurrency");
}

#[test]
fn cancelling_a_new_tab_returns_to_the_previous_one() {
    let (mut app, engine) = start();
    open(&mut app, "a.example");
    command(&mut app, "focus-nav-noop");
    app.select(0);
    key(&mut app, 't');
    assert_eq!(app.selected, 1);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.selected, 0);
    assert_eq!(engine.borrow().active, app.tabs[0].tab_id);
    assert!(engine.borrow().visible);
}

#[test]
fn editing_the_url_navigates_the_current_tab() {
    let (mut app, engine) = start();
    key(&mut app, 'o');
    let edit = app.edit.clone().unwrap();
    assert_eq!(edit.text, HOME_URL);
    assert!(edit.replace_on_type);
    type_text(&mut app, "example.org/x");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].url, "https://example.org/x");
    assert!(app.tabs[0].can_go_back);
    assert_eq!(engine.borrow().url_of(app.tabs[0].tab_id.unwrap()).unwrap(), "https://example.org/x");

    key(&mut app, 'H');
    assert_eq!(app.tabs[0].url, HOME_URL);
    assert!(app.tabs[0].can_go_forward);
    key(&mut app, 'L');
    assert_eq!(app.tabs[0].url, "https://example.org/x");
}

#[test]
fn url_editing_keys() {
    let (mut app, engine) = start();
    key(&mut app, 'o');
    press(&mut app, KeyCode::Left);
    assert!(!app.edit.as_ref().unwrap().replace_on_type, "moving the cursor keeps the text");
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_text(&mut app, "abc def");
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.edit.as_ref().unwrap().text, "abc de");
    app.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert_eq!(app.edit.as_ref().unwrap().text, "abc ");
    press(&mut app, KeyCode::Home);
    type_text(&mut app, "x");
    assert_eq!(app.edit.as_ref().unwrap().text, "xabc ");
    // Option+← and Option+→ arrive as Esc b and Esc f.
    let alt = |app: &mut App, code| app.handle_key(KeyEvent::new(code, KeyModifiers::ALT));
    command(&mut app, "clear-line");
    assert_eq!(app.edit.as_ref().unwrap().text, "", "Cmd+Delete clears the field");
    type_text(&mut app, "example.com/some path");
    alt(&mut app, KeyCode::Char('b'));
    assert_eq!(app.edit.as_ref().unwrap().cursor, 17);
    alt(&mut app, KeyCode::Char('b'));
    assert_eq!(app.edit.as_ref().unwrap().cursor, 12);
    alt(&mut app, KeyCode::Char('f'));
    assert_eq!(app.edit.as_ref().unwrap().cursor, 16);
    alt(&mut app, KeyCode::Char('f'));
    assert_eq!(app.edit.as_ref().unwrap().cursor, 21);
    assert_eq!(app.edit.as_ref().unwrap().text, "example.com/some path", "moving by words types nothing");
    alt(&mut app, KeyCode::Backspace);
    assert_eq!(app.edit.as_ref().unwrap().text, "example.com/some ");
    command(&mut app, "clear-line");
    type_text(&mut app, "abc ");
    press(&mut app, KeyCode::Home);
    type_text(&mut app, "x");
    app.handle_paste("pasted\ntext");
    assert_eq!(app.edit.as_ref().unwrap().text, "xpastedtextabc ");
    command(&mut app, "copy");
    assert_eq!(app.edit.as_ref().unwrap().text, "xpastedtextabc ", "copying leaves the field as it was");
    let copied = engine.borrow().log.iter().rev().find(|r| r["method"] == "clipboard.set").cloned().unwrap();
    assert_eq!(copied["text"], "xpastedtextabc ");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.tabs[0].url, HOME_URL, "cancel leaves the tab alone");
}

#[test]
fn switching_reordering_and_closing() {
    let (mut app, engine) = start();
    open(&mut app, "a.example");
    open(&mut app, "b.example");
    command(&mut app, "toggle-mode");
    assert_eq!(app.mode, Mode::Nav);
    assert_eq!(urls(&app), vec![HOME_URL, "https://a.example", "https://b.example"]);
    assert_eq!(app.selected, 2);

    key(&mut app, 'k');
    assert_eq!(app.selected, 1);
    assert_eq!(engine.borrow().active, app.tabs[1].tab_id);
    key(&mut app, 'k');
    key(&mut app, 'k');
    assert_eq!(app.selected, 0, "j/k do not wrap");

    key(&mut app, 'J');
    assert_eq!(urls(&app), vec!["https://a.example", HOME_URL, "https://b.example"]);
    assert_eq!(app.selected, 1);
    key(&mut app, 'K');
    assert_eq!(urls(&app), vec![HOME_URL, "https://a.example", "https://b.example"]);

    let closing = app.tabs[0].tab_id.unwrap();
    key(&mut app, 'x');
    assert_eq!(urls(&app), vec!["https://a.example", "https://b.example"]);
    assert!(!engine.borrow().tab_ids().contains(&closing));
    assert_eq!(engine.borrow().active, app.tabs[0].tab_id);
    assert_eq!(engine.borrow().tab_ids().len(), 2);
}

#[test]
fn closing_the_last_tab_leaves_a_new_one() {
    let (mut app, engine) = start();
    key(&mut app, 'x');
    assert_eq!(app.tabs.len(), 1);
    assert!(app.edit.is_some());
    assert!(engine.borrow().tab_ids().is_empty());
    press(&mut app, KeyCode::Esc);
    assert_eq!(urls(&app), vec![HOME_URL], "an empty browser reopens the home page");
    assert!(app.tabs[0].tab_id.is_some());
}

#[test]
fn menu_commands() {
    let (mut app, engine) = start();
    open(&mut app, "a.example");
    open(&mut app, "b.example");
    assert_eq!(app.mode, Mode::Page);

    command(&mut app, "next-tab");
    assert_eq!(app.selected, 0, "next-tab wraps");
    command(&mut app, "prev-tab");
    assert_eq!(app.selected, 2);
    app.handle_line(r#"{"event":"command","name":"select-tab","arg":2}"#);
    assert_eq!(app.selected, 1);
    app.handle_line(r#"{"event":"command","name":"select-tab","arg":9}"#);
    assert_eq!(app.selected, 2);
    assert_eq!(app.mode, Mode::Page, "switching tabs from the page keeps page mode");

    command(&mut app, "edit-url");
    assert_eq!(app.mode, Mode::Nav);
    assert_eq!(app.edit.as_ref().unwrap().text, "https://b.example");
    press(&mut app, KeyCode::Esc);

    command(&mut app, "toggle-mode");
    assert_eq!(app.mode, Mode::Page);
    command(&mut app, "toggle-mode");
    assert_eq!(app.mode, Mode::Nav);

    command(&mut app, "close-tab");
    assert_eq!(app.tabs.len(), 2);

    command(&mut app, "quit");
    assert!(app.should_quit);
    assert!(engine.borrow().shut_down);
}

#[test]
fn page_initiated_tabs() {
    let (mut app, engine) = start();
    open(&mut app, "a.example");
    app.select(0);
    let opener = app.tabs[0].tab_id.unwrap();

    let popup = engine.borrow_mut().preload("https://popup.example");
    app.handle_line(&format!(r#"{{"event":"tab.created","tab_id":{popup},"opener_tab_id":{opener},"url":"https://popup.example"}}"#));
    assert_eq!(urls(&app), vec![HOME_URL, "https://popup.example", "https://a.example"]);
    assert_eq!(app.selected, 1, "a new foreground tab is selected");
    assert_eq!(engine.borrow().active, Some(popup));

    let background = engine.borrow_mut().preload("https://bg.example");
    app.handle_line(&format!(r#"{{"event":"tab.created","tab_id":{background},"opener_tab_id":{popup},"url":"https://bg.example","background":true}}"#));
    assert_eq!(urls(&app), vec![HOME_URL, "https://popup.example", "https://bg.example", "https://a.example"]);
    assert_eq!(app.selected, 1, "a background tab does not steal the selection");
    assert_eq!(engine.borrow().active, Some(popup));

    app.handle_line(&format!(r#"{{"event":"tab.closed","tab_id":{popup}}}"#));
    assert_eq!(urls(&app), vec![HOME_URL, "https://bg.example", "https://a.example"]);
    assert_eq!(engine.borrow().active, Some(background));
}

#[test]
fn tab_updates_and_crashes() {
    let (mut app, _engine) = start();
    let id = app.tabs[0].tab_id.unwrap();
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{id},"title":"WebKit","loading":true,"progress":0.4}}"#));
    assert_eq!(app.tabs[0].title, "WebKit");
    assert!(app.tabs[0].loading);
    assert_eq!(app.tabs[0].progress, 0.4);
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{id},"title":"","loading":true}}"#));
    assert_eq!(app.tabs[0].title, "WebKit", "keep the old title while the next page loads");

    app.handle_line(&format!(r#"{{"event":"tab.crashed","tab_id":{id}}}"#));
    assert!(app.tabs[0].crashed);
    assert!(app.message.is_some());
    key(&mut app, 'r');
    assert!(!app.tabs[0].crashed);

    app.handle_line(r#"{"event":"tab.updated","tab_id":999,"title":"ghost"}"#);
    app.handle_line("not json at all");
    assert_eq!(app.tabs.len(), 1);
}

#[test]
fn focus_events_drive_the_mode() {
    let (mut app, _engine) = start();
    assert_eq!(app.mode, Mode::Nav);
    key(&mut app, 'o');
    app.handle_line(r#"{"event":"focus.changed","mode":"page"}"#);
    assert_eq!(app.mode, Mode::Page);
    assert!(app.edit.is_none(), "clicking the page abandons the URL edit");
    app.handle_line(r#"{"event":"focus.changed","mode":"nav"}"#);
    assert_eq!(app.mode, Mode::Nav);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Page);
}

#[test]
fn mouse_selects_tabs_and_opens_the_url_bar() {
    let (mut app, engine) = start();
    open(&mut app, "a.example");
    let click = |app: &mut App, column: u16, row: u16| {
        app.handle_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE });
    };
    // Sidebar rows: 3 is the pinned placeholder, 4 the rule, 5 the first tab.
    click(&mut app, 5, 3);
    click(&mut app, 5, 4);
    assert_eq!(app.selected, 1, "the placeholder and the rule are not tabs");
    click(&mut app, 5, 5);
    assert_eq!(app.selected, 0);
    assert_eq!(engine.borrow().active, app.tabs[0].tab_id);
    click(&mut app, 60, 0);
    assert_eq!(app.edit.as_ref().unwrap().text, HOME_URL);
    press(&mut app, KeyCode::Esc);
    click(&mut app, 5, 38);
    assert_eq!(app.tabs.len(), 3);
    assert!(app.edit.is_some());
}

#[test]
fn session_is_saved_and_restored_lazily() {
    let path = temp_session("restore");
    let _ = std::fs::remove_file(&path);

    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(engine.clone(), Some(path.clone()));
    open(&mut app, "a.example");
    open(&mut app, "b.example");
    app.select(1);
    key(&mut app, 't'); // a blank tab is never saved
    press(&mut app, KeyCode::Esc);
    app.quit();
    let saved = session::load(&path).unwrap();
    assert_eq!(saved.spaces.len(), 1);
    assert!(saved.tabs.is_empty(), "the pre-space fields are no longer written");
    assert_eq!(saved.spaces[0].active, 1);
    assert_eq!(saved.spaces[0].tabs.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(), vec![HOME_URL, "https://a.example", "https://b.example"]);

    // A new app process: nothing is loaded, and only the active tab gets created.
    let fresh = Rc::new(RefCell::new(MockEngine::new()));
    fresh.borrow_mut().mode = "nav";
    let mut saved_other_instance = saved.clone();
    saved_other_instance.instance = Some(999);
    session::save(&path, &saved_other_instance).unwrap();
    let mut app = start_with(fresh.clone(), Some(path.clone()));
    assert_eq!(urls(&app), vec![HOME_URL, "https://a.example", "https://b.example"]);
    assert_eq!(app.selected, 1);
    assert_eq!(fresh.borrow().tab_ids().len(), 1, "only the active tab is loaded eagerly");
    assert!(app.tabs[0].tab_id.is_none());
    assert_eq!(app.tabs[0].title, "webkit.org", "titles come from the session until loaded");
    key(&mut app, 'j');
    assert_eq!(fresh.borrow().tab_ids().len(), 1, "not while the selection is passing by");
    rest(&mut app);
    assert_eq!(fresh.borrow().tab_ids().len(), 2, "others load on first activation");
    assert_eq!(fresh.borrow().active, app.tabs[2].tab_id);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn restart_readopts_tabs_that_are_still_loaded() {
    let path = temp_session("readopt");
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let a = engine.borrow_mut().preload("https://a.example");
    let b = engine.borrow_mut().preload("https://b.example");
    let stray = engine.borrow_mut().preload("https://stray.example");
    session::save(
        &path,
        &Session {
            instance: Some(1), // the mock's instance
            active: 0,
            sidebar_hidden: false,
            tabs: vec![
                SessionTab { url: "https://b.example".into(), title: "B".into(), tab_id: Some(b), ..Default::default() },
                SessionTab { url: "https://gone.example".into(), title: "Gone".into(), tab_id: Some(77), ..Default::default() },
                SessionTab { url: "https://a.example".into(), title: "A".into(), tab_id: Some(a), ..Default::default() },
            ],
            ..Default::default()
        },
    )
    .unwrap();

    let app = start_with(engine.clone(), Some(path.clone()));
    assert_eq!(urls(&app), vec!["https://b.example", "https://gone.example", "https://a.example", "https://stray.example"]);
    assert_eq!(app.tabs[0].tab_id, Some(b));
    assert_eq!(app.tabs[1].tab_id, None, "a tab the app no longer has is reloaded on demand");
    assert_eq!(app.tabs[2].tab_id, Some(a));
    assert_eq!(app.tabs[3].tab_id, Some(stray));
    let engine = engine.borrow();
    assert_eq!(engine.tab_ids().len(), 3, "nothing was created again");
    assert_eq!(engine.active, Some(b));
    assert!(!engine.log.iter().any(|r| r["method"] == "tab.create"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn closing_a_tab_before_it_finishes_opening_does_not_leak_it() {
    // An engine that answers late: requests are queued and replies delivered by hand.
    struct Deferred(Engine, Rc<RefCell<Vec<String>>>);
    impl crate::transport::Transport for Deferred {
        fn send(&mut self, line: &str) -> Vec<String> {
            let replies = self.0.borrow_mut().handle(line);
            if line.contains("\"tab.create\"") {
                self.1.borrow_mut().extend(replies);
                Vec::new()
            } else {
                replies
            }
        }
    }
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let held = Rc::new(RefCell::new(Vec::new()));
    let mut app = App::new(Box::new(Deferred(engine.clone(), held.clone())), None);
    app.start();
    app.set_area(Rect::new(0, 0, 120, 40));
    assert!(app.tabs[0].pending);
    key(&mut app, 'x');
    for line in held.borrow_mut().drain(..).collect::<Vec<_>>() {
        app.handle_line(&line);
    }
    assert!(engine.borrow().tab_ids().is_empty(), "the late tab was closed");
}

#[test]
fn layout_adapts_to_small_terminals() {
    let (mut app, engine) = start();
    app.set_area(Rect::new(0, 0, 80, 24));
    assert_eq!(engine.borrow().viewport, Some((28, 1, 52, 22)));
    for size in [(0, 0), (1, 1), (10, 2), (28, 3), (29, 1), (200, 60)] {
        app.set_area(Rect::new(0, 0, size.0, size.1));
    }
    assert_eq!(engine.borrow().viewport, Some((28, 1, 172, 58)));
}

fn labels(app: &App) -> Vec<String> {
    app.tabs.iter().map(|t| t.label()).collect()
}

/// Three tabs: home, a.example, b.example; NAV mode; the last one selected.
fn three_tabs() -> (App, Engine) {
    let (mut app, engine) = start();
    open(&mut app, "a.example");
    open(&mut app, "b.example");
    command(&mut app, "toggle-mode");
    assert_eq!(app.mode, Mode::Nav);
    (app, engine)
}

#[test]
fn sidebar_toggle_gives_the_page_the_full_width() {
    let (mut app, engine) = start();
    assert_eq!(engine.borrow().viewport, Some((28, 1, 92, 38)));
    key(&mut app, 'b');
    app.set_area(Rect::new(0, 0, 120, 40));
    assert!(!app.sidebar_visible);
    assert_eq!(engine.borrow().viewport, Some((0, 1, 120, 38)));
    assert_eq!(app.layout.tab_list.height, 0);
    // Tabs still switch, and clicks where the sidebar was do nothing.
    open(&mut app, "a.example");
    command(&mut app, "toggle-mode");
    app.handle_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 5, row: 5, modifiers: KeyModifiers::NONE });
    assert_eq!(app.selected, 1);
    key(&mut app, 'k');
    assert_eq!(app.selected, 0);

    command(&mut app, "toggle-sidebar");
    app.set_area(Rect::new(0, 0, 120, 40));
    assert!(app.sidebar_visible);
    assert_eq!(engine.borrow().viewport, Some((28, 1, 92, 38)));
}

#[test]
fn pinning_moves_tabs_between_sections() {
    let (mut app, _engine) = three_tabs();
    assert_eq!(app.side_rows(), vec![SideRow::PinPlaceholder, SideRow::Separator, SideRow::Tab(0), SideRow::Tab(1), SideRow::Tab(2)]);

    key(&mut app, 'p'); // pin b
    assert_eq!(urls(&app), vec!["https://b.example", HOME_URL, "https://a.example"]);
    assert_eq!(app.selected, 0);
    assert!(app.tabs[0].pinned);
    assert_eq!(app.tabs[0].pin_url.as_deref(), Some("https://b.example"));
    assert_eq!(app.side_rows(), vec![SideRow::Tab(0), SideRow::Separator, SideRow::Tab(1), SideRow::Tab(2)]);

    app.select(2);
    command(&mut app, "toggle-pin"); // pin a: goes after b
    assert_eq!(urls(&app), vec!["https://b.example", "https://a.example", HOME_URL]);
    assert_eq!(app.selected, 1);
    assert_eq!(app.pinned_count(), 2);

    // Reordering stays inside a section.
    key(&mut app, 'J');
    assert_eq!(urls(&app), vec!["https://b.example", "https://a.example", HOME_URL], "a pinned tab cannot move below the rule");
    key(&mut app, 'K');
    assert_eq!(urls(&app), vec!["https://a.example", "https://b.example", HOME_URL]);
    app.select(2);
    key(&mut app, 'K');
    assert_eq!(app.selected, 2, "an unpinned tab cannot move above the rule");

    // Unpinning puts the tab at the top of the unpinned section.
    app.select(0);
    key(&mut app, 'p');
    assert_eq!(urls(&app), vec!["https://b.example", "https://a.example", HOME_URL]);
    assert_eq!(app.selected, 1);
    assert!(!app.tabs[1].pinned);
    assert_eq!(app.tabs[1].pin_url, None);
}

#[test]
fn new_tabs_never_land_in_the_pinned_section() {
    let (mut app, engine) = three_tabs();
    app.select(0);
    key(&mut app, 'p');
    app.select(1);
    key(&mut app, 'p');
    app.select(0); // a pinned tab is selected
    key(&mut app, 't');
    assert_eq!(app.selected, 2, "the new tab opens below the rule");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.selected, 0);

    let opener = app.tabs[0].tab_id.unwrap();
    let popup = engine.borrow_mut().preload("https://popup.example");
    app.handle_line(&format!(r#"{{"event":"tab.created","tab_id":{popup},"opener_tab_id":{opener},"url":"https://popup.example"}}"#));
    assert_eq!(app.tabs[2].url, "https://popup.example");
    assert!(!app.tabs[2].pinned);
    assert_eq!(app.selected, 2);
}

#[test]
fn a_pinned_tab_returns_to_its_pinned_page() {
    let (mut app, engine) = three_tabs();
    key(&mut app, 'p');
    key(&mut app, 'o');
    type_text(&mut app, "b.example/deep/link");
    press(&mut app, KeyCode::Enter);
    command(&mut app, "toggle-mode");
    assert_eq!(app.tabs[0].url, "https://b.example/deep/link");
    assert_eq!(app.tabs[0].pin_url.as_deref(), Some("https://b.example"), "browsing does not move the pin");

    key(&mut app, 'P');
    assert_eq!(app.tabs[0].url, "https://b.example");
    assert_eq!(engine.borrow().url_of(app.tabs[0].tab_id.unwrap()).unwrap(), "https://b.example");
    assert!(app.tabs[0].can_go_back, "going home is a navigation, not a reset");

    app.select(1);
    command(&mut app, "pin-home");
    assert!(app.message.as_deref().unwrap().contains("not pinned"));
    assert_eq!(app.tabs[1].url, HOME_URL);
}

#[test]
fn a_pinned_tab_keeps_its_name_within_its_site() {
    let (mut app, _engine) = three_tabs();
    let id = app.tabs[2].tab_id.unwrap();
    let update = |app: &mut App, url: &str, title: &str| {
        app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{id},"url":"{url}","title":"{title}"}}"#));
    };
    update(&mut app, "https://b.example", "Bee Mail");
    key(&mut app, 'p');
    assert_eq!(app.tabs[0].pin_title.as_deref(), Some("Bee Mail"));

    update(&mut app, "https://www.b.example/inbox/42", "Re: lunch");
    assert_eq!(app.tabs[0].label(), "Bee Mail", "another page of the same site");
    update(&mut app, "https://other.example/", "Elsewhere");
    assert_eq!(app.tabs[0].label(), "Elsewhere", "another site shows its own title");
    update(&mut app, "https://b.example/sent", "Sent");
    assert_eq!(app.tabs[0].label(), "Bee Mail", "back on the pinned site");

    key(&mut app, 'n');
    type_text(&mut app, "Post");
    press(&mut app, KeyCode::Enter);
    update(&mut app, "https://other.example/", "Elsewhere");
    assert_eq!(app.tabs[0].label(), "Post", "a name the user gave always shows");
    app.tabs[0].name = None;

    key(&mut app, 'p'); // unpin
    assert_eq!(app.tabs[0].pin_title, None);
    update(&mut app, "https://b.example/sent", "Sent");
    assert_eq!(app.tabs[0].label(), "Sent");
}

#[test]
fn a_pin_without_a_title_is_named_by_its_pinned_page() {
    let (mut app, _engine) = three_tabs();
    let id = app.tabs[2].tab_id.unwrap();
    app.tabs[2].title.clear();
    key(&mut app, 'p');
    assert_eq!(app.tabs[0].pin_title, None);
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{id},"url":"https://b.example","title":"Bee Mail"}}"#));
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{id},"url":"https://b.example/x","title":"Deep"}}"#));
    assert_eq!(app.tabs[0].label(), "Bee Mail");
}

#[test]
fn closing_a_pinned_tab_unloads_it_but_keeps_the_pin() {
    let (mut app, engine) = three_tabs();
    key(&mut app, 'p');
    key(&mut app, 'o');
    type_text(&mut app, "b.example/elsewhere");
    press(&mut app, KeyCode::Enter);
    command(&mut app, "toggle-mode");
    let tab_id = app.tabs[0].tab_id.unwrap();

    key(&mut app, 'x');
    assert_eq!(app.tabs.len(), 3, "the pin stays");
    assert!(app.tabs[0].pinned);
    assert_eq!(app.tabs[0].tab_id, None);
    assert_eq!(app.tabs[0].url, "https://b.example", "an unloaded pin is back at its pinned page");
    assert!(!engine.borrow().tab_ids().contains(&tab_id));
    assert_eq!(app.selected, 1, "the selection moves to the first unpinned tab");
    assert_eq!(engine.borrow().active, app.tabs[1].tab_id);
    assert_eq!(engine.borrow().tab_ids().len(), 2, "the unloaded pin is not loaded again behind our back");

    // Selecting it loads it again, at the pinned page.
    key(&mut app, 'k');
    rest(&mut app);
    assert_eq!(app.selected, 0);
    let reloaded = app.tabs[0].tab_id.expect("loaded again");
    assert_eq!(engine.borrow().url_of(reloaded).unwrap(), "https://b.example");

    // A page closing itself is treated the same way.
    app.handle_line(&format!(r#"{{"event":"tab.closed","tab_id":{reloaded}}}"#));
    assert_eq!(app.tabs.len(), 3);
    assert_eq!(app.tabs[0].tab_id, None);

    // Unpinned, it closes for good.
    key(&mut app, 'k');
    key(&mut app, 'p');
    key(&mut app, 'x');
    assert_eq!(urls(&app), vec![HOME_URL, "https://a.example"]);
}

#[test]
fn closing_the_only_tab_when_it_is_pinned_opens_a_new_one() {
    let (mut app, _engine) = start();
    key(&mut app, 'p');
    key(&mut app, 'x');
    assert_eq!(app.tabs.len(), 2);
    assert!(app.tabs[0].pinned && app.tabs[0].tab_id.is_none());
    assert_eq!(app.selected, 1);
    assert!(app.edit.is_some(), "a blank tab with the address field open");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.selected, 0, "cancelling falls back to the pinned tab");
    assert!(app.tabs[0].tab_id.is_some());
}

#[test]
fn renaming_tabs() {
    let (mut app, _engine) = three_tabs();
    key(&mut app, 'n');
    let edit = app.edit.clone().unwrap();
    assert_eq!(edit.kind, EditKind::Rename);
    assert_eq!(edit.text, "b.example");
    assert!(edit.replace_on_type);
    type_text(&mut app, "Work mail");
    press(&mut app, KeyCode::Enter);
    assert!(app.edit.is_none());
    assert_eq!(labels(&app), vec!["webkit.org", "a.example", "Work mail"]);
    assert_eq!(app.tabs[2].url, "https://b.example", "renaming does not navigate");
    assert_eq!(app.mode, Mode::Nav);

    // The name survives navigation and title changes.
    let id = app.tabs[2].tab_id.unwrap();
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{id},"title":"Inbox (3)"}}"#));
    assert_eq!(app.tabs[2].label(), "Work mail");

    // Escape keeps the old name; an empty name restores the title.
    key(&mut app, 'n');
    type_text(&mut app, "zzz");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.tabs[2].label(), "Work mail");
    assert_eq!(app.tabs.len(), 3);
    command(&mut app, "rename-tab");
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.tabs[2].label(), "Inbox (3)");

    // Pinned tabs can be renamed too.
    key(&mut app, 'p');
    key(&mut app, 'n');
    type_text(&mut app, "Mail");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.tabs[0].label(), "Mail");
    assert!(app.tabs[0].pinned);
}

#[test]
fn pins_names_and_sidebar_survive_a_restart() {
    let path = temp_session("pins");
    let _ = std::fs::remove_file(&path);
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(engine, Some(path.clone()));
    open(&mut app, "a.example");
    open(&mut app, "b.example");
    command(&mut app, "toggle-mode");
    key(&mut app, 'p'); // pin b
    key(&mut app, 'n');
    type_text(&mut app, "Bee");
    press(&mut app, KeyCode::Enter);
    key(&mut app, 'o');
    type_text(&mut app, "b.example/later");
    press(&mut app, KeyCode::Enter);
    command(&mut app, "toggle-mode");
    key(&mut app, 'b');
    app.quit();

    let mut saved = session::load(&path).unwrap();
    assert!(saved.sidebar_hidden);
    assert!(saved.spaces[0].tabs[0].pinned);
    saved.instance = Some(999); // a different app process: nothing is loaded
    session::save(&path, &saved).unwrap();

    let fresh = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(fresh.clone(), Some(path.clone()));
    assert!(!app.sidebar_visible);
    assert_eq!(app.pinned_count(), 1);
    assert_eq!(app.tabs[0].label(), "Bee");
    assert_eq!(app.tabs[0].url, "https://b.example/later", "a pin reopens where it was left");
    assert_eq!(app.tabs[0].pin_url.as_deref(), Some("https://b.example"));
    assert_eq!(app.tabs[0].pin_title.as_deref(), Some("b.example"), "the pinned title is saved too");
    assert_eq!(app.selected, 0);
    key(&mut app, 'P');
    assert_eq!(fresh.borrow().url_of(app.tabs[0].tab_id.unwrap()).unwrap(), "https://b.example");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn restore_puts_pinned_tabs_first_and_keeps_the_selection() {
    let path = temp_session("order");
    let tab = |url: &str, pinned: bool| SessionTab { url: url.into(), pinned, ..Default::default() };
    session::save(
        &path,
        &Session {
            instance: Some(999),
            active: 0,
            sidebar_hidden: false,
            tabs: vec![tab("https://one.example", false), tab("https://two.example", true), tab("https://three.example", false)],
            ..Default::default()
        },
    )
    .unwrap();
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let app = start_with(engine, Some(path.clone()));
    assert_eq!(urls(&app), vec!["https://two.example", "https://one.example", "https://three.example"]);
    assert_eq!(app.selected, 1, "still on one.example");
    assert_eq!(app.tabs[0].pin_url.as_deref(), Some("https://two.example"), "a pin without a saved address uses its URL");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn key_reference_replaces_the_page_until_a_key_is_pressed() {
    let (mut app, engine) = three_tabs();
    key(&mut app, '?');
    assert!(app.help);
    assert!(!engine.borrow().visible, "the page is hidden while the reference shows");
    key(&mut app, 'x');
    assert!(!app.help);
    assert_eq!(app.tabs.len(), 3, "the dismissing key does nothing else");
    assert!(engine.borrow().visible);
    assert_eq!(app.mode, Mode::Nav);

    // From the page, it returns to the page.
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Page);
    command(&mut app, "help");
    assert!(app.help);
    assert_eq!(app.mode, Mode::Nav);
    press(&mut app, KeyCode::Esc);
    assert!(!app.help);
    assert_eq!(app.mode, Mode::Page);
    assert!(engine.borrow().visible);

    // Another command closes it and then runs.
    command(&mut app, "help");
    command(&mut app, "prev-tab");
    assert!(!app.help);
    assert_eq!(app.selected, 1);
    assert!(engine.borrow().visible);
}

#[test]
fn sidebar_scrolls_to_keep_the_selection_visible() {
    let (mut app, _engine) = start();
    for i in 0..12 {
        open(&mut app, &format!("site{i}.example"));
    }
    command(&mut app, "toggle-mode");
    app.set_area(Rect::new(0, 0, 100, 13)); // 8 list rows for 2 + 13 rows
    assert_eq!(app.layout.tab_list.height, 8);
    let rows = app.side_rows();
    let visible = |app: &App| {
        let row = rows.iter().position(|r| *r == SideRow::Tab(app.selected)).unwrap();
        row >= app.scroll && row < app.scroll + 8
    };
    assert!(visible(&app), "the last tab is in view");
    for _ in 0..12 {
        key(&mut app, 'k');
        assert!(visible(&app));
    }
    assert_eq!(app.scroll, 0, "back at the top, the placeholder shows again");
}

/// Fills in the space editor and saves it.
fn fill_space_form(app: &mut App, name: &str, color_steps: usize, profile: &str) {
    assert!(app.form.is_some(), "the space editor is open");
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_text(app, name);
    press(app, KeyCode::Tab);
    for _ in 0..color_steps {
        press(app, KeyCode::Right);
    }
    press(app, KeyCode::Tab); // background
    press(app, KeyCode::Tab); // art
    press(app, KeyCode::Tab); // profile
    assert_eq!(app.form.as_ref().unwrap().field, FormField::Profile);
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_text(app, profile);
    press(app, KeyCode::Enter);
}

fn new_space(app: &mut App, name: &str, profile: &str) {
    key(app, 'N');
    fill_space_form(app, name, 0, profile);
    // A new space opens on a blank tab with the address field ready.
    assert!(app.edit.is_some());
}

#[test]
fn a_fresh_browser_has_one_space_on_the_default_profile() {
    let (mut app, engine) = start();
    assert_eq!(app.spaces.len(), 1);
    assert_eq!(app.current_space().name, "Home");
    assert_eq!(app.current_space().profile, "default");
    assert_eq!(engine.borrow().profile_of(app.tabs[0].tab_id.unwrap()).unwrap(), "default");
    key(&mut app, 'l');
    assert!(app.message.as_deref().unwrap().contains("only one space"));
    key(&mut app, 'D');
    assert!(!app.confirm_delete, "the last space cannot be deleted");
    assert!(app.message.as_deref().unwrap().contains("cannot be deleted"));
}

#[test]
fn each_space_has_its_own_tabs_and_pins() {
    let (mut app, engine) = three_tabs();
    key(&mut app, 'p'); // pin b.example in Home
    assert_eq!(app.pinned_count(), 1);

    new_space(&mut app, "Work", "");
    assert!(!engine.borrow().visible, "the editor and then the blank tab hide the page");
    assert_eq!(app.spaces.len(), 2);
    assert_eq!(app.space, 1);
    assert_eq!(app.current_space().name, "Work");
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.pinned_count(), 0, "pins belong to the space they were made in");
    type_text(&mut app, "work.example");
    press(&mut app, KeyCode::Enter);
    command(&mut app, "toggle-mode");
    assert_eq!(urls(&app), vec!["https://work.example"]);
    let work_tab = app.tabs[0].tab_id.unwrap();
    assert_eq!(engine.borrow().active, Some(work_tab));
    assert_eq!(engine.borrow().tab_ids().len(), 4, "the other space's tabs stay loaded");

    key(&mut app, 'h');
    assert_eq!(app.space, 0);
    assert_eq!(urls(&app), vec!["https://b.example", HOME_URL, "https://a.example"]);
    assert_eq!(app.pinned_count(), 1);
    assert_eq!(app.selected, 0, "each space remembers its selection");
    assert_eq!(engine.borrow().active, app.tabs[0].tab_id);
    assert!(engine.borrow().visible);
    assert!(!engine.borrow().log.iter().any(|r| r["method"] == "tab.create" && r["id"].as_u64() > Some(100)), "nothing reloads on a switch");

    key(&mut app, 'h'); // wraps
    assert_eq!(app.space, 1);
    assert_eq!(engine.borrow().active, Some(work_tab));
    app.handle_line(r#"{"event":"command","name":"select-space","arg":1}"#);
    assert_eq!(app.space, 0);
    app.handle_line(r#"{"event":"command","name":"select-space","arg":7}"#);
    assert_eq!(app.space, 0, "there is no seventh space");
    command(&mut app, "next-space");
    assert_eq!(app.space, 1);
}

#[test]
fn spaces_keep_their_profiles_apart() {
    let (mut app, engine) = start();
    new_space(&mut app, "Work Stuff", "");
    assert_eq!(app.current_space().profile, "work-stuff", "a new space gets a profile of its own by default");
    type_text(&mut app, "work.example");
    press(&mut app, KeyCode::Enter);
    command(&mut app, "toggle-mode");
    let work = app.tabs[0].tab_id.unwrap();
    assert_eq!(engine.borrow().profile_of(work).unwrap(), "work-stuff");

    // A second space can share an existing profile by naming it.
    new_space(&mut app, "Work admin", " Work  stuff ");
    assert_eq!(app.current_space().profile, "work-stuff");
    type_text(&mut app, "admin.example");
    press(&mut app, KeyCode::Enter);
    command(&mut app, "toggle-mode");
    assert_eq!(engine.borrow().profile_of(app.tabs[0].tab_id.unwrap()).unwrap(), "work-stuff");

    // And "default" joins the first space's logins.
    new_space(&mut app, "Scratch", "default");
    type_text(&mut app, "scratch.example");
    press(&mut app, KeyCode::Enter);
    assert_eq!(engine.borrow().profile_of(app.tabs[0].tab_id.unwrap()).unwrap(), "default");
    let request = engine.borrow().log.iter().rev().find(|r| r["method"] == "tab.create").cloned().unwrap();
    assert!(request.get("profile").is_none(), "the default profile is not named on the wire");
}

#[test]
fn editing_a_space_changes_name_colour_and_profile() {
    let (mut app, engine) = three_tabs();
    key(&mut app, 'p'); // b.example pinned
    let before = engine.borrow().tab_ids();
    key(&mut app, 'e');
    let form = app.form.clone().unwrap();
    assert!(form.editing);
    assert_eq!((form.name.as_str(), form.color, form.profile.as_str()), ("Home", 0, "default"));
    assert!(!engine.borrow().visible);

    // Name and colour only: nothing is reloaded.
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_text(&mut app, "Personal");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.form.as_ref().unwrap().field, FormField::Color);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Left);
    type_text(&mut app, "zzz"); // letters do nothing on the colour field
    press(&mut app, KeyCode::Enter);
    assert!(app.form.is_none());
    assert_eq!((app.current_space().name.as_str(), app.current_space().color, app.current_space().profile.as_str()), ("Personal", 1, "default"));
    assert_eq!(engine.borrow().tab_ids(), before);
    assert!(engine.borrow().visible);

    // Changing the profile closes the pages and reopens the selected one in it.
    key(&mut app, 'e');
    press(&mut app, KeyCode::BackTab);
    assert_eq!(app.form.as_ref().unwrap().field, FormField::Profile);
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_text(&mut app, "me");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.current_space().profile, "me");
    assert_eq!(urls(&app), vec!["https://b.example", HOME_URL, "https://a.example"], "tabs and pins are kept");
    assert!(app.tabs[0].pinned);
    let ids = engine.borrow().tab_ids();
    assert_eq!(ids.len(), 1, "only the selected tab is open again");
    assert!(before.iter().all(|id| !ids.contains(id)), "the old pages are gone");
    assert_eq!(engine.borrow().profile_of(ids[0]).unwrap(), "me");
    assert_eq!(engine.borrow().active, Some(ids[0]));
    key(&mut app, 'j');
    rest(&mut app);
    assert_eq!(engine.borrow().profile_of(app.tabs[1].tab_id.unwrap()).unwrap(), "me");
}

#[test]
fn the_space_editor_can_be_cancelled_and_needs_a_name() {
    let (mut app, engine) = start();
    key(&mut app, 'N');
    press(&mut app, KeyCode::Enter);
    let form = app.form.clone().unwrap();
    assert!(form.error.is_some(), "an empty name is refused");
    assert_eq!(app.spaces.len(), 1);
    type_text(&mut app, "x");
    assert!(app.form.as_ref().unwrap().error.is_none());
    press(&mut app, KeyCode::Esc);
    assert!(app.form.is_none());
    assert_eq!(app.spaces.len(), 1);
    assert!(engine.borrow().visible, "the page is back");

    // A new space is offered a colour that is not in use.
    key(&mut app, 'N');
    assert_eq!(app.form.as_ref().unwrap().color, 1);
    // Another command dismisses the editor.
    command(&mut app, "new-tab");
    assert!(app.form.is_none());
    assert!(app.edit.is_some());
}

#[test]
fn deleting_a_space_closes_its_tabs() {
    let (mut app, engine) = three_tabs();
    new_space(&mut app, "Work", "");
    type_text(&mut app, "work.example");
    press(&mut app, KeyCode::Enter);
    command(&mut app, "toggle-mode");
    open(&mut app, "work2.example");
    command(&mut app, "toggle-mode");
    assert_eq!(engine.borrow().tab_ids().len(), 5);

    key(&mut app, 'D');
    assert!(app.confirm_delete);
    assert!(!engine.borrow().visible);
    key(&mut app, 'n');
    assert!(!app.confirm_delete);
    assert_eq!(app.spaces.len(), 2, "any key but y keeps the space");
    assert!(engine.borrow().visible);

    command(&mut app, "delete-space");
    key(&mut app, 'y');
    assert_eq!(app.spaces.len(), 1);
    assert_eq!(app.current_space().name, "Home");
    assert_eq!(urls(&app), vec![HOME_URL, "https://a.example", "https://b.example"]);
    assert_eq!(engine.borrow().tab_ids().len(), 3, "the deleted space's pages are closed");
    assert_eq!(engine.borrow().active, app.tabs[app.selected].tab_id);
    assert!(engine.borrow().visible);
}

#[test]
fn events_reach_tabs_in_other_spaces() {
    let (mut app, engine) = three_tabs();
    key(&mut app, 'p'); // b pinned, selected
    let pinned = app.tabs[0].tab_id.unwrap();
    let home_tab = app.tabs[1].tab_id.unwrap();
    new_space(&mut app, "Work", "");
    type_text(&mut app, "work.example");
    press(&mut app, KeyCode::Enter);
    let work_tab = app.tabs[0].tab_id.unwrap();

    // Title changes
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{home_tab},"title":"Changed in the background"}}"#));
    assert_eq!(app.spaces[0].tabs[1].title, "Changed in the background");

    // A page in the other space opens a tab: it lands there, next to its opener.
    let popup = engine.borrow_mut().preload("https://popup.example");
    app.handle_line(&format!(r#"{{"event":"tab.created","tab_id":{popup},"opener_tab_id":{home_tab},"url":"https://popup.example"}}"#));
    assert_eq!(app.tabs.len(), 1, "not in the space being looked at");
    assert_eq!(app.spaces[0].tabs[2].url, "https://popup.example");
    assert_eq!(engine.borrow().active, Some(work_tab), "and it does not take over the window");

    // Pages closing themselves: an ordinary tab goes, a pinned one unloads.
    app.handle_line(&format!(r#"{{"event":"tab.closed","tab_id":{popup}}}"#));
    assert_eq!(app.spaces[0].tabs.len(), 3);
    app.handle_line(&format!(r#"{{"event":"tab.closed","tab_id":{pinned}}}"#));
    assert_eq!(app.spaces[0].tabs.len(), 3);
    assert_eq!(app.spaces[0].tabs[0].tab_id, None);
    assert!(app.spaces[0].tabs[0].pinned);

    app.handle_line(&format!(r#"{{"event":"tab.crashed","tab_id":{home_tab}}}"#));
    assert!(app.spaces[0].tabs[1].crashed);
    assert!(app.message.is_none(), "a crash elsewhere is not announced over this space");
}

#[test]
fn spaces_survive_a_restart() {
    let path = temp_session("spaces");
    let _ = std::fs::remove_file(&path);
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(engine, Some(path.clone()));
    open(&mut app, "a.example");
    command(&mut app, "toggle-mode");
    key(&mut app, 'N');
    fill_space_form(&mut app, "Work", 2, "");
    type_text(&mut app, "work.example");
    press(&mut app, KeyCode::Enter);
    command(&mut app, "toggle-mode");
    key(&mut app, 'p');
    open(&mut app, "work2.example");
    command(&mut app, "toggle-mode");
    app.quit();

    let mut saved = session::load(&path).unwrap();
    assert_eq!(saved.current, 1);
    assert_eq!(saved.spaces.iter().map(|l| (l.name.as_str(), l.profile.as_str())).collect::<Vec<_>>(), vec![("Home", "default"), ("Work", "work")]);
    assert_eq!(saved.spaces[1].color, 3, "the suggested colour 1, moved two steps");
    saved.instance = Some(999);
    session::save(&path, &saved).unwrap();

    let fresh = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(fresh.clone(), Some(path.clone()));
    assert_eq!(app.spaces.len(), 2);
    assert_eq!(app.space, 1);
    assert_eq!(app.current_space().color, 3);
    assert_eq!(urls(&app), vec!["https://work.example", "https://work2.example"]);
    assert!(app.tabs[0].pinned);
    assert_eq!(app.selected, 1);
    assert_eq!(fresh.borrow().tab_ids().len(), 1, "only the showing tab of the showing space is loaded");
    assert_eq!(fresh.borrow().profile_of(app.tabs[1].tab_id.unwrap()).unwrap(), "work");
    key(&mut app, 'l');
    assert_eq!(urls(&app), vec![HOME_URL, "https://a.example"]);
    assert_eq!(app.selected, 1);
    assert_eq!(fresh.borrow().profile_of(app.tabs[1].tab_id.unwrap()).unwrap(), "default");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_restarted_tui_readopts_tabs_in_every_space() {
    let path = temp_session("space-readopt");
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let home = engine.borrow_mut().preload("https://home.example");
    let work = engine.borrow_mut().preload("https://work.example");
    let tab = |url: &str, id: u64| SessionTab { url: url.into(), tab_id: Some(id), ..Default::default() };
    session::save(
        &path,
        &Session {
            instance: Some(1),
            current: 1,
            spaces: vec![
                SessionSpace { name: "Home".into(), profile: "default".into(), tabs: vec![tab("https://home.example", home)], ..Default::default() },
                SessionSpace { name: "Work".into(), color: 4, profile: "work".into(), tabs: vec![tab("https://work.example", work)], ..Default::default() },
            ],
            ..Default::default()
        },
    )
    .unwrap();
    let app = start_with(engine.clone(), Some(path.clone()));
    assert_eq!(app.tabs[0].tab_id, Some(work));
    assert_eq!(app.spaces[0].tabs[0].tab_id, Some(home));
    assert!(!engine.borrow().log.iter().any(|r| r["method"] == "tab.create"), "nothing is opened twice");
    assert_eq!(engine.borrow().active, Some(work));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn switching_space_while_a_tab_is_still_opening_does_not_lose_it() {
    struct Deferred(Engine, Rc<RefCell<Vec<String>>>, Rc<RefCell<bool>>);
    impl crate::transport::Transport for Deferred {
        fn send(&mut self, line: &str) -> Vec<String> {
            let replies = self.0.borrow_mut().handle(line);
            if *self.2.borrow() && line.contains("\"tab.create\"") {
                self.1.borrow_mut().extend(replies);
                Vec::new()
            } else {
                replies
            }
        }
    }
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let held = Rc::new(RefCell::new(Vec::new()));
    let hold = Rc::new(RefCell::new(false));
    let mut app = App::new(Box::new(Deferred(engine.clone(), held.clone(), hold.clone())), None);
    app.start();
    app.set_area(Rect::new(0, 0, 120, 40));
    let home_tab = app.tabs[0].tab_id.unwrap();
    new_space(&mut app, "Work", "");
    *hold.borrow_mut() = true;
    type_text(&mut app, "slow.example");
    press(&mut app, KeyCode::Enter);
    assert!(app.tabs[0].pending);
    key(&mut app, 'h'); // back to Home before the tab exists
    assert_eq!(app.space, 0);
    for line in held.borrow_mut().drain(..).collect::<Vec<_>>() {
        app.handle_line(&line);
    }
    let slow = app.spaces[1].tabs[0].tab_id.expect("the tab is recorded in its space");
    assert!(engine.borrow().tab_ids().contains(&slow));
    assert_eq!(engine.borrow().active, Some(home_tab), "Home's tab is showing again");
}

#[test]
fn clicking_a_space_dot_switches_to_it() {
    let (mut app, _engine) = start();
    new_space(&mut app, "Work", "");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.space, 1);
    let click = |app: &mut App, column: u16| {
        app.handle_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row: 1, modifiers: KeyModifiers::NONE });
    };
    click(&mut app, 2); // between the dots
    assert_eq!(app.space, 1);
    click(&mut app, 1);
    assert_eq!(app.space, 0);
    click(&mut app, 3);
    assert_eq!(app.space, 1);
    click(&mut app, 9); // no space there
    assert_eq!(app.space, 1);
}

/// Draws the interface into an off-screen buffer and returns it as text.
fn render(app: &mut App, width: u16, height: u16) -> String {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            app.set_area(frame.area());
            crate::ui::draw(frame, app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[test]
fn the_screen_shows_the_space_its_pins_and_its_tabs() {
    let (mut app, _engine) = three_tabs();
    key(&mut app, 'p');
    key(&mut app, 'n');
    type_text(&mut app, "Mail");
    press(&mut app, KeyCode::Enter);
    new_space(&mut app, "Work", "");
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'h');
    let screen = render(&mut app, 90, 16);
    println!("{screen}");
    let lines: Vec<&str> = screen.lines().collect();
    assert!(lines[0].starts_with(" ~/Home"), "the space's name heads the sidebar");
    assert!(lines[1].starts_with(" ● •"), "one dot per space, the current one the larger");
    assert!(lines[3].contains("Mail"), "pinned tabs come first");
    assert!(lines[4].contains("╌╌╌╌╌╌"), "then the rule");
    assert!(lines[5].contains("webkit.org") && lines[6].contains("a.example"));
    assert!(lines[0].contains("▐b.example") && !lines[0].contains("https://"), "the toolbar shows the address, site first");
    assert!(lines[15].contains("NAV") && lines[15].contains("3 tabs"));

    // With the sidebar hidden the status line says where you are.
    key(&mut app, 'b');
    let screen = render(&mut app, 90, 16);
    let last = screen.lines().last().unwrap();
    assert!(last.contains("Home") && last.contains("1/3") && last.contains("pinned") && last.contains("Mail"), "{last}");
    assert!(screen.lines().next().unwrap().trim_start().starts_with('←'), "the toolbar starts at the left edge");
}

#[test]
fn the_screen_shows_the_empty_pin_placeholder() {
    let (mut app, _engine) = start();
    let screen = render(&mut app, 90, 12);
    assert!(screen.lines().nth(3).unwrap().contains("No pinned tabs"));
    assert!(screen.lines().nth(10).unwrap().contains("+ New tab"));
}

#[test]
fn the_space_editor_and_the_delete_question_are_drawn_in_the_page_area() {
    let (mut app, _engine) = start();
    key(&mut app, 'N');
    type_text(&mut app, "Side Project");
    press(&mut app, KeyCode::Tab);
    let screen = render(&mut app, 100, 24);
    println!("{screen}");
    assert!(screen.lines().next().unwrap().starts_with(" ~/Side Project"), "the sidebar previews the name being typed");
    assert!(screen.contains("New space"));
    assert!(screen.contains("Side Project"));
    assert!(screen.contains("[●]") && screen.contains("green") && screen.contains("← → to change"));
    assert!(screen.contains("Background") && screen.contains("ink"));
    assert!(screen.contains("‹ none ›"));
    assert!(screen.contains("its own: side-project"), "the profile it will get is shown before saving");
    assert!(screen.contains("share logins"));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Esc);

    key(&mut app, 'e');
    assert!(render(&mut app, 100, 24).contains("Edit space"));
    press(&mut app, KeyCode::Esc);

    key(&mut app, 'D');
    let screen = render(&mut app, 100, 24);
    println!("{screen}");
    assert!(screen.contains("Delete space \"Side Project\"?"));
    assert!(screen.contains("y  delete"));
    key(&mut app, 'n');

    key(&mut app, '?');
    let screen = render(&mut app, 120, 30);
    assert!(screen.contains("Previous / next space") && screen.contains("Edit space") && screen.contains("NAV mode"));
    // Small windows must not panic.
    for (w, h) in [(1, 1), (30, 5), (40, 12), (60, 8)] {
        render(&mut app, w, h);
        key(&mut app, 'N');
        render(&mut app, w, h);
        press(&mut app, KeyCode::Esc);
        key(&mut app, '?');
    }
}

/// Colours of one cell of a rendered screen.
fn cell_colors(app: &mut App, width: u16, height: u16, x: u16, y: u16) -> (ratatui::style::Color, ratatui::style::Color, String) {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            app.set_area(frame.area());
            crate::ui::draw(frame, app);
        })
        .unwrap();
    let cell = terminal.backend().buffer()[(x, y)].clone();
    (cell.fg, cell.bg, cell.symbol().to_string())
}

#[test]
fn a_space_has_a_background_and_art_that_preview_while_editing() {
    use ratatui::style::Color;
    let (mut app, _engine) = start();
    assert_eq!((app.current_space().background, app.current_space().art), (0, 0));
    let ink = Color::Rgb(23, 23, 28);
    let midnight = Color::Rgb(17, 22, 36);
    // (60, 20) is inside the page area, which is plain background while the editor shows.
    key(&mut app, 'e');
    assert_eq!(cell_colors(&mut app, 100, 30, 99, 28).1, ink);

    press(&mut app, KeyCode::Tab); // colour
    press(&mut app, KeyCode::Tab); // background
    assert_eq!(app.form.as_ref().unwrap().field, FormField::Background);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.theme().1, 2);
    assert_eq!(cell_colors(&mut app, 100, 30, 99, 28).1, midnight, "the tint previews before saving");
    let panel = cell_colors(&mut app, 100, 30, 5, 10).1;
    assert_eq!(panel, Color::Rgb(24, 29, 44), "the sidebar is the same tint, lifted");
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.theme().1, crate::art::BACKGROUNDS.len() - 1, "the choice wraps round");
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);

    press(&mut app, KeyCode::Tab); // art
    assert_eq!(app.form.as_ref().unwrap().field, FormField::Art);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.theme().2, crate::art::ART.len(), "left of none is the last piece");
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.theme().2, 1);
    let screen = render(&mut app, 100, 30);
    println!("{screen}");
    assert!(screen.contains("‹ mountains ›"));
    assert!(screen.contains("midnight"));
    // The piece is 24 pixels wide and 7 rows tall, at the bottom of the sidebar list.
    let art_rows: Vec<&str> = screen.lines().filter(|l| l.contains('▀') || l.contains('▄')).collect();
    assert_eq!(art_rows.len(), 7, "the art previews in the sidebar");

    // Escape discards everything.
    press(&mut app, KeyCode::Esc);
    assert_eq!((app.current_space().background, app.current_space().art), (0, 0));
    assert_eq!(app.theme(), (0, 0, 0));

    // Saving keeps it.
    key(&mut app, 'e');
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Enter);
    assert_eq!((app.current_space().background, app.current_space().art), (1, 3));
    assert_eq!(app.current_space().profile, "default", "theme changes leave the profile alone");
    assert!(app.tabs[0].tab_id.is_some(), "and reload nothing");
}

#[test]
fn art_gives_way_to_tabs_and_shows_on_a_blank_tab() {
    let (mut app, _engine) = start();
    key(&mut app, 'e');
    for _ in 0..3 {
        press(&mut app, KeyCode::Tab);
    }
    for _ in 0..4 {
        press(&mut app, KeyCode::Right); // plant
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.current_space().art, 4);
    let has_art = |screen: &str| screen.lines().any(|l| l.chars().take(27).any(|c| c == '▀' || c == '▄'));
    assert!(has_art(&render(&mut app, 100, 30)));
    // A short sidebar has no room: tabs come first.
    assert!(!has_art(&render(&mut app, 100, 14)));
    // A long tab list pushes the art out too.
    for i in 0..16 {
        open(&mut app, &format!("site{i}.example"));
    }
    command(&mut app, "toggle-mode");
    assert!(!has_art(&render(&mut app, 100, 30)));

    // A blank tab shows the piece above its text, in the page area.
    key(&mut app, 't');
    let screen = render(&mut app, 100, 30);
    let page_art = screen.lines().filter(|l| l.chars().skip(28).any(|c| c == '▀' || c == '▄')).count();
    assert_eq!(page_art, 7);
    assert!(screen.contains("New tab"));
}

#[test]
fn backgrounds_and_art_survive_a_restart_per_space() {
    let path = temp_session("themes");
    let _ = std::fs::remove_file(&path);
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(engine, Some(path.clone()));
    key(&mut app, 'N');
    type_text(&mut app, "Work");
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    for _ in 0..5 {
        press(&mut app, KeyCode::Right); // plum
    }
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Left); // the last piece
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "work.example");
    press(&mut app, KeyCode::Enter);
    app.quit();

    let saved = session::load(&path).unwrap();
    assert_eq!((saved.spaces[0].background, saved.spaces[0].art), (0, 0));
    assert_eq!((saved.spaces[1].background, saved.spaces[1].art), (5, crate::art::ART.len()));
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("\"background\"").count(), 1, "the plain look is not written out");

    let fresh = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(fresh, Some(path.clone()));
    assert_eq!(app.theme(), (app.current_space().color, 5, crate::art::ART.len()));
    key(&mut app, 'h');
    assert_eq!((app.theme().1, app.theme().2), (0, 0), "each space keeps its own look");

    // A session naming a tint or piece this version does not have falls back.
    let mut odd = saved.clone();
    odd.spaces[1].background = 99;
    odd.spaces[1].art = 99;
    session::save(&path, &odd).unwrap();
    let fresh = Rc::new(RefCell::new(MockEngine::new()));
    let app = start_with(fresh, Some(path.clone()));
    assert_eq!((app.current_space().background, app.current_space().art), (0, 0));
    let _ = std::fs::remove_file(&path);
}

// ----- history and downloads -----

/// Saturday 14 November 2026, 15:00 (tests read the clock as UTC).
const NOW: u64 = 1_794_668_400;

fn history_urls(app: &App) -> Vec<String> {
    (0..app.history.len()).map(|i| app.history.get(i).unwrap().url.clone()).collect()
}

fn last_request(engine: &Engine, method: &str) -> Option<serde_json::Value> {
    engine.borrow().log.iter().rev().find(|r| r["method"] == method).cloned()
}

#[test]
fn visits_are_recorded_with_their_space() {
    let (mut app, _engine) = start();
    app.now_override = Some(NOW - 7200);
    open(&mut app, "a.example");
    app.now_override = Some(NOW - 60);
    open(&mut app, "b.example");
    new_space(&mut app, "Work", "");
    app.now_override = Some(NOW);
    type_text(&mut app, "c.example/page");
    press(&mut app, KeyCode::Enter);

    let seen: Vec<(String, String)> = (0..app.history.len()).map(|i| app.history.get(i).unwrap()).map(|v| (v.url.clone(), v.space.clone())).collect();
    assert_eq!(
        seen[..3],
        [
            ("https://c.example/page".to_string(), "Work".to_string()),
            ("https://b.example".to_string(), "Home".to_string()),
            ("https://a.example".to_string(), "Home".to_string()),
        ]
    );
    assert_eq!(app.history.get(1).unwrap().title, "b.example", "the page's title is filled in");

    // Going back and forth between tabs is not visiting.
    let count = app.history.len();
    command(&mut app, "toggle-mode");
    key(&mut app, 'h');
    key(&mut app, 'j');
    key(&mut app, 'k');
    assert_eq!(app.history.len(), count);
}

#[test]
fn the_history_view_opens_pages_and_forgets_them() {
    let (mut app, engine) = start();
    app.now_override = Some(NOW - 2 * 86_400);
    open(&mut app, "old.example");
    app.now_override = Some(NOW - 5 * 3600);
    open(&mut app, "morning.example");
    new_space(&mut app, "Work", "");
    app.now_override = Some(NOW - 60);
    type_text(&mut app, "work.example");
    press(&mut app, KeyCode::Enter);
    app.now_override = Some(NOW);
    assert_eq!(app.mode, Mode::Page);

    command(&mut app, "history");
    assert_eq!(app.mode, Mode::Nav);
    assert!(!app.page_visible(), "the view takes the page's place");
    let screen = render(&mut app, 120, 20);
    println!("{screen}");
    for expected in ["History", "Recent", "Earlier today", "Thursday, 12 November", "work.example", "● Work", "● Home", "10:00",
                     "⏎ open in new tab", "s in its space", "x forget", "C clear all", "esc close"] {
        assert!(screen.contains(expected), "the history view shows {expected:?}");
    }
    let recent = screen.find("Recent").unwrap();
    assert!(recent < screen.find("Earlier today").unwrap() && screen.find("Earlier today").unwrap() < screen.find("Thursday").unwrap());

    // Forget the newest, then open the next in a new tab of the current space.
    let before = history_urls(&app);
    key(&mut app, 'x');
    assert_eq!(history_urls(&app), before[1..]);
    key(&mut app, 'j');
    let wanted = app.history.get(app.view_selected).unwrap().url.clone();
    let tabs = app.tabs.len();
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.view, None);
    assert_eq!(app.current_space().name, "Work");
    assert_eq!(app.tabs.len(), tabs + 1);
    assert_eq!(app.current().unwrap().url, wanted);
    assert_eq!(last_request(&engine, "tab.create").unwrap()["url"], wanted.as_str());
    assert_eq!(app.mode, Mode::Page, "the opened page takes the keyboard");

    // `s` opens a page in the space it was visited in.
    command(&mut app, "history");
    key(&mut app, 'G');
    assert_eq!(app.history.get(app.view_selected).unwrap().space, "Home");
    key(&mut app, 's');
    assert_eq!(app.current_space().name, "Home");
    assert_eq!(app.current().unwrap().url, "https://old.example");

    // Clearing asks first.
    key(&mut app, 'y');
    assert_eq!(app.view, Some(crate::views::View::History));
    key(&mut app, 'C');
    key(&mut app, 'n');
    assert!(!app.history.is_empty());
    key(&mut app, 'C');
    assert!(render(&mut app, 120, 20).contains("Clear all history?"));
    key(&mut app, 'y');
    assert!(app.history.is_empty());
    assert!(render(&mut app, 120, 20).contains("No pages visited yet."));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.view, None);
    assert!(app.page_visible());
}

#[test]
fn history_survives_a_restart_and_restoring_is_not_visiting() {
    let path = temp_session("history");
    let history_path = crate::history::path_beside(&path);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&history_path);
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(engine, Some(path.clone()));
    open(&mut app, "a.example");
    let before = history_urls(&app);
    assert!(before.contains(&"https://a.example".to_string()));
    app.quit();

    let mut saved = session::load(&path).unwrap();
    saved.instance = Some(999);
    session::save(&path, &saved).unwrap();
    let mut app = start_with(Rc::new(RefCell::new(MockEngine::new())), Some(path.clone()));
    assert_eq!(history_urls(&app), before);
    key(&mut app, 'k');
    key(&mut app, 'j');
    assert_eq!(history_urls(&app), before, "tabs loading again after a restart add nothing");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&history_path);
}

fn download_event(id: u64, name: &str, state: &str, received: u64, total: u64) -> String {
    format!(
        r#"{{"event":"download.updated","download_id":{id},"url":"https://files.example/{name}","filename":"{name}","path":"/tmp/{name}","state":"{state}","received":{received},"total":{total},"started":{}}}"#,
        NOW - 30
    )
}

#[test]
fn downloads_show_in_the_status_line_and_in_their_view() {
    let (mut app, engine) = three_tabs();
    app.now_override = Some(NOW);
    assert_eq!(app.download_notice(), None);
    app.handle_line(&download_event(1, "old.pdf", "done", 2_000_000, 2_000_000));
    app.handle_line(&download_event(2, "big.zip", "active", 0, 40_000_000));
    app.handle_line(&download_event(2, "big.zip", "active", 10_000_000, 40_000_000));
    assert_eq!(app.download_notice(), Some(("↓ big.zip 25%".to_string(), false)));
    let screen = render(&mut app, 120, 16);
    let status = screen.lines().last().unwrap();
    assert!(status.contains("↓ big.zip ━━━━━━━━━━━━ 25%"), "the status line reports the download: {status}");
    // The bar is a quarter full: three of its twelve cells are not the colour of the rest.
    let at = status.chars().position(|c| c == '━').unwrap() as u16;
    let bar = |app: &mut App, cell: u16| cell_colors(app, 120, 16, at + cell, 15).0;
    assert_eq!(bar(&mut app, 1), bar(&mut app, 2));
    assert_ne!(bar(&mut app, 2), bar(&mut app, 3), "the bar fills as the file arrives");
    assert_eq!(bar(&mut app, 3), bar(&mut app, 11));
    assert!(app.animating(), "a download in progress keeps the screen moving");

    key(&mut app, 'd');
    assert_eq!(app.view, Some(crate::views::View::Downloads));
    let screen = render(&mut app, 120, 16);
    println!("{screen}");
    for expected in ["Downloads", "2 files, 1 in progress", "big.zip", "25%", "10 MB of 40 MB", "old.pdf", "✓ 2.0 MB", "files.example", "14:59",
                     "⏎ open", "f show in Finder", "x stop or remove", "C clear list", "esc close"] {
        assert!(screen.contains(expected), "the downloads view shows {expected:?}");
    }
    assert!(screen.find("big.zip").unwrap() < screen.find("old.pdf").unwrap(), "newest first");

    // The running one is selected: x stops it, Enter and f act on the finished one.
    key(&mut app, 'x');
    assert_eq!(last_request(&engine, "download.cancel").unwrap()["download_id"], 2);
    app.handle_line(&download_event(2, "big.zip", "cancelled", 10_000_000, 40_000_000));
    key(&mut app, 'x');
    assert_eq!(last_request(&engine, "download.remove").unwrap()["download_id"], 2);
    app.handle_line(r#"{"event":"downloads.changed","downloads":[{"download_id":1,"filename":"old.pdf","state":"done","received":2000000,"total":2000000}]}"#);
    assert_eq!(app.downloads.len(), 1);
    assert_eq!(app.view_selected, 0);
    press(&mut app, KeyCode::Enter);
    assert_eq!(last_request(&engine, "download.open").unwrap()["download_id"], 1);
    key(&mut app, 'f');
    assert_eq!(last_request(&engine, "download.reveal").unwrap()["download_id"], 1);
    key(&mut app, 'C');
    assert!(last_request(&engine, "download.clear").is_some());
    app.handle_line(r#"{"event":"downloads.changed","downloads":[]}"#);
    assert!(render(&mut app, 120, 16).contains("Nothing has been downloaded."));

    // The list is the same in every space, and a finished download is announced until looked at.
    press(&mut app, KeyCode::Esc);
    new_space(&mut app, "Work", "");
    press(&mut app, KeyCode::Esc);
    app.handle_line(&download_event(3, "notes.txt", "active", 0, 0));
    app.handle_line(&download_event(3, "notes.txt", "done", 12, 12));
    assert_eq!(app.download_notice(), Some(("↓ notes.txt ✓".to_string(), false)));
    key(&mut app, 'd');
    assert_eq!(app.download_notice(), None);
    assert!(render(&mut app, 120, 16).contains("notes.txt"));
}

#[test]
fn a_failed_open_is_reported() {
    let (mut app, _engine) = three_tabs();
    app.handle_line(&download_event(1, "gone.pdf", "done", 5, 5));
    key(&mut app, 'd');
    // The mock always succeeds, so answer one request ourselves.
    let id = app.next_request;
    app.send(crate::protocol::Request::DownloadOpen { download_id: 99 }, crate::app::Pending::Notify);
    assert_eq!(app.message, None);
    app.pending.insert(id, crate::app::Pending::Notify);
    app.handle_line(&format!(r#"{{"id":{id},"ok":false,"error":"The file is no longer where it was saved."}}"#));
    assert_eq!(app.message.as_deref(), Some("The file is no longer where it was saved."));
    assert!(render(&mut app, 120, 16).lines().last().unwrap().contains("no longer where it was saved"));
}

#[test]
fn a_page_that_changes_its_own_address_is_one_visit() {
    let (mut app, _engine) = three_tabs();
    app.now_override = Some(NOW);
    let id = app.tabs[2].tab_id.unwrap();
    let before = app.history.len();
    let update = |app: &mut App, url: &str, entry: u64, entry_url: &str, title: &str, loading: bool| {
        app.handle_line(&format!(
            r#"{{"event":"tab.updated","tab_id":{id},"url":"{url}","title":"{title}","loading":{loading},"entry":{entry},"entry_url":"{entry_url}"}}"#
        ));
    };
    // The tab is restarted by a newer app that numbers its pages: the page it is on is not new.
    update(&mut app, "https://b.example", 1, "https://b.example", "b.example", false);
    assert_eq!(app.history.len(), before);

    // A search: the address is on its way, then arrives, then the page rewrites it twice.
    update(&mut app, "https://duckduckgo.com/?q=tilde", 1, "https://b.example", "b.example", true);
    assert_eq!(app.history.len(), before, "an address that has not arrived is not a visit");
    update(&mut app, "https://duckduckgo.com/?q=tilde", 2, "https://duckduckgo.com/?q=tilde", "", true);
    update(&mut app, "https://duckduckgo.com/?q=tilde&ia=web", 2, "https://duckduckgo.com/?q=tilde&ia=web", "tilde at DuckDuckGo", true);
    update(&mut app, "https://duckduckgo.com/?t=h_&q=tilde&ia=web", 2, "https://duckduckgo.com/?t=h_&q=tilde&ia=web", "tilde at DuckDuckGo", false);
    assert_eq!(app.history.len(), before + 1, "one search, one line");
    let top = app.history.get(0).unwrap();
    assert_eq!((top.url.as_str(), top.title.as_str()), ("https://duckduckgo.com/?t=h_&q=tilde&ia=web", "tilde at DuckDuckGo"));

    // Following a result is another page.
    update(&mut app, "https://tilde.example/", 3, "https://tilde.example/", "Tilde", false);
    assert_eq!(app.history.len(), before + 2);
}

#[test]
fn a_link_from_another_app_opens_in_a_new_tab() {
    let (mut app, engine) = three_tabs();
    key(&mut app, 'y'); // whatever is covering the page gives way
    app.handle_line(r#"{"event":"open-url","url":"https://clicked.example/x"}"#);
    assert_eq!(app.view, None);
    assert_eq!(app.tabs.len(), 4);
    assert_eq!(app.current().unwrap().url, "https://clicked.example/x");
    assert_eq!(last_request(&engine, "tab.create").unwrap()["url"], "https://clicked.example/x");
    assert_eq!(app.mode, Mode::Page);
    assert!(app.page_visible());
    assert_eq!(app.history.get(0).unwrap().url, "https://clicked.example/x");

    // While a new, still empty tab is open, the link takes its place.
    command(&mut app, "new-tab");
    assert_eq!(app.tabs.len(), 5);
    app.handle_line(r#"{"event":"open-url","url":"https://second.example/"}"#);
    assert_eq!(app.tabs.len(), 5);
    assert_eq!(app.current().unwrap().url, "https://second.example/");
}

#[test]
fn stepping_past_unloaded_tabs_does_not_load_them() {
    let path = temp_session("debounce");
    let _ = std::fs::remove_file(&path);
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(engine.clone(), Some(path.clone()));
    open(&mut app, "a.example");
    open(&mut app, "b.example");
    app.select(0);
    app.quit();

    // A new app process: only the selected tab (the first) is loaded.
    let mut saved = session::load(&path).unwrap();
    saved.instance = Some(999);
    session::save(&path, &saved).unwrap();
    let fresh = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = start_with(fresh.clone(), Some(path.clone()));
    assert_eq!(fresh.borrow().tab_ids().len(), 1);

    // Passing over the second tab on the way to the third loads only the third.
    let now = std::time::Instant::now();
    key(&mut app, 'j');
    assert!(!app.tabs[1].loading, "nothing is shown as loading yet");
    app.tick(now + crate::app::LOAD_DELAY / 2);
    assert_eq!(fresh.borrow().tab_ids().len(), 1, "the delay has not passed");
    key(&mut app, 'j');
    rest(&mut app);
    assert_eq!(app.tabs[1].tab_id, None, "the tab passed over stays unloaded");
    assert_eq!(fresh.borrow().active, app.tabs[2].tab_id);
    assert_eq!(fresh.borrow().tab_ids().len(), 2);

    // Stepping back to a loaded tab shows it at once; picking a tab directly loads it at once.
    key(&mut app, 'k');
    key(&mut app, 'k');
    assert_eq!(fresh.borrow().active, app.tabs[0].tab_id);
    app.select(1);
    assert_eq!(fresh.borrow().tab_ids().len(), 3);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_loading_tab_has_a_spinner_that_moves() {
    let (mut app, _engine) = three_tabs();
    assert!(!app.animating(), "nothing moves while nothing loads");
    app.tabs[app.selected].loading = true;
    app.tabs[app.selected].progress = 0.5;
    assert!(app.animating());
    let row = app.side_rows().iter().position(|r| *r == SideRow::Tab(app.selected)).unwrap() as u16 + 3;
    let first = cell_colors(&mut app, 90, 16, 1, row).2;
    app.frame += 1;
    let second = cell_colors(&mut app, 90, 16, 1, row).2;
    assert!(crate::ui::SPINNER.contains(&first.as_str()) && crate::ui::SPINNER.contains(&second.as_str()));
    assert_ne!(first, second, "each frame draws the next step of the spinner");
    // Half of the row carries the trace of the page arriving.
    assert_ne!(cell_colors(&mut app, 90, 16, 5, row).1, cell_colors(&mut app, 90, 16, 20, row).1);
}

/// `TILDE_UI_DUMP=dir cargo test dump_ui` writes the interface on every
/// background as HTML, for looking at when designing.
#[test]
fn dump_ui() {
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};
    use ratatui::Terminal;
    let Some(dir) = std::env::var_os("TILDE_UI_DUMP") else { return };
    let (mut app, _engine) = three_tabs();
    key(&mut app, 'p');
    app.now_override = Some(NOW);
    app.handle_line(&download_event(2, "big.zip", "active", 14_000_000, 40_000_000));
    app.tabs[1].loading = true;
    app.tabs[1].progress = 0.6;
    app.frame = 3;
    let css = |c: Color| match c {
        Color::Rgb(r, g, b) => format!("rgb({r},{g},{b})"),
        Color::Black => "#000".to_string(),
        _ => "inherit".to_string(),
    };
    let mut html = String::from("<meta charset=utf-8><body style='background:#888;margin:8px'>");
    for background in 0..crate::art::BACKGROUNDS.len() {
        app.spaces[app.space].background = background;
        app.spaces[app.space].color = background % crate::spaces::COLORS.len();
        app.spaces[app.space].art = 1 + background % crate::art::ART.len();
        let (width, height) = (96, 20);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                app.set_area(frame.area());
                crate::ui::draw(frame, &app);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        html.push_str("<pre style='font:13px/1.15 Menlo,monospace;margin:0 0 10px;display:inline-block'>");
        for y in 0..height {
            for x in 0..width {
                let cell = &buffer[(x, y)];
                let weight = if cell.modifier.contains(Modifier::BOLD) { "bold" } else { "normal" };
                let symbol = cell.symbol().replace('&', "&amp;").replace('<', "&lt;");
                html.push_str(&format!("<span style='color:{};background:{};font-weight:{weight}'>{symbol}</span>", css(cell.fg), css(cell.bg)));
            }
            html.push('\n');
        }
        html.push_str("</pre><br>");
    }
    std::fs::write(std::path::Path::new(&dir).join("ui.html"), html).unwrap();
}

#[test]
fn the_address_bar_completes_from_history() {
    let (mut app, engine) = start();
    app.history.visit("https://github.com/rust-lang/rust", "Rust", "Home", 1_000);
    app.history.visit("http://plain.example/", "Plain", "Home", 2_000);
    let rest = |app: &App| app.edit.as_ref().unwrap().completion.as_ref().map(|c| c.rest.clone());
    key(&mut app, 't');
    type_text(&mut app, "gi");
    assert_eq!(rest(&app).as_deref(), Some("thub.com"));
    assert!(render(&mut app, 80, 12).contains("▐github.com"), "the offer is shown after the typed text");
    // Deleting refuses the offer without eating a typed character.
    press(&mut app, KeyCode::Backspace);
    assert_eq!((app.edit.as_ref().unwrap().text.as_str(), rest(&app)), ("gi", None));
    press(&mut app, KeyCode::Backspace);
    assert_eq!((app.edit.as_ref().unwrap().text.as_str(), rest(&app)), ("g", None));
    // → takes it, and typing on completes pages of the site.
    key(&mut app, 'i');
    press(&mut app, KeyCode::Right);
    assert_eq!((app.edit.as_ref().unwrap().text.as_str(), rest(&app)), ("github.com", None));
    key(&mut app, '/');
    assert_eq!(rest(&app).as_deref(), Some("rust-lang/rust"));
    // Enter opens the offered page.
    press(&mut app, KeyCode::Enter);
    let sent = engine.borrow().log.iter().rev().find(|r| r["method"] == "tab.create").cloned().unwrap();
    assert_eq!(sent["input"], "https://github.com/rust-lang/rust");
    // A site only ever seen over http is spelled out when taken.
    key(&mut app, 't');
    type_text(&mut app, "pla");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.edit.as_ref().unwrap().text, "http://plain.example/");
    // Pasting and searching are left alone.
    command(&mut app, "clear-line");
    app.handle_paste("gi");
    assert_eq!(rest(&app), None);
    command(&mut app, "clear-line");
    type_text(&mut app, "gi t");
    assert_eq!(rest(&app), None);
}

#[test]
fn a_local_file_without_a_title_goes_by_its_name() {
    let tab = crate::app::TabEntry { url: "file:///Users/me/Documents/report.pdf".into(), ..Default::default() };
    assert_eq!(tab.label(), "report.pdf");
    let tab = crate::app::TabEntry { url: "https://www.example.com/a.pdf".into(), ..Default::default() };
    assert_eq!(tab.label(), "example.com");
}

#[test]
fn find_in_page() {
    let (mut app, engine) = start();
    open(&mut app, "a.example");
    let sent = |method: &str| engine.borrow().log.iter().rev().find(|r| r["method"] == method).cloned().unwrap();
    let result = |app: &App| app.find.as_ref().unwrap().result.map(|r| (r.index, r.count));
    command(&mut app, "find");
    assert!(app.find_has_keys(), "the bar takes the keys");
    assert!(render(&mut app, 100, 12).contains(" FIND  ▐Find in page"));
    type_text(&mut app, "fox");
    assert_eq!((sent("find.set")["text"].as_str(), sent("find.set")["fresh"].as_bool()), (Some("fox"), Some(true)));
    assert_eq!(result(&app), Some((1, 3)));
    assert!(render(&mut app, 100, 12).contains("▐fox"));
    assert!(render(&mut app, 100, 12).contains("1 of 3"));
    // Enter and ↓ go on, ↑ goes back, and both wrap around.
    press(&mut app, KeyCode::Enter);
    assert_eq!((result(&app), sent("find.set")["fresh"].as_bool()), (Some((2, 3)), Some(false)));
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Up);
    assert_eq!((result(&app), sent("find.set")["backwards"].as_bool()), (Some((3, 3)), Some(true)));
    command(&mut app, "find-next");
    assert_eq!(result(&app), Some((1, 3)));
    // Nothing found is said, and an emptied field takes the marks off.
    type_text(&mut app, "zzz");
    assert!(render(&mut app, 100, 12).contains("No matches"));
    command(&mut app, "clear-line");
    assert_eq!((app.find.as_ref().unwrap().edit.text.as_str(), result(&app)), ("", None));
    assert_eq!(engine.borrow().log.last().unwrap()["method"], "find.close");
    // Esc closes; the text is remembered for the next time and for Cmd+G.
    type_text(&mut app, "fox");
    press(&mut app, KeyCode::Esc);
    assert!(app.find.is_none());
    assert_eq!(sent("find.close")["tab_id"], sent("find.set")["tab_id"]);
    command(&mut app, "find-next");
    assert_eq!(app.find.as_ref().unwrap().edit.text, "fox");
    command(&mut app, "find");
    assert!(app.find.as_ref().unwrap().edit.replace_on_type, "the old text is typed over");
    // Cmd+E searches for what the page has selected.
    app.handle_line(r#"{"event":"command","name":"find","text":"jumps"}"#);
    assert_eq!((app.find.as_ref().unwrap().edit.text.as_str(), sent("find.set")["text"].as_str()), ("jumps", Some("jumps")));
    // The bar belongs to its tab: going to another closes it.
    command(&mut app, "new-tab");
    assert!(app.find.is_none());
}

#[test]
fn a_tab_using_the_camera_or_microphone_is_marked() {
    let (mut app, _engine) = start();
    open(&mut app, "meet.example");
    let tab_id = app.current().unwrap().tab_id.unwrap();
    assert!(!render(&mut app, 100, 12).contains("● mic"));
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{tab_id},"microphone":true}}"#));
    assert!(render(&mut app, 100, 12).contains("● mic"));
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{tab_id},"camera":true}}"#));
    assert!(render(&mut app, 100, 12).contains("● cam mic"));
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{tab_id},"camera":false,"microphone":false}}"#));
    assert!(!render(&mut app, 100, 12).contains("● cam") && !render(&mut app, 100, 12).contains("● mic"));
}
