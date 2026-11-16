# tilde

A web browser whose entire interface is a TUI. Tabs, URL bar, controls and
every other piece of browser UI are drawn by a terminal program; the page
itself is rendered by Apple's WebKit (`WKWebView`) in the main pane.

The goal is a full-featured, keyboard-first browser in the spirit of Arc / Zen /
Nook (vertical tabs, spaces, pinned tabs, command palette, split view) without
giving up real-browser fidelity for the page: video, WebGL, passkeys, smooth
scrolling, IME.

> Status: phase 1 is built, plus spaces (separate sets of tabs with their own
> colour and profile), pinned tabs, tab renaming, a sidebar toggle and a key
> reference. See [docs/HANDOFF-phase-1.md](docs/HANDOFF-phase-1.md)
> for what is and is not verified.

```
┌ Tilde ──────────────────────────────────────────────────────────────┐
│ ● Space: work      │ ← → ⟳  https://webkit.org/                    │
│────────────────────│───────────────────────────────────────────────│
│ ▸ WebKit           │                                               │
│   Hacker News      │                                               │
│   GitHub – tilde    │        native WKWebView, laid exactly         │
│   MDN: CSS grid    │        over this rectangle of cells           │
│                    │                                               │
│                    │                                               │
│ + New tab          │                                               │
│────────────────────│───────────────────────────────────────────────│
│ NAV  4 tabs        │ Loading… 62%                                  │
└────────────────────────────────────────────────────────────────────┘
```

## How it works

Tilde is a small native macOS app with one window. That window contains two
things: an embedded terminal view that fills it, and a `WKWebView` placed on top
of the rectangle of cells the TUI leaves empty for the page.

| Part | Language | Owns |
|---|---|---|
| `Tilde.app` | Swift, AppKit + WebKit | The window, the embedded terminal view, the `WKWebView`s, navigation and web state, the menu bar and its shortcuts |
| `tilde-tui` | Rust, ratatui + crossterm | All browser UI, layout, keybindings, tab organisation, session file |

The app starts `tilde-tui` inside its embedded terminal and the two talk over a
Unix socket. The TUI says "the page goes at these cells"; the app, which owns
both views, converts cells to points and positions the web view. Because both
views live in one window we control, alignment is exact, nothing has to track
another app's window, and no system permissions are needed.

### Why the UI is a separate terminal program

- The interface really is a TUI, built with a real TUI toolkit, not a native UI
  styled to look like one.
- The app can keep tabs alive if the TUI crashes or is restarted.
- The same TUI and protocol can later drive other ways of showing the page
  (see [Alternatives](#alternatives-considered)).

### Modes

There are two keyboard modes, and the status line always shows which is active:

- **NAV**: the TUI has keyboard focus. Plain vim-style keys drive the browser
  (`j`/`k` between tabs, `o` to open a URL, `x` to close).
- **PAGE**: the web view has keyboard focus. Keys go to the web page.

`Cmd` shortcuts (`Cmd+L`, `Cmd+T`, `Cmd+W`, …) are menu items of the app, so
they work the same in both modes.

The name is the character a shell uses for "home". The sidebar shows the
current space the same way, as `~/Work` or `~/Personal`.

The project was first called Weft, and its spaces were called looms. The first
launch of Tilde moves the old session, cookies and site data to their new
locations, so tabs and logins carry over; sessions in the old format are read
as they are.

## Build and run

```
./scripts/bundle.sh        # release build → build/Tilde.app
open build/Tilde.app

./scripts/test.sh          # unit tests of both halves
python3 scripts/e2e.py     # end-to-end checks on a private copy of build/Tilde.app
                           # (add --background to keep it behind your windows)
```

With [`just`](https://github.com/casey/just) installed: `just run`, `just test`,
`just dev` (debug build, no bundle), `just mock` (the TUI alone against a fake
engine, in any terminal).

### Keys

In NAV mode:

| Key | Action |
|---|---|
| `j` / `k` | Next / previous tab |
| `Enter` or `i` | Go to the page |
| `o` | Edit the URL |
| `t` / `x` | New tab / close tab |
| `H` / `L` | Back / forward |
| `r` | Reload, or stop while loading |
| `J` / `K` | Move the tab down / up (within its section) |
| `p` | Pin or unpin the tab |
| `P` | Return a pinned tab to the page it was pinned at |
| `n` | Rename the tab (an empty name restores the page title) |
| `b` | Show or hide the sidebar |
| `h` / `l` | Previous / next space |
| `N` | New space |
| `e` | Edit the space: name, colour, background, art, profile |
| `D` | Delete the space (asks first) |
| `?` | Key reference |
| `q` | Quit |

In both modes: `Cmd+.` switches mode, `Cmd+L` edits the URL, `Cmd+T` and
`Cmd+W` open and close tabs, `Cmd+R` reloads, `Cmd+[` and `Cmd+]` go back and
forward, `Ctrl+Tab` and `Ctrl+Shift+Tab` cycle tabs, `Cmd+1`…`Cmd+9` select one,
`Cmd+B` toggles the sidebar, `Cmd+D` pins or unpins, `Cmd+Shift+H` returns to
the pinned page, `Cmd+/` shows the key reference, `Opt+Cmd+←` and `Opt+Cmd+→`
switch space, `Ctrl+1`…`Ctrl+9` select one, `Cmd+Shift+N` makes a new one.
Cmd-click opens a link in a background tab. Rename is in the Tabs menu; editing
and deleting spaces are in the Spaces menu.

### Spaces

A space is a separate set of tabs: its own pinned tabs, its own ordinary tabs,
its own selection. The sidebar is headed by the space's name in its colour, with
one dot per space underneath (click a dot to switch). Tabs of the other spaces
stay loaded, so switching is instant and nothing reloads.

Each space has:

- **A name.**
- **A colour**, one of eight, used for the space's name, the selection marker
  and the mode badge.
- **A background**, one of eight dark tints (ink, graphite, midnight, ocean,
  forest, plum, wine, ember) that colours the whole interface while the space
  is showing.
- **Art**, optionally: one of five small pixel-art pieces (mountains, sunset,
  cat, plant, rocket), drawn under the tabs when the sidebar has room, and on
  blank tabs.
- **A profile**: a separate store of cookies, logins and site data. A new space
  gets a profile of its own, named after it, so signing in to the same site
  with a different account in each space just works. To share logins between
  spaces, give them the same profile name; `default` is the profile of the
  first space. Changing a space's profile reopens its tabs in the new one.

The space editor previews colour, background and art in the window as you move
through the choices; Escape discards them. Backgrounds and pieces are defined
in `tui/src/art.rs`: a piece is a grid of characters and a palette, so adding
one is a matter of drawing it there.

Deleting a space closes its tabs. The profile's data stays on disk, so a space
made later with the same profile name is signed in again. There is no way yet
to move a tab between spaces or to erase a profile.

### Pinned tabs

Pinned tabs are the top section of the sidebar, above a rule. Pinning records
the tab's address at that moment; browsing elsewhere does not move it, and `P`
goes back to it. Closing a pinned tab unloads its page but keeps the pin; unpin
it first to remove it. Pins belong to the space they were made in. Pins, names
and the sidebar state are saved in the session.

The key reference (`?`) is shown in the page area, since nothing can be drawn
over the page; any key puts the page back.

Files: session in `~/Library/Application Support/tilde/session.json`, logs in
`~/Library/Logs/tilde/`.

## Requirements

- macOS 15 or later (developed on macOS 27)
- Swift 6 toolchain, Rust stable
- No particular terminal emulator: Tilde embeds its own terminal view

## Roadmap

| Phase | Theme | Contents |
|---|---|---|
| 1 (built) | Foundation | App shell with embedded terminal, engine, TUI. Vertical tab list, URL bar, back/forward/reload, NAV/PAGE modes, session restore. See the handoff. |
| 2 | Daily driver | Command palette, history and URL autocomplete, find in page, downloads, zoom, JS dialogs / HTTP auth / file pickers, context menu actions, favicons in the sidebar, TUI popups over the page (page swapped for a still snapshot while they are open) |
| 3 | Arc-style organisation | Folders, favourites, tab archiving, split view (several web views), peek windows, moving tabs between spaces, erasing a profile, multiple windows |
| 4 | Power features | Content blocking (`WKContentRuleList`), userscripts / per-site styles, WebExtensions (`WKWebExtension`), reader mode, picture-in-picture, Web Inspector, passkeys and password autofill, default-browser registration, themes and font configuration |
| 5 | Beyond the app | Running the TUI in the user's own terminal (overlay or in-band frame streaming), remote use |

## Repository layout

```
tilde/
├── README.md
├── docs/
│   ├── HANDOFF-phase-1.md   phase 1 spec, status and acceptance criteria
│   └── spike-notes.md       what the first spike established
├── app/                     Swift package → Tilde.app
│   └── Sources/
│       ├── TildeCore/        URL-bar resolution, wire format (unit tested)
│       └── Tilde/            window, terminal host, tab store, socket server
├── tui/                     Rust crate → tilde-tui
│   └── src/                 app logic, drawing, protocol, mock engine
├── scripts/                 bundle.sh, test.sh, e2e.py, make-icon.sh
└── justfile                 thin wrappers over scripts/
```

## Alternatives considered

Both keep the same TUI and protocol, and remain possible later.

| Approach | How | Why not now |
|---|---|---|
| Overlay on the user's terminal | Borderless WebKit window floated over a Ghostty window and made to follow it | Needs cross-process window tracking and ordering; lags during drags; breaks with terminal tabs and splits |
| In-band frame streaming | Off-screen `WKWebView`, frames sent with the kitty graphics protocol, input synthesised from terminal events | Feels like screen sharing: low frame rate, no native popups, no protected video, fragile text input |

## Non-goals

- Cross-platform support. WebKit-on-macOS is the point.
- Rendering pages as text or half-block characters (see browsh, carbonyl).
- Being a general-purpose terminal emulator.
