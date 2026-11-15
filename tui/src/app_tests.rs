//! Behaviour tests: the real `App` driven against the mock engine.

use crate::app::{App, HOME_URL};
use crate::mock::MockEngine;
use crate::protocol::Mode;
use crate::session::{self, Session, SessionTab};
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
    std::env::temp_dir().join(format!("weft-app-test-{}-{name}.json", std::process::id()))
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
    let (mut app, _engine) = start();
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
    app.handle_paste("pasted\ntext");
    assert_eq!(app.edit.as_ref().unwrap().text, "xpastedtextabc ");
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
    click(&mut app, 5, 2);
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
    assert_eq!(saved.active, 1);
    assert_eq!(saved.tabs.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(), vec![HOME_URL, "https://a.example", "https://b.example"]);

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
            tabs: vec![
                SessionTab { url: "https://b.example".into(), title: "B".into(), tab_id: Some(b) },
                SessionTab { url: "https://gone.example".into(), title: "Gone".into(), tab_id: Some(77) },
                SessionTab { url: "https://a.example".into(), title: "A".into(), tab_id: Some(a) },
            ],
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
