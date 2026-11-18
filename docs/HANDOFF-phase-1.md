# Phase 1 handoff: foundation

Read [../README.md](../README.md) first for the concept, the app/TUI split and
the two modes. This document is everything needed to build phase 1.

## Goal

A browser you can actually open and click around in: launch Tilde, see a
vertical tab list and URL bar drawn by the TUI, and a native WebKit page sitting
exactly in the main pane. Open, switch and close tabs, navigate, and move
keyboard focus between the TUI and the page.

Phase 1 is done when every item under [Acceptance](#acceptance) passes.

## Status

Built. `scripts/test.sh` runs 77 unit tests (14 Swift, 63 Rust) and
`scripts/e2e.py` runs 42 end-to-end checks; all pass. The end-to-end run uses a
private copy of `build/Tilde.app` with its own bundle identifier, so it has its
own WebKit data and can run while the real Tilde is open.

Those counts include features added after phase 1, described in the README:
sidebar toggle, pinned tabs, renaming, key reference, and spaces. They added
`command` names (`toggle-sidebar`, `toggle-pin`, `pin-home`, `rename-tab`,
`help`, `next-space`, `prev-space`, `select-space`, `new-space`, `edit-space`,
`delete-space`) and one protocol field: `tab.create` takes an optional
`profile` name. The app maps a profile name to a persistent
`WKWebsiteDataStore` by hashing it into the store's UUID; an absent name or
`default` is the default store. Spaces themselves exist only in the TUI.

The end-to-end checks drive the app with synthesised keys and clicks, so the
following acceptance points are **not** verified and need a person:

| Item | Not verified |
|---|---|
| 2 | Sound (the video test is muted), IME and dead keys, a real YouTube page |
| 8 | A live window drag, and moving to a display with another scale factor |
| all | Real keyboard and mouse: every input in the tests is synthesised |

Also not done: libghostty was not evaluated (see
[spike-notes.md](spike-notes.md)), and the `justfile` has not been run because
`just` is not installed here; its recipes only call `scripts/`.

Changes to the protocol made during the build are marked "added" below.

## Decisions already made

| Decision | Choice | Reason |
|---|---|---|
| Shape | One native app window holding an embedded terminal view plus a `WKWebView` subview | Exact alignment, no window tracking, no permissions |
| TUI | Rust, `ratatui` + `crossterm`, binary `tilde-tui` | Mature layout/widgets; phase 2–3 UI (palette, folders) will need them |
| App | Swift 6, AppKit + WebKit, SwiftPM, assembled into `Tilde.app` by a script | A real bundle from day one (Info.plist, menu bar, later entitlements) without an Xcode project |
| Terminal view | SwiftTerm by default, behind a small `TerminalHost` protocol; libghostty evaluated in M0 | SwiftTerm is a known-working AppKit view; libghostty renders better but its embedding API may not be stable |
| IPC | Newline-delimited JSON over a Unix domain socket | Debuggable with `nc -U`; no codegen |
| Lifecycle | The app is the parent: it creates the socket and runs `tilde-tui` in the embedded terminal's pty | The app must exist before there is a terminal to draw in |
| State ownership | The app is the source of truth for web state (URL, title, loading). The TUI owns organisation (order, selection) and the session file | Keeps later spaces/folders out of the app |
| Mode names | `nav` (TUI focused) and `page` (web view focused) | |

Not verified yet, and deliberately front-loaded into M0: the terminal-view
choice and the cell-to-point mapping are designed from API knowledge, not from a
working prototype. If M0 turns up a blocker, stop and report rather than
building on it.

## Out of scope for phase 1

History, autocomplete, command palette, find in page, downloads, pinned tabs,
spaces, favicons, split view, profiles, multiple windows, content blocking,
extensions, JS `alert`/`confirm` UI, HTTP auth prompts, file upload panels,
TUI popups overlapping the page, config file, themes. Do not build hooks for
these beyond what the protocol below already allows.

## Architecture

```
 Tilde.app window
 ┌─────────────────────────────────────────┐
 │ TerminalHost view (fills the window)    │
 │  ┌ pty ─────────────────────────────┐   │
 │  │ tilde-tui (Rust)                  │   │      JSON lines over
 │  │  sidebar │ viewport rect (blank) │◄──┼────  $TILDE_SOCKET
 │  └──────────┼───────────────────────┘   │          │
 │             ▼                           │          ▼
 │   WKWebView subview, framed to the      │   Engine: TabStore ·
 │   viewport rect in points               │   ViewportLayout · Commands
 └─────────────────────────────────────────┘
```

### App components

- **TerminalHost**: protocol wrapping the terminal view. Needs: start a process
  in a pty with a given environment, report grid size in cells, report cell
  size in points and the grid's origin within the view, become first responder,
  notify on resize and on child exit. Requirements on the implementation:
  truecolor, SGR mouse reporting, bracketed paste, synchronized output.
- **TabStore**: `[TabID: WKWebView]`, one shared `WKWebViewConfiguration`
  (default persistent data store). KVO on `title`, `url`, `isLoading`,
  `estimatedProgress`, `canGoBack`, `canGoForward`, coalesced into `tab.updated`
  events (at most one per tab per ~50 ms).
- **ViewportLayout**: converts the TUI's cell rectangle to a frame in the window
  and applies it to the active web view, which is a sibling subview above the
  terminal view. Recomputes on window resize, font change and backing-scale
  change. Inactive web views are removed from the hierarchy but kept alive.
- **Commands**: the main menu. Every browser shortcut is a menu item whose
  action sends a `command` event to the TUI. Includes a standard Edit menu so
  copy/paste/undo work in the page.
- **SocketServer**: one client at a time; a new connection replaces the old one.

`WKUIDelegate.createWebViewWith` (target=`_blank`, `window.open`) creates a new
tab and emits `tab.created`. JS dialogs: auto-dismiss and log; real UI is phase 2.

### Window

Standard titled window, title bar visible, title set to the active tab's title.
Resizing snaps to whole cells (`contentResizeIncrements`) so there is no partial
row or column. Minimum size 80×24 cells. Custom title bar styling is later work.

### Focus

| Mode | First responder | Who handles plain keys |
|---|---|---|
| `nav` | terminal view | TUI |
| `page` | active web view | Web page |

- `focus.set` calls `makeFirstResponder` on the matching view.
- Mouse clicks change first responder natively; the app observes the change and
  emits `focus.changed`. The TUI never assumes a mode, it only displays what the
  app reports.
- A web page must not be able to swallow browser shortcuts. Give the main menu
  first refusal on the reserved set (override `performKeyEquivalent` on the
  window, or a local event monitor) before the web view sees the event.
- A web page must not be able to take the keyboard either. WebKit makes the
  web view first responder when script focuses an element, so the window
  refuses that move unless it comes from a click or from `focus.set`.

### TUI process lifecycle

- The app starts `tilde-tui` with `TILDE_SOCKET` set to
  `~/Library/Application Support/tilde/tilde-<app pid>.sock`.
- TUI exit code 0 → the app quits.
- Non-zero exit → the web view is hidden, the terminal shows the TUI's output
  and "press any key to restart". On restart the TUI calls `tab.list` and
  re-adopts the tabs that are still loaded.
- `tilde-tui --mock` runs against an in-process fake engine in any terminal, for
  UI development and tests. Without `TILDE_SOCKET` and without `--mock` it prints
  a one-line explanation and exits.

## Protocol v1

One JSON object per line, UTF-8. Requests carry `id`; the app answers each with
`{"id":…, "ok":true, …}` or `{"id":…, "ok":false, "error":"…"}`. Events have
`event` and no `id`. Unknown fields are ignored on both sides.

Requests (TUI → app):

| `method` | Params | Notes |
|---|---|---|
| `hello` | `protocol: 1` | First message. Reply includes `app_version`, current `mode`, and `instance` (added: identifies the app process, so a restarted TUI knows whether saved `tab_id`s are still valid) |
| `viewport.set` | `col`, `row`, `cols`, `rows` | Cell rectangle for the page. Sent on start, resize and layout change |
| `viewport.visible` | `visible: bool` | TUI hides the page when it needs the space |
| `tab.list` | | Reply: `tabs` with full state of each, and `active` |
| `tab.create` | `url?`, `input?`, `profile?`, `activate: bool` | Reply: `tab_id`. `input` (added) is raw URL bar text, resolved like `tab.navigate`, so a new tab needs one request. `profile` (added) names the cookie store; tabs a page opens stay in their opener's profile |
| `tab.close` | `tab_id` | |
| `tab.activate` | `tab_id` | Swaps the visible web view |
| `tab.navigate` | `tab_id`, `input` | Raw URL bar text; the app resolves URL vs. search. Reply includes the resolved `url` (added) |
| `tab.back` / `tab.forward` / `tab.reload` / `tab.stop` | `tab_id` | |
| `focus.set` | `mode: "nav" \| "page"` | Reply includes the resulting `mode` (added); `page` is refused while no page is on screen |
| `download.list` | | Reply has `downloads`, oldest first (added). A download is `download_id`, `url`, `filename`, `path`, `state` (`active`, `done`, `failed`, `cancelled`), `received`, `total` (0 while unknown), `started`, `error` |
| `download.open` / `download.reveal` / `download.cancel` / `download.remove` | `download_id` | Open the file, show it in the Finder, stop it, take it off the list (added). `open` fails with a sentence for the user when the file is gone |
| `download.clear` | | Takes every download that is not running off the list (added) |
| `clipboard.set` | `text` | Puts text on the system clipboard (added). Sent in answer to the `copy` command, which the app emits for Cmd+C while the terminal has focus |
| `net.watch` | `tab_id?` | Starts recording the requests of every tab and reporting those of this one (added, for the dev tools). Reply: `requests`, what the tab has made so far, and `late`, true when the tab's page was loaded before recording began and so is only recorded in part. Without `tab_id` recording goes on and nothing is reported |
| `net.stop` | | Stops recording and forgets everything recorded (added) |
| `net.get` | `tab_id`, `request_id` | Reply: `request`, everything known about one request: the fields of the list plus `status_text`, `protocol`, `body_size`, `detail` (`full`, `document` or `timing`: how much could be seen), `request_headers`, `request_body`, `request_note`, `response_headers`, `response_body`, `response_note`, `response_truncated`, `frames` (added) |
| `net.clear` | `tab_id` | Forgets the tab's requests (added) |
| `net.preserve` | `enabled: bool` | Whether requests are kept when a tab goes to another page (added) |
| `storage.get` | `tab_id` | Answered once WebKit has been asked (added). Reply: `sites` (each `name` and `kinds`), `cookies` (every cookie of the tab's profile: `name`, `value`, `domain`, `path`, `expires?`, `secure`, `http_only`, `same_site`), and for the page the tab is on its `origin`, `site`, `local`, `session` (each `key`, `value`, `size`), `databases` and `caches` |
| `storage.delete` | `tab_id`, `kind`, `name?`, `domain?`, `path?`, `site?`, `all?` | Deletes a `cookie`, a `local` or `session` key, an `indexeddb` database or a `cache` by `name`; with `all`, every one of that kind (cookies: of `site`); kind `site` deletes everything `site` stores (added) |
| `shutdown` | | App quits cleanly |

Events (app → TUI):

| `event` | Fields |
|---|---|
| `tab.created` | `tab_id`, `opener_tab_id`, `url`, `background` (page-initiated tabs only). `background` (added) is true for Cmd-click, and the TUI then keeps its selection |
| `tab.updated` | `tab_id`, any of `url`, `title`, `loading`, `progress`, `can_go_back`, `can_go_forward` |
| `tab.closed` | `tab_id` (`window.close()`) |
| `tab.crashed` | `tab_id` (web content process terminated; TUI offers reload) |
| `focus.changed` | `mode` |
| `download.updated` | the download's fields: one download started, progressed or ended (added) |
| `downloads.changed` | `downloads`: the whole list, sent when downloads are taken off it (added) |
| `command` | `name`, `arg?` (a menu shortcut) |
| `net.updated` | `tab_id`, `requests`, `reset?`, `late`: requests of the watched tab that are new or changed, each `id`, `method`, `url`, `status`, `kind`, `state` (`pending`, `done`, `failed`), `size?`, `cached?`, `duration?`, `started`, `mime`, `error?`. With `reset` they replace the list: the tab went to another page (added) |

URL bar input resolution: has a scheme → use as is; looks like a host
(`localhost`, contains a dot and no spaces, or an IP) → prefix `https://`;
otherwise a DuckDuckGo search URL. `data:` and `javascript:` input is taken as
is even when it contains spaces.

Beyond the spec: Cmd-click opens a link in a background tab; links with a
scheme WebKit does not handle (`mailto:` and the like) go to the system; a
failed load shows an error page that keeps the failed address; stale sockets
from crashed app processes are removed at launch.

A second socket exists only when `TILDE_DEBUG_SOCKET` is set. It serves the
`debug.*` requests used by `scripts/e2e.py` (state dump, the terminal's text,
typing, synthesised keys and clicks, page JavaScript, resize, killing a web
process). It is not
part of the protocol and the TUI never sees it.

## TUI

Layout: fixed 28-column sidebar on the left (tab list, `+ New tab`), one-row
toolbar above the viewport (back/forward/reload state and URL), one-row status
line at the bottom (mode, tab count, loading progress). The viewport rectangle
is left blank; never draw into it. Mouse: click a tab to activate it, click the
URL to edit it.

Keys in `nav` mode:

| Key | Action |
|---|---|
| `j` / `k`, `↓` / `↑` | Select next / previous tab (activates immediately) |
| `Enter` or `i` | Switch to `page` mode |
| `o` | Edit URL of current tab (`Enter` go, `Esc` cancel) |
| `t` | New tab and edit its URL |
| `x` | Close tab |
| `H` / `L` | Back / forward |
| `r` | Reload (stop if loading) |
| `J` / `K` | Move tab down / up |
| `q` | Quit |

Menu shortcuts, available in both modes, delivered as `command`:

| Shortcut | `name` | Notes |
|---|---|---|
| `Cmd+.` | `toggle-mode` | |
| `Cmd+L` | `edit-url` | Switches to `nav` |
| `Cmd+T` | `new-tab` | Switches to `nav`, URL bar open |
| `Cmd+W` | `close-tab` | |
| `Cmd+R` | `reload` | |
| `Cmd+[` / `Cmd+]` | `back` / `forward` | |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | `next-tab` / `prev-tab` | |
| `Cmd+1`…`Cmd+9` | `select-tab` with `arg` | |
| `Cmd+Q` | `quit` | TUI saves the session, then sends `shutdown` |

After a navigation is submitted from the URL bar, switch to `page` mode.

Session: on every tab-set change, write
`~/Library/Application Support/tilde/session.json` (ordered URLs, titles, active
index) atomically. On launch, restore it: create only the active tab in the app
and keep the rest as unloaded entries that are created on first activation.
With no session, open one tab at `https://webkit.org`.

Logs: app to `~/Library/Logs/tilde/app.log`, TUI to `~/Library/Logs/tilde/tui.log`.
Neither writes diagnostics into the terminal view.

## Milestones

Build in this order; each one is runnable.

**M0. Spike the risks (throwaway code is fine).**
An AppKit app with a terminal view running a shell, and a `WKWebView` loading a
real site placed over a hard-coded cell rectangle.
Answer and write down in `docs/spike-notes.md`:
- SwiftTerm: does it expose (or let us derive) exact cell size and grid origin?
  Is alignment within 1 pt at 1x and 2x, and after resize?
- SwiftTerm: rendering quality and speed with a ratatui app redrawing at 60 Hz;
  truecolor, SGR mouse, bracketed paste, synchronized output all present?
- libghostty: is there a usable embedding API for a full terminal surface
  today, and how stable is it? Recommend one of the two with reasons. The
  recommendation decides the `TerminalHost` implementation; the protocol and
  TUI do not change either way.
- Does first responder move cleanly between the terminal view and the web view
  in both directions, by keyboard and by mouse?
- Can the main menu reliably win reserved shortcuts over page JavaScript?
- Does live window resize keep the web view glued to the cell rectangle without
  visible tearing?

**M1. App with protocol.** Window, `TerminalHost`, `TabStore`, socket server,
all requests and events. Driven by hand: run `nc -U "$TILDE_SOCKET"` in the
embedded terminal's shell.

**M2. TUI skeleton.** `tilde-tui` launched by the app, draws the layout, sends
`viewport.set`, one tab, URL bar navigation, `--mock` mode, clean exit.

**M3. Tabs.** Tab list bound to app events, create/close/switch/reorder,
page-initiated tabs, loading progress, back/forward state, mouse support.

**M4. Modes and shortcuts.** Both focus directions, mode indicator, main menu,
Edit menu, reserved-shortcut precedence.

**M5. Robustness.** Resize, session save/restore, TUI-crash restart with
`tab.list` re-adoption, web-process-crash handling, `justfile` with `build`,
`bundle`, `run`, `test`; `Tilde.app` bundles `tilde-tui` and is ad-hoc signed.

## Acceptance

1. `just run` opens a Tilde window showing the TUI with a WebKit page filling the
   viewport rectangle, edges aligned to the cell grid within 1 pt.
2. A YouTube video plays with sound; a text field on a page accepts typing,
   paste, and an IME or dead-key sequence.
3. Ten tabs can be opened, switched with `j`/`k` without visible reload,
   reordered and closed; titles and URLs in the sidebar and toolbar track the page.
4. A `target=_blank` link opens a new tab that appears in the sidebar.
5. `Cmd+L` from the page opens the URL bar; typing `example.com` + `Enter`
   navigates; typing `swift concurrency` + `Enter` searches.
6. `Enter` switches to `page` and `Cmd+.` switches back; the status line always
   shows the true mode, including after mouse clicks on either view.
7. A page that calls `preventDefault()` on every `keydown` cannot block `Cmd+T`,
   `Cmd+W` or `Cmd+L`.
8. Resizing the window, including a live drag, keeps the page glued to its cell
   rectangle; moving the window to a display with a different scale factor does too.
9. Quit and relaunch restores tabs, order and active tab.
10. `kill -9` of `tilde-tui` shows the restart prompt; after restart the same tabs
    are present without reloading. Quitting leaves no `tilde-tui` process.
11. Protocol encode/decode and URL-input resolution have unit tests on both
    sides; the TUI has tests against the mock engine; `just test` passes.

## Known limitations to leave alone

- TUI elements cannot overlap the page (the web view is always above the cells).
- One window, one profile, fixed font.
- No passkeys or password autofill (needs a properly signed bundle with entitlements).

## Report back with

- `docs/spike-notes.md` with the M0 answers and the terminal-view recommendation.
- Any protocol changes made, and why.
- Acceptance items that fail or were verified only partially, stated plainly.
