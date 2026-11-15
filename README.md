# weft

A web browser whose entire interface is a TUI. Tabs, URL bar, controls and
every other piece of browser UI are drawn by a terminal program; the page
itself is rendered by Apple's WebKit (`WKWebView`) in the main pane.

The goal is a full-featured, keyboard-first browser in the spirit of Arc / Zen /
Nook (vertical tabs, spaces, pinned tabs, command palette, split view) without
giving up real-browser fidelity for the page: video, WebGL, passkeys, smooth
scrolling, IME.

> Status: phase 1 is built. It browses, with tabs, modes and session restore,
> and nothing beyond that yet. See [docs/HANDOFF-phase-1.md](docs/HANDOFF-phase-1.md)
> for what is and is not verified.

```
┌ Weft ──────────────────────────────────────────────────────────────┐
│ ● Space: work      │ ← → ⟳  https://webkit.org/                    │
│────────────────────│───────────────────────────────────────────────│
│ ▸ WebKit           │                                               │
│   Hacker News      │                                               │
│   GitHub – weft    │        native WKWebView, laid exactly         │
│   MDN: CSS grid    │        over this rectangle of cells           │
│                    │                                               │
│                    │                                               │
│ + New tab          │                                               │
│────────────────────│───────────────────────────────────────────────│
│ NAV  4 tabs        │ Loading… 62%                                  │
└────────────────────────────────────────────────────────────────────┘
```

## How it works

Weft is a small native macOS app with one window. That window contains two
things: an embedded terminal view that fills it, and a `WKWebView` placed on top
of the rectangle of cells the TUI leaves empty for the page.

| Part | Language | Owns |
|---|---|---|
| `Weft.app` | Swift, AppKit + WebKit | The window, the embedded terminal view, the `WKWebView`s, navigation and web state, the menu bar and its shortcuts |
| `weft-tui` | Rust, ratatui + crossterm | All browser UI, layout, keybindings, tab organisation, session file |

The app starts `weft-tui` inside its embedded terminal and the two talk over a
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

## Build and run

```
./scripts/bundle.sh        # release build → build/Weft.app
open build/Weft.app

./scripts/test.sh          # unit tests of both halves
python3 scripts/e2e.py     # end-to-end checks against build/Weft.app
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
| `J` / `K` | Move the tab down / up |
| `q` | Quit |

In both modes: `Cmd+.` switches mode, `Cmd+L` edits the URL, `Cmd+T` and
`Cmd+W` open and close tabs, `Cmd+R` reloads, `Cmd+[` and `Cmd+]` go back and
forward, `Ctrl+Tab` and `Ctrl+Shift+Tab` cycle tabs, `Cmd+1`…`Cmd+9` select one.
Cmd-click opens a link in a background tab.

Files: session in `~/Library/Application Support/weft/session.json`, logs in
`~/Library/Logs/weft/`.

## Requirements

- macOS 15 or later (developed on macOS 27)
- Swift 6 toolchain, Rust stable
- No particular terminal emulator: Weft embeds its own terminal view

## Roadmap

| Phase | Theme | Contents |
|---|---|---|
| 1 (built) | Foundation | App shell with embedded terminal, engine, TUI. Vertical tab list, URL bar, back/forward/reload, NAV/PAGE modes, session restore. See the handoff. |
| 2 | Daily driver | Command palette, history and URL autocomplete, find in page, downloads, zoom, JS dialogs / HTTP auth / file pickers, context menu actions, favicons in the sidebar, pinned tabs, TUI popups over the page (page swapped for a still snapshot while they are open) |
| 3 | Arc-style organisation | Spaces, folders, favourites, tab archiving, split view (several web views), peek windows, profiles (separate `WKWebsiteDataStore` per space), multiple windows |
| 4 | Power features | Content blocking (`WKContentRuleList`), userscripts / per-site styles, WebExtensions (`WKWebExtension`), reader mode, picture-in-picture, Web Inspector, passkeys and password autofill, default-browser registration, themes and font configuration |
| 5 | Beyond the app | Running the TUI in the user's own terminal (overlay or in-band frame streaming), remote use |

## Repository layout

```
weft/
├── README.md
├── docs/
│   ├── HANDOFF-phase-1.md   phase 1 spec, status and acceptance criteria
│   └── spike-notes.md       what the first spike established
├── app/                     Swift package → Weft.app
│   └── Sources/
│       ├── WeftCore/        URL-bar resolution, wire format (unit tested)
│       └── Weft/            window, terminal host, tab store, socket server
├── tui/                     Rust crate → weft-tui
│   └── src/                 app logic, drawing, protocol, mock engine
├── scripts/                 bundle.sh, test.sh, e2e.py
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
