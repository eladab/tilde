//! The dev tools pane, driven against the mock engine and drawn to a buffer.

use crate::app::App;
use crate::devtools::{Dock, NetFilter, NetTab, StorageKind, Tool};
use crate::mock::MockEngine;
use crate::protocol::Mode;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::Terminal;
use std::cell::RefCell;
use std::rc::Rc;

type Engine = Rc<RefCell<MockEngine>>;

fn start_sized(cols: u16, rows: u16) -> (App, Engine) {
    let engine = Rc::new(RefCell::new(MockEngine::new()));
    let mut app = App::new(Box::new(engine.clone()), None);
    app.now_override = Some(1_800_000_000);
    app.start();
    app.set_area(Rect::new(0, 0, cols, rows));
    (app, engine)
}

fn start() -> (App, Engine) {
    start_sized(120, 40)
}

fn key(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    let area = Rect::new(0, 0, app.layout.status.width, app.layout.status.y + 1);
    app.set_area(area);
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    let area = Rect::new(0, 0, app.layout.status.width, app.layout.status.y + 1);
    app.set_area(area);
}

fn keys(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, c);
    }
}

fn command(app: &mut App, name: &str) {
    app.handle_line(&format!(r#"{{"event":"command","name":"{name}"}}"#));
    let area = Rect::new(0, 0, app.layout.status.width, app.layout.status.y + 1);
    app.set_area(area);
}

fn click(app: &mut App, col: u16, row: u16) {
    app.handle_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: col, row, modifiers: KeyModifiers::NONE });
    let area = Rect::new(0, 0, app.layout.status.width, app.layout.status.y + 1);
    app.set_area(area);
}

/// What is on screen, one string per line.
fn screen(app: &mut App) -> Vec<String> {
    let (cols, rows) = (app.layout.status.width, app.layout.status.y + 1);
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    terminal
        .draw(|frame| {
            app.set_area(frame.area());
            crate::ui::draw(frame, app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..rows).map(|y| (0..cols).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string()).collect()
}

fn shows(app: &mut App, text: &str) -> bool {
    screen(app).iter().any(|line| line.contains(text))
}

fn last_request(engine: &Engine, method: &str) -> Option<serde_json::Value> {
    engine.borrow().log.iter().rev().find(|r| r["method"] == method).cloned()
}

fn viewport(engine: &Engine) -> (u64, u64, u64, u64) {
    engine.borrow().viewport.expect("viewport set")
}

#[test]
fn opening_makes_room_beside_the_page_and_watches_its_tab() {
    let (mut app, engine) = start();
    assert_eq!(viewport(&engine), (28, 1, 92, 38));
    key(&mut app, 'I');
    assert!(app.dev.open && app.dev_has_keys());
    assert_eq!(viewport(&engine), (28, 1, 51, 38), "the page keeps the columns the pane does not take");
    assert!(engine.borrow().visible, "the page stays on screen");
    assert_eq!(engine.borrow().net_watch, Some(app.tabs[0].tab_id));
    assert_eq!(app.dev.net.requests.len(), 14);
    assert!(shows(&mut app, " DEV "));
    assert!(shows(&mut app, "14 requests"));
    assert!(shows(&mut app, "report.csv"));

    key(&mut app, 'q');
    assert!(!app.dev.open);
    assert_eq!(viewport(&engine), (28, 1, 92, 38));
    assert_eq!(engine.borrow().net_watch, None, "capturing stops with the pane");
    assert!(shows(&mut app, " NAV "));
}

#[test]
fn the_pane_can_sit_below_the_page_and_change_size() {
    let (mut app, engine) = start();
    keys(&mut app, "Im");
    assert_eq!(app.dev.dock, Dock::Bottom);
    assert_eq!(viewport(&engine), (28, 1, 92, 21));
    key(&mut app, '+');
    assert_eq!(viewport(&engine), (28, 1, 92, 19));
    keys(&mut app, "--");
    assert_eq!(viewport(&engine), (28, 1, 92, 23));
    let session = app.session();
    assert!(session.devtools_below);
    assert_eq!(session.devtools_size, 40);
}

#[test]
fn a_request_opens_into_headers_payload_and_response() {
    let (mut app, engine) = start_sized(150, 46);
    key(&mut app, 'I');
    assert_eq!(app.dev.net.selected, Some(1));
    assert!(last_request(&engine, "net.get").is_none(), "a narrow pane asks for details only when they are opened");
    keys(&mut app, "jjjjjjj");
    assert_eq!(app.dev.net.selected, Some(7));
    press(&mut app, KeyCode::Enter);
    assert!(app.dev.net.detail_open);
    assert_eq!(last_request(&engine, "net.get").unwrap()["request_id"], 7);
    assert!(shows(&mut app, "General"));
    assert!(shows(&mut app, "X-CSRF-Token  b1946ac92492d2347c6235b4d2611184"));
    assert!(!shows(&mut app, "report.csv"), "the details take the list's place");

    key(&mut app, 'l');
    assert_eq!(app.dev.net.tab, NetTab::Payload);
    assert!(shows(&mut app, "Query parameters (3)"));
    assert!(shows(&mut app, "q     terminal browser"));
    assert!(shows(&mut app, "\"query\": \"terminal browser\","), "the body is JSON, re-indented, in its own order");
    key(&mut app, 'l');
    assert!(shows(&mut app, "\"took_ms\": 41,"));

    // J and K move to other requests without leaving the details.
    key(&mut app, 'J');
    assert_eq!(app.dev.net.selected, Some(8));
    key(&mut app, 'h');
    assert!(shows(&mut app, "Form data (3)"));
    assert!(shows(&mut app, "user      ada@example.com"));

    press(&mut app, KeyCode::Esc);
    assert!(!app.dev.net.detail_open && app.dev_has_keys());
    assert!(shows(&mut app, "report.csv"));
}

#[test]
fn a_wide_pane_shows_the_list_and_the_details_together() {
    let (mut app, engine) = start_sized(150, 46);
    keys(&mut app, "Im");
    assert!(app.dev_wide());
    assert_eq!(last_request(&engine, "net.get").unwrap()["request_id"], 1);
    assert!(shows(&mut app, "report.csv") || shows(&mut app, "hero.webp"));
    assert!(shows(&mut app, "max-age=31536000; includeSubDomains"));
    key(&mut app, 'j');
    assert_eq!(last_request(&engine, "net.get").unwrap()["request_id"], 2);
    assert!(shows(&mut app, "WebKit reports only the timing and size"));
}

#[test]
fn requests_are_filtered_by_kind_and_by_address() {
    let (mut app, _) = start();
    keys(&mut app, "If");
    assert_eq!(app.dev.net.filter, NetFilter::Fetch);
    assert_eq!(app.dev.net.visible().len(), 7);
    assert!(shows(&mut app, "7 of 14 requests"));
    assert_eq!(app.dev.net.selected, Some(6), "the selection moves to a request that is listed");
    keys(&mut app, "/export");
    assert!(app.dev.search.is_some());
    assert_eq!(app.dev.net.visible().len(), 1);
    press(&mut app, KeyCode::Enter);
    assert!(app.dev.search.is_none());
    assert_eq!(app.dev.net.selected, Some(11));
    assert!(shows(&mut app, "/export"));
    // Escape undoes the search, then the filter, then gives the keys back.
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.dev.net.visible().len(), 7);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.dev.net.visible().len(), 14);
    assert!(app.dev_has_keys());
    press(&mut app, KeyCode::Esc);
    assert!(app.dev.open && !app.dev_has_keys());
    assert!(shows(&mut app, " NAV "));
}

#[test]
fn updates_change_and_add_requests_in_the_order_they_began() {
    let (mut app, _) = start();
    key(&mut app, 'I');
    let tab = app.tabs[0].tab_id.unwrap();
    app.handle_line(&format!(
        r#"{{"event":"net.updated","tab_id":{tab},"requests":[{{"id":13,"method":"GET","url":"https://webkit.org/api/feed","status":200,"kind":"fetch","state":"done","started":1800000002100.0,"duration":900.0}},{{"id":20,"method":"GET","url":"https://webkit.org/early.js","kind":"js","state":"done","started":1800000000100.0}}]}}"#
    ));
    assert_eq!(app.dev.net.requests.len(), 15);
    assert_eq!(app.dev.net.requests[1].id, 20, "a request reported late is listed where it began");
    assert_eq!(app.dev.net.requests.last().unwrap().status, 200);
    // Another tab's requests are not this list's.
    app.handle_line(r#"{"event":"net.updated","tab_id":999,"reset":true,"requests":[]}"#);
    assert_eq!(app.dev.net.requests.len(), 15);
    // The tab went to another page.
    app.handle_line(&format!(r#"{{"event":"net.updated","tab_id":{tab},"reset":true,"requests":[{{"id":30,"method":"GET","url":"https://a.example/","kind":"doc","state":"pending"}}]}}"#));
    assert_eq!(app.dev.net.requests.len(), 1);
    assert_eq!(app.dev.net.selected, Some(30));

    key(&mut app, 'x');
    assert!(app.dev.net.requests.is_empty());
    assert!(shows(&mut app, "No requests yet."));
}

#[test]
fn the_pane_follows_the_tab_that_is_showing() {
    let (mut app, engine) = start();
    key(&mut app, 'I');
    press(&mut app, KeyCode::Esc);
    key(&mut app, 't');
    assert_eq!(engine.borrow().net_watch, Some(None), "a blank tab has nothing to watch, but capturing goes on");
    assert!(shows(&mut app, "This tab has no page yet."));
    keys(&mut app, "a.example");
    press(&mut app, KeyCode::Enter);
    assert_eq!(engine.borrow().net_watch, Some(app.tabs[1].tab_id));
    assert!(app.dev.net.requests[0].url.contains("a.example"));
    command(&mut app, "prev-tab");
    assert_eq!(engine.borrow().net_watch, Some(app.tabs[0].tab_id));
    assert!(app.dev.net.requests[0].url.contains("webkit.org"));
}

#[test]
fn the_menu_command_opens_focuses_and_closes() {
    let (mut app, _) = start();
    command(&mut app, "devtools");
    assert!(app.dev_has_keys());
    // From the page it takes the keys back instead of closing.
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Page);
    command(&mut app, "devtools");
    assert_eq!(app.mode, Mode::Nav);
    assert!(app.dev.open && app.dev_has_keys());
    command(&mut app, "devtools");
    assert!(!app.dev.open);
}

#[test]
fn copying_a_request() {
    let (mut app, engine) = start();
    keys(&mut app, "Ijjjjjjjc");
    assert_eq!(last_request(&engine, "clipboard.set").unwrap()["text"], "https://webkit.org/api/search?q=terminal+browser&page=1&lang=en");
    press(&mut app, KeyCode::Enter);
    key(&mut app, 'C');
    let curl = last_request(&engine, "clipboard.set").unwrap()["text"].as_str().unwrap().to_string();
    assert!(curl.starts_with("curl 'https://webkit.org/api/search?q=terminal+browser&page=1&lang=en' -X POST"), "{curl}");
    assert!(curl.contains("-H 'Content-Type: application/json'"));
    assert!(curl.contains(r#"--data-raw '{"query":"terminal browser""#));
    keys(&mut app, "llc");
    assert!(last_request(&engine, "clipboard.set").unwrap()["text"].as_str().unwrap().starts_with(r#"{"total":2"#), "the body is copied as it was sent");
}

#[test]
fn storage_lists_what_the_page_site_stores() {
    let (mut app, engine) = start_sized(150, 46);
    key(&mut app, 'I');
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.dev.tool, Tool::Storage);
    assert_eq!(last_request(&engine, "storage.get").unwrap()["tab_id"], app.tabs[0].tab_id.unwrap());
    assert_eq!(app.dev.storage.site_name(), "webkit.org");
    let names: Vec<String> = app.dev.storage.items(app.now()).into_iter().map(|i| i.name).collect();
    assert_eq!(names, vec!["prefs", "session", "theme"], "cookies of the site and its subdomains, by name");
    assert!(shows(&mut app, "Site webkit.org"));
    assert!(shows(&mut app, "the page in this tab"));

    press(&mut app, KeyCode::Enter);
    assert!(shows(&mut app, "Domain    .webkit.org"));
    assert!(shows(&mut app, "Decoded"));
    assert!(shows(&mut app, "\"lang\": \"en\","));
    press(&mut app, KeyCode::Esc);

    key(&mut app, 'l');
    assert_eq!(app.dev.storage.kind, StorageKind::Local);
    assert!(shows(&mut app, "onboarding.done"));
    keys(&mut app, "ll");
    assert!(shows(&mut app, "offline › articles"));
    assert!(shows(&mut app, "128 records"));
    press(&mut app, KeyCode::Enter);
    assert!(shows(&mut app, "The first 2 of 128 records."));
}

#[test]
fn storage_items_can_be_deleted() {
    let (mut app, engine) = start();
    key(&mut app, 'I');
    keys(&mut app, "2ljx");
    let deleted = last_request(&engine, "storage.delete").unwrap();
    assert_eq!((deleted["kind"].as_str(), deleted["name"].as_str()), (Some("local"), Some("onboarding.done")));
    assert_eq!(app.dev.storage.items(app.now()).len(), 2, "the list is read again after a deletion");
    assert_eq!(app.message.as_deref(), Some("Deleted onboarding.done."));

    // Everything of a kind is only deleted after a yes.
    key(&mut app, 'X');
    assert_eq!(app.dev_question().as_deref(), Some("Delete 2 local storage keys of webkit.org?"));
    assert!(shows(&mut app, "y deletes"));
    key(&mut app, 'n');
    assert_eq!(app.dev.storage.items(app.now()).len(), 2);
    keys(&mut app, "Xy");
    assert_eq!(last_request(&engine, "storage.delete").unwrap()["all"], true);
    assert!(app.dev.storage.items(app.now()).is_empty());
    assert!(shows(&mut app, "No local storage."));
}

#[test]
fn storage_of_another_site_is_its_cookies() {
    let (mut app, engine) = start_sized(150, 46);
    keys(&mut app, "I2s");
    assert!(app.dev.storage.picking);
    assert!(shows(&mut app, "3 with data in profile default"));
    assert!(shows(&mut app, "this page"));
    key(&mut app, 'j');
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.dev.storage.site_name(), "example.com");
    let items = app.dev.storage.items(app.now());
    assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), vec!["_ga", "sid"]);
    assert_eq!(items[1].when, "Expired");
    assert!(shows(&mut app, "not the page in this tab"));
    key(&mut app, 'l');
    assert!(shows(&mut app, "It stores: Local Storage, IndexedDB."));

    // The site list can delete everything a site has.
    // It opens on the site that is showing.
    keys(&mut app, "sX");
    assert_eq!(app.dev_question().as_deref(), Some("Delete everything example.com stores, cookies included?"));
    key(&mut app, 'y');
    let deleted = last_request(&engine, "storage.delete").unwrap();
    assert_eq!((deleted["kind"].as_str(), deleted["site"].as_str()), (Some("site"), Some("example.com")));
    assert!(app.dev.storage.sites.iter().find(|s| s.name == "example.com").is_some_and(|s| s.cookies == 0));
}

#[test]
fn storage_is_read_again_when_the_page_has_loaded() {
    let (mut app, engine) = start();
    keys(&mut app, "I2");
    let count = |engine: &Engine| engine.borrow().log.iter().filter(|r| r["method"] == "storage.get").count();
    assert_eq!(count(&engine), 1);
    let tab = app.tabs[0].tab_id.unwrap();
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{tab},"loading":true}}"#));
    assert_eq!(count(&engine), 1);
    app.handle_line(&format!(r#"{{"event":"tab.updated","tab_id":{tab},"loading":false}}"#));
    assert_eq!(count(&engine), 2);
}

#[test]
fn the_mouse_works_in_the_pane() {
    let (mut app, _) = start();
    key(&mut app, 'I');
    press(&mut app, KeyCode::Esc);
    assert!(!app.dev_has_keys());
    let list = app.dev_list();
    click(&mut app, list.x + 5, list.y + 2);
    assert!(app.dev_has_keys(), "a click in the pane gives it the keys");
    assert_eq!(app.dev.net.selected, Some(3));
    click(&mut app, list.x + 5, list.y + 2);
    assert!(app.dev.net.detail_open, "a second click opens the request");
    let (_, storage) = app.dev_tool_tabs()[1];
    click(&mut app, storage.x + 2, storage.y);
    assert_eq!(app.dev.tool, Tool::Storage);
    // A click on a tab in the sidebar gives the keys back to the tabs.
    let row = app.layout.tab_list.y + 1;
    click(&mut app, 5, row);
    assert!(app.dev.open && !app.dev_has_keys());
}

#[test]
fn other_keys_still_reach_the_tabs_once_the_pane_lets_go() {
    let (mut app, _) = start();
    key(&mut app, 't');
    keys(&mut app, "a.example");
    press(&mut app, KeyCode::Enter);
    app.handle_line(r#"{"event":"focus.changed","mode":"nav"}"#);
    key(&mut app, 'I');
    key(&mut app, 'k');
    assert_eq!(app.selected, 1, "j and k move in the pane while it has the keys");
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'k');
    assert_eq!(app.selected, 0);
    key(&mut app, 'I');
    assert!(app.dev_has_keys(), "I gives the keys back to the open pane");
}

#[test]
fn a_window_too_small_for_the_pane_keeps_the_page() {
    let (mut app, engine) = start_sized(60, 12);
    key(&mut app, 'I');
    assert_eq!(app.layout.devtools, Rect::default());
    assert_eq!(viewport(&engine), (28, 1, 32, 10));
    assert!(!app.dev_has_keys(), "a pane that is not on screen takes no keys");
}

#[test]
fn the_details_keep_their_place_while_a_request_goes_on() {
    let (mut app, engine) = start_sized(150, 20);
    key(&mut app, 'I');
    press(&mut app, KeyCode::Enter);
    keys(&mut app, "jjj");
    assert_eq!(app.dev.detail_scroll, 3);
    let asked = |engine: &Engine| engine.borrow().log.iter().filter(|r| r["method"] == "net.get").count();
    let before = asked(&engine);
    let tab = app.tabs[0].tab_id.unwrap();
    app.handle_line(&format!(r#"{{"event":"net.updated","tab_id":{tab},"requests":[{{"id":1,"method":"GET","url":"https://webkit.org","status":200,"kind":"doc","state":"done","started":1800000000000.0,"duration":300.0}}]}}"#));
    assert_eq!(asked(&engine), before + 1, "a request that changed is read again");
    press(&mut app, KeyCode::Null);
    assert_eq!(app.dev.detail_scroll, 3);
    // Another request starts at its top.
    key(&mut app, 'J');
    assert_eq!(app.dev.detail_scroll, 0);
}

#[test]
fn a_page_loaded_before_the_pane_opened_says_so_until_it_is_reloaded() {
    let (mut app, engine) = start_sized(150, 46);
    key(&mut app, 'I');
    assert!(app.dev.net.late);
    assert!(shows(&mut app, "Loaded before dev tools opened."));
    let first = app.dev_list().y;
    key(&mut app, 'r');
    assert!(last_request(&engine, "tab.reload").is_some());
    assert!(!app.dev.net.late, "a page loaded with the pane open is recorded from its start");
    assert!(!shows(&mut app, "Loaded before dev tools opened."));
    assert_eq!(app.dev_list().y, first - 1, "the list takes the notice's line back");
    // A tab opened while the pane is open never needs the notice.
    press(&mut app, KeyCode::Esc);
    key(&mut app, 't');
    keys(&mut app, "a.example");
    press(&mut app, KeyCode::Enter);
    assert!(!app.dev.net.late);
}
