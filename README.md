# tilde

A web browser whose entire interface is a TUI. Tabs, URL bar, controls and
every other piece of browser UI are drawn by a terminal program; the page
itself is rendered by Apple's WebKit (`WKWebView`) in the main pane.

The goal is a full-featured, keyboard-first browser in the spirit of Arc / Zen /
Nook (vertical tabs, spaces, pinned tabs, command palette, split view) without
giving up real-browser fidelity for the page: video, WebGL, passkeys, smooth
scrolling, IME.

> Status: phase 1 is built, plus spaces (separate sets of tabs with their own
> colour and profile), pinned tabs, tab renaming, a sidebar toggle, downloads,
> history, dev tools (network and storage), a key reference, following links
> and scrolling from the keyboard, a command palette, content blocking, tabs
> that sleep and come back as they were, and the dialogs pages ask for. See
> [docs/HANDOFF-phase-1.md](docs/HANDOFF-phase-1.md) for the protocol and
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

## Install

Download the disk image of the [latest release](https://github.com/eladab/tilde/releases/latest),
open it and drag Tilde to Applications. It needs macOS 14 or later on Apple
silicon.

Releases are not notarized by Apple, so the first time macOS refuses to open
Tilde: allow it under System Settings → Privacy & Security → **Open Anyway**
(or run `xattr -dr com.apple.quarantine /Applications/Tilde.app`).

After that Tilde keeps itself up to date: it offers a new release when there
is one, and **Tilde → Check for Updates…** looks at once.

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

./scripts/dmg.sh           # build/Tilde-<version>.dmg, for drag-to-Applications install

./scripts/test.sh          # unit tests of both halves
python3 scripts/e2e.py     # end-to-end checks on a private copy of build/Tilde.app
                           # (add --background to keep it behind your windows)
```

`bundle.sh` signs the app with the first code-signing identity in your keychain
(or the one named by `TILDE_SIGN_IDENTITY`; `-` means none), and ad hoc if
there is none. A signature carries the name on its certificate, so do not hand
out a build signed with a personal one. With
an ad-hoc signature macOS treats every build as a different app, so keychain
questions such as "Tilde wants to use … Tilde WebCrypto Master Key" (asked by
WebKit for sites like WhatsApp Web) return after each rebuild.

The disk image is signed with the same identity but is not notarized, so on a
Mac other than the one that built it Gatekeeper refuses it until the user
allows it in System Settings → Privacy & Security. Distributing it properly
needs a Developer ID certificate and notarization. The steps are written
down but have not been run, for want of a Developer ID: build with
`TILDE_HARDENED=1` and the Developer ID as `TILDE_SIGN_IDENTITY`, then
`scripts/notarize.sh` (it says what it needs). The release workflow does not
do this yet.

With [`just`](https://github.com/casey/just) installed: `just run`, `just test`,
`just dev` (debug build, no bundle), `just mock` (the TUI alone against a fake
engine, in any terminal).

### Releases

```
./scripts/release.sh 0.2.0        # sets the version, commits, tags v0.2.0
git push origin main v0.2.0       # GitHub builds and publishes the release
```

The pushed tag runs `.github/workflows/release.yml`, which tests, builds the
disk image with an ad-hoc signature, and publishes a release holding
`Tilde-<version>.dmg` and `appcast.xml`.

`appcast.xml` is what makes installed copies update. Tilde embeds
[Sparkle](https://sparkle-project.org), which reads the `appcast.xml` of the
newest release (`SUFeedURL` in `app/Info.plist`), and installs the image it
names only if that image is signed with the private half of `SUPublicEDKey`.
The workflow signs with the repository secret `SPARKLE_PRIVATE_KEY`; without
the secret it still publishes the release, but with no feed.

To release from a fork, make a key pair of your own with
`app/.build/artifacts/sparkle/Sparkle/bin/generate_keys --account tilde`
(after a build), put the public key and your repository's address in
`app/Info.plist`, and store the output of `generate_keys --account tilde -x
<file>` as the secret.

### Keys

In NAV mode:

| Key | Action |
|---|---|
| `j` / `k` | Next / previous tab |
| `Enter` or `i` | Go to the page |
| `f` / `F` | Follow a link by its label / open it in a new tab behind this one |
| `Space`, `Ctrl+D` / `Ctrl+U` | Scroll the page down a screenful / by half a screenful |
| `Ctrl+E` / `Ctrl+Y`, `g` / `G` | Scroll by a line / to the top, the bottom |
| `:` / `>` | Command palette / commands only |
| `/` | Find in the page |
| `o` | Edit the URL |
| `t` / `x` / `u` | New tab / close tab / reopen the tab closed last |
| `m` | Mute or unmute the tab |
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
| `y` | History |
| `d` | Downloads |
| `I` | Dev tools: network and storage |
| `?` | Key reference |
| `q` | Quit |

In both modes: `Cmd+.` switches mode, `Cmd+L` edits the URL (and `Cmd+C` then copies it), `Cmd+T` and
`Cmd+W` open and close tabs, `Cmd+R` reloads, `Cmd+[` and `Cmd+]` go back and
forward, `Ctrl+Tab` and `Ctrl+Shift+Tab` cycle tabs, `Cmd+1`…`Cmd+9` select one,
`Cmd+B` toggles the sidebar, `Cmd+Plus` and `Cmd+Minus` make the page larger
and smaller (`Cmd+0` restores its actual size, and pinching on the trackpad
zooms too), `Cmd+D` pins or unpins, `Cmd+Shift+H` returns to
the pinned page, `Cmd+Y` shows the history, `Opt+Cmd+L` the downloads, `Opt+Cmd+I` the dev tools, `Cmd+/` the key reference, `Opt+Cmd+←` and `Opt+Cmd+→`
switch space, `Ctrl+1`…`Ctrl+9` select one, `Cmd+Shift+N` makes a new one.
`Cmd+Shift+P` opens the command palette, `Cmd+J` labels the page's links
(`Cmd+Shift+J` to open one in a new tab), `Cmd+Shift+T` reopens the tab closed
last, `Opt+Cmd+M` mutes or unmutes the tab.
`Cmd+\` fills the focused page field from the macOS Passwords app: pick a login
in the system picker, then its username or password.
Cmd-click opens a link in a background tab. Rename is in the Tabs menu; editing
and deleting spaces are in the Spaces menu.

### The page from the keyboard

In NAV mode the page can be read and used without being given the keys.

**Scrolling.** `Space` and `Ctrl+F` go down a screenful, `Ctrl+B` up;
`Ctrl+D` / `Ctrl+U` half a screenful; `Ctrl+E` / `Ctrl+Y` a line; `g` and `G`
the top and the bottom. In an application that scrolls a pane of its own, the
pane under the middle of the window is the one that moves.

**Following links.** `f` puts a short label on every link, button and field
on screen, and the status line says `HINT`. Type a label: a link is followed,
a button pressed, and a field is given the keys (PAGE mode) with the cursor
in it. `F` does the same but opens a link in a new tab behind the one
showing. `Backspace` takes back a letter, `Esc` takes the labels down. From
the page, `Cmd+J` and `Cmd+Shift+J` do this and hand the keys back
afterwards.

Labels are put on what is in the page's top frame, including inside shadow
roots; what is inside an embedded frame is not reached. The script that
draws them runs apart from the page's own, and nothing is put in a page until
one of these keys is first used on it.

### Command palette

`:` (or `Cmd+Shift+P`, from the page too) opens one field that finds:

- **a tab**, in any space, by its name or address: `Enter` goes to it;
- **a page from the history**, once something is typed, the pages gone to
  most often first: `Enter` opens it in a new tab;
- **a closed tab**: `Enter` reopens it as it was;
- **a command**, by name, with its key shown beside it; `>` lists commands only;
- or, failing all of those, **opens what was typed** as an address or a search.

Every word typed must be found; the start of a word counts for more than its
middle, and a title for more than an address. `↑` / `↓` (or `Ctrl+P` /
`Ctrl+N`) move, `Esc` closes.

The palette is drawn over the page. Nothing can really be drawn over a web
view, so while it is open the app shows a still of the page, dimmed, with the
palette's cells cut out of it.

### Tabs that sleep

A tab that has been out of sight for 30 minutes is put to sleep: its page is
let go, which gives its memory back, and the tab stays in the sidebar, shown
fainter. Selecting it brings the page back as it was: the same pages back and
forward, scrolled to the same place. Pinned tabs never sleep (they are the
ones kept open on purpose), nor does a tab that is playing sound, using the
camera or the microphone, or still loading. **Put the Other Tabs to Sleep**
(Tabs menu, or the palette) does it at once.

The same kept state is what makes `u` (`Cmd+Shift+T`) reopen a closed tab
with its history, and what restores every tab's pages back and forward after
a restart. States are files in `tabstate/` beside the session; those of tabs
that no longer exist are removed at launch.

The time is `discard_after_minutes` in `settings.json` (see
[Settings](#settings)); `0` never puts a tab to sleep.

### Content blocking

Requests to advertising and tracking networks are blocked, in WebKit itself
(`WKContentRuleList`): a blocked request is never made and its script never
runs. Only requests from another site's page are blocked, so a network's own
site still opens.

The rules come from a short list built into Tilde and from
[Peter Lowe's list of ad and tracking servers](https://pgl.yoyo.org/adservers/),
fetched once a week and kept in `blocklist.txt` beside the session. A site
that breaks can be exempted: **Content Blocking On or Off for This Site** in
the View menu or the palette, which reloads the page and is remembered
(`blocking.json`). `settings.json` can switch blocking off altogether or name
another list.

This blocks by domain. It does not hide the empty spaces ads leave, and it
does not read the element-hiding rules of lists such as EasyList.

### What pages ask for

- `alert`, `confirm` and `prompt` are sheets on the window, headed by the
  site that asks. From its third dialog on, a page can be told to show no
  more until the tab goes elsewhere.
- A file field (`<input type=file>`) opens the system's file picker.
- A site that asks for a name and password (HTTP authentication) gets a
  sheet for them; they are kept while Tilde runs, not on disk.
- A site that asks for a client certificate is shown the identities in the
  keychain to choose from.

### Sound, zoom and icons

A tab that is playing sound shows `♪` at the end of its row; `m` (or
`Opt+Cmd+M`) mutes it, and it then says `♪ off`.

Page zoom is remembered per site (`zoom.json`): a site made larger is larger
in every tab and after a restart.

The sidebar shows each site's icon before the tab's name. A terminal cannot
draw a picture, so the TUI tells the app which cells belong to which site and
the app lays the icons over them. Icons are fetched apart from the page, with
no cookies, and kept in `icons/` beside the session.

### Settings

`~/Library/Application Support/tilde/settings.json` is optional, and so is
every key in it:

```json
{
  "discard_after_minutes": 30,
  "content_blocking": true,
  "blocklist_url": "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=nohtml&showintro=0&mimetype=plaintext"
}
```

`blocklist_url` must be `https` and may be a list of domains, one per line,
or a hosts file; an empty string fetches nothing. Changes take effect at the
next launch.

### Default browser

Tilde opens web links and HTML files for other apps: each becomes a new tab of
the space that is showing. To make it the default browser, install it in
`/Applications` (from the disk image), open it once, then either choose
**Tilde → Make Tilde the Default Browser…** or pick Tilde under System Settings
→ Desktop & Dock → Default web browser. macOS asks you to confirm.

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
to move a tab between spaces or to erase a profile. The command palette finds
and goes to a tab of any space.

### Pinned tabs

Pinned tabs are the top section of the sidebar, above a rule. Pinning records
the tab's address at that moment; browsing elsewhere does not move it, and `P`
goes back to it. It records the page's title too, and the tab keeps showing
that title while it is on the same site (the same host, ignoring `www.`); on
another site it shows that page's title, until it comes back. A name given with
`n` always shows. Closing a pinned tab unloads its page but keeps the pin; unpin
it first to remove it. Pins belong to the space they were made in. Pins, names
and the sidebar state are saved in the session.

### Downloads

A page that offers a file (a link marked for download, a response the server
says to save, or a type WebKit cannot show) downloads it to `~/Downloads`; a
name already taken gets a number. While a download runs, the status line shows
its name and progress; when it ends it shows the name with a tick until the
Downloads view has been opened. Clicking that notice opens the view.

The Downloads view (`d`, or `Opt+Cmd+L`) lists every download, running and
past, newest first. It is one list for the whole browser, whatever space is
showing.

| Key | Action |
|---|---|
| `j` / `k` | Move |
| `Enter` or `o` | Open the file |
| `f` | Show it in the Finder |
| `x` | Stop a running download; take a finished one off the list |
| `C` | Clear the list (running downloads stay) |
| `Esc` | Close the view |

Taking a download off the list never deletes its file. A download that is
stopped or fails leaves no partial file, and one that was running when Tilde
quit is listed as failed; downloads are not resumed.

### History

Every page visited is recorded with the time and the space it was visited in.
The History view (`y`, or `Cmd+Y`) lists them all, from every space, newest
first: pages of the last hour under "Recent", then "Earlier today", then
"Yesterday", then one heading per date. Each line ends with its space, in the
space's colour. A page visited again on the same day is listed once, at its
latest time.

| Key | Action |
|---|---|
| `j` / `k`, `g` / `G`, `Ctrl+D` / `Ctrl+U` | Move by line, to either end, by screenful |
| `/` | Filter: only pages whose title or address has every word typed. `Enter` goes back to the list, `Esc` drops the filter |
| `Enter` | Open the page in a new tab of the current space |
| `s` | Open it in a new tab of the space it was visited in |
| `x` | Forget this visit |
| `C` | Clear all history (asks first) |
| `Esc` | Close the view |

Restoring tabs at launch is not a visit, and neither are blank or `data:` pages.
The newest 10,000 visits are kept. The command palette searches them too.

Both views take the place of the page, like the key reference, and a click on a
line selects it (a second click opens it).

### Dev tools

`I` (or `Opt+Cmd+I`, from the page too) opens a pane beside the page with two
tools, **Network** and **Storage**. The page stays on screen and stays usable:
it is given the columns the pane does not take. While the pane has the keys
the status line says `DEV`; `Esc` hands them back to the tabs without closing
it, `I` takes them again, and `q` closes it.

```
│  ←  →  ↻   https://example.com/                                           │
│                        │  Network   Storage             14 requests · 373 KB │
│                        │  All  Fetch  Doc  JS  CSS  Img  Media  WS  Other    │
│                        │ St       Name                        Size     Time  │
│                        │▎200 GET  example.com/                18 KB   212 ms │
│                        │ 200 GET  site.css?v=42             9.1 KB    38 ms  │
│        the page        │  —  GET  logo.svg                   cache     0 ms  │
│                        │ 200 POST search?q=terminal+brow…   1.9 KB   187 ms  │
│                        │ 404 GET  notifications               96 B    45 ms  │
│                        │ ERR GET  collect?e=pageview&t=1…        —   failed  │
│                        │ ··· GET  feed?cursor=eyJvIjoyMH0        —  pending  │
│                        │ j/k move   ⏎ details   f filter   / search   c copy │
```

| Key | Action |
|---|---|
| `Tab`, `1` / `2` | Switch between Network and Storage |
| `j` / `k`, `g` / `G`, `Ctrl+D` / `Ctrl+U` | Move by line, to either end, by screenful |
| `Enter` | Open the selected item's details; `Esc` goes back |
| `/` | Search the list; `Esc` clears the search |
| `c` (or `Cmd+C`) | Copy: a request's address or the part of it that is showing, a stored value |
| `m` | Move the pane below the page, or back beside it |
| `-` / `+` | Make the pane smaller / larger |
| `Esc` | Give the keys back to the tabs |
| `q` | Close the pane |

A pane wide enough (below the page, in a wide window) shows the list and the
selected item's details side by side. The mouse works too: click a tool, a
filter, a line (twice to open it), and scroll. Where the pane sits and how
large it is are saved in the session.

**Network** lists the requests of the tab that is showing, in the order they
began: status, method, name, type, size and time. Requests are recorded while
the pane is open, for every tab, so switching tabs shows that tab's own list.
Opening it on a page that has already loaded lists what the page fetched so
far, by address and time only, and a notice above the list says so: press `r`
to reload the page and record everything in full. Without the reload some
later requests are recorded in full and some are not (see below). The list starts
afresh when the tab goes to another page, unless `p` is on.

| Key | Action |
|---|---|
| `f` / `F` | Next / previous filter: all, fetch and XHR, documents, scripts, styles, images, media, WebSockets, other |
| `G` | Go to the newest request and stay on the newest as more arrive |
| `h` / `l` (in the details) | Headers, payload, response |
| `J` / `K` (in the details) | Next / previous request |
| `C` | Copy the request as a `curl` command |
| `x` | Clear the list |
| `p` | Keep the list across pages |
| `r` | Reload the page |

The details have three parts. *Headers*: address, method, status, type, size,
time and protocol, then the response and request headers. *Payload*: the query
parameters, decoded, and the body that was sent (JSON is re-indented, form
data is listed field by field). *Response*: the body, with JSON re-indented and
coloured; for a WebSocket, its messages in both directions.

How much is shown depends on how the request was made, because WebKit gives an
app no way to watch a web view's traffic. Tilde puts the list together from
three things it can see:

| Request | What is shown |
|---|---|
| `fetch`, `XMLHttpRequest`, `WebSocket` | Everything the page's own script could see: the headers it set, the body it sent, status, response headers and the response body when it is text (the first 512 KB of it). A script that Tilde adds to pages while the pane is open records these |
| The document of a page or frame | Status, response headers, type, size and time, from WebKit's navigation callbacks. Not its body |
| Everything else (scripts, styles, images, fonts, media) | Address, time, and size when the server allows it to be measured, from WebKit's resource timings. No status, headers or body; `cache` in the size column means it was not fetched at all |

Never shown: the headers WebKit adds itself (cookies, user agent, and the
like), response headers a server on another origin does not expose to scripts,
and requests made by workers, which are not listed at all.

Nothing touches a page's requests while the pane is closed. The script that
records `fetch`, `XMLHttpRequest` and `WebSocket` is added when the pane opens:
to every page loaded from then on, at its start, and to the pages already
there. Closing the pane takes it out again and gives each page its own
functions back.

That is why a page loaded before the pane opened is only recorded in part.
Pages and their libraries keep their own reference to `fetch` as they load,
and calls made through it go around a wrapper added afterwards; they are
listed with their timing only. Reloading with the pane open records
everything.

**Storage** shows what one site stores in the space's profile: cookies, local
storage, session storage, IndexedDB and Cache Storage. It opens on the site of
the page in the tab and follows the tab. `s` lists every site that has stored
anything in the profile, with how many cookies and what else; `Enter` there
shows that site.

| Key | Action |
|---|---|
| `h` / `l` | Previous / next kind of storage |
| `s` | The list of sites; in it, `X` deletes everything a site stores (asks first) |
| `J` / `K` (in the details) | Next / previous item |
| `x` | Delete the selected cookie, key, database or cache |
| `X` | Delete everything of this kind for the site (asks first) |
| `r` | Read the storage again; it is also read again when the page has loaded |

Cookies are complete for every site, `HttpOnly` ones included, with domain,
path, expiry, `Secure` and `SameSite`; a value that is URL-encoded is shown
decoded as well. The other kinds can only be read by a page of their own
origin, so they are shown for the page in the tab (its top frame) and not for
other sites, for which the pane says what WebKit reports them to hold. Session
storage is the tab's own. IndexedDB shows each object store with its number of
records and the first 25 of them; Cache Storage shows each cache with the
addresses of its first 100 entries. Values are read-only: they can be copied
and deleted, not edited.

The key reference (`?`) is shown in the page area, since nothing can be drawn
over the page; any key puts the page back.

Files: session, `history.json`, `downloads.json`, `permissions.json`,
`zoom.json`, `blocking.json`, `blocklist.txt`, the optional `settings.json`
and the folders `tabstate/` and `icons/` in
`~/Library/Application Support/tilde/`, logs in `~/Library/Logs/tilde/`.

## Requirements

- macOS 14 or later (developed on macOS 27)
- Swift 6 toolchain, Rust stable
- No particular terminal emulator: Tilde embeds its own terminal view

## Roadmap

| Phase | Theme | Contents |
|---|---|---|
| 1 (built) | Foundation | App shell with embedded terminal, engine, TUI. Vertical tab list, URL bar, back/forward/reload, NAV/PAGE modes, session restore. See the handoff. |
| 2 (built, but for two items) | Daily driver | Command palette, URL autocomplete from history, find in page, JS dialogs / HTTP auth / file pickers, favicons in the sidebar, the palette over a still of the page, link hints and scrolling from NAV, tabs that sleep, reopening closed tabs. Still to do: resuming downloads, context menu actions |
| 3 | Arc-style organisation | Folders, favourites, tab archiving, split view (several web views), peek windows, moving tabs between spaces, erasing a profile, multiple windows |
| 4 | Power features | Content blocking by domain is built; element hiding is not. Userscripts / per-site styles, WebExtensions (`WKWebExtension`), reader mode, picture-in-picture, the rest of the dev tools (console, elements; full request capture through a local proxy), passkeys, themes and font configuration, link hints inside frames |
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
│       ├── TildeCore/        URL-bar resolution, wire format, request log, blocking rules (unit tested)
│       └── Tilde/            window, terminal host, tab store, socket server, dev tools' network and storage,
│                             page keys (scrolling, hints), content blocker, dialogs, tab state, icons
├── tui/                     Rust crate → tilde-tui
│   └── src/                 app logic, views, history, palette, reach (hints, sleep, closed tabs), dev tools,
│                             drawing, protocol, mock engine
├── scripts/                 bundle.sh, dmg.sh, release.sh, appcast.sh, notarize.sh, test.sh, e2e.py, make-icon.sh
├── .github/workflows/       tests on every push; a release on every version tag
└── justfile                 thin wrappers over scripts/
```

## License

[MIT](LICENSE).

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
