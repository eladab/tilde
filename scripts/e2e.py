#!/usr/bin/env python3
"""End-to-end acceptance checks for Tilde.

Launches build/Tilde.app with a throwaway session and drives it through the
debug socket (enabled only by TILDE_DEBUG_SOCKET): typing into the TUI,
synthesised key presses and clicks, and JavaScript probes in the page.

The app under test is a private copy of the bundle with its own bundle
identifier, so it has its own WebKit data and never touches the logins or site
data of the real Tilde, and it can run while the real Tilde is open. Only the
processes this script starts are ever signalled.

Needs network access and takes about a minute and a half. Run
`scripts/bundle.sh` first.

    python3 scripts/e2e.py                # takes keyboard focus while it runs
    python3 scripts/e2e.py --background   # stays behind your windows; skips the
                                          # checks that need real focus (clicks)
"""
import base64, hashlib, http.server, json, os, signal, socket, subprocess, sys, tempfile, threading, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCE_APP = os.path.join(ROOT, "build/Tilde.app")
# Kept short: Unix socket paths are limited to about 100 characters.
WORK = tempfile.mkdtemp(prefix="e", dir=os.environ.get("TILDE_E2E_TMP", "/tmp"))
APP = os.path.join(WORK, "TildeE2E.app")
BACKGROUND = "--background" in sys.argv
DEBUG_SOCK = os.path.join(WORK, "d.sock")
DOWNLOADS = os.path.join(WORK, "downloads")
SESSION = os.path.join(WORK, "session.json")
failures = []
app_pid = None  # the instance this script started; nothing else is ever signalled
skipped = []


def prepare_app():
    """Copies the bundle and gives the copy its own identity."""
    subprocess.run(["cp", "-R", SOURCE_APP, APP], check=True)
    plist = os.path.join(APP, "Contents/Info.plist")
    for key, value in [("CFBundleIdentifier", "dev.tilde.Tilde.e2e"), ("CFBundleName", "TildeE2E"),
                       ("CFBundleDisplayName", "TildeE2E"), ("CFBundleExecutable", "TildeE2E")]:
        subprocess.run(["plutil", "-replace", key, "-string", value, plist], check=True)
    # The copy must not offer itself to macOS as a browser.
    for key in ["CFBundleURLTypes", "CFBundleDocumentTypes"]:
        subprocess.run(["plutil", "-remove", key, plist], check=True)
    os.rename(os.path.join(APP, "Contents/MacOS/Tilde"), os.path.join(APP, "Contents/MacOS/TildeE2E"))
    subprocess.run(["codesign", "--force", "--sign", "-", APP], check=True, capture_output=True)


def alive(pid):
    if not pid:
        return False
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def skip(name, why):
    print("SKIP  " + name + "   [" + why + "]")
    skipped.append(name)


class Debug:
    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.connect(path)
        self.file = self.sock.makefile("rw")
        self.n = 0

    def call(self, method, **params):
        self.n += 1
        params.update(id=self.n, method=method)
        self.file.write(json.dumps(params) + "\n")
        self.file.flush()
        while True:
            message = json.loads(self.file.readline())
            if message.get("id") == self.n:
                return message


def check(name, ok, detail=""):
    print(("PASS  " if ok else "FAIL  ") + name + (f"   [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(name)


tui_pid = None


def processes():
    """The app and TUI processes of this run that are still alive."""
    return [pid for pid in (app_pid, tui_pid) if alive(pid)]


def wait_gone(timeout=10):
    end = time.time() + timeout
    while time.time() < end:
        if not processes():
            return True
        time.sleep(0.2)
    return False


def launch():
    global app_pid, tui_pid
    if os.path.exists(DEBUG_SOCK):
        os.remove(DEBUG_SOCK)
    command = ["open", "-n", APP, "--env", f"TILDE_DEBUG_SOCKET={DEBUG_SOCK}", "--env", f"TILDE_SESSION={SESSION}",
               "--env", f"TILDE_DOWNLOADS={DOWNLOADS}"]
    if BACKGROUND:
        command[1:1] = ["-g"]
        command += ["--env", "TILDE_BACKGROUND=1"]
    subprocess.run(command, check=True)
    end = time.time() + 15
    while not os.path.exists(DEBUG_SOCK):
        if time.time() > end:
            sys.exit("the app did not create its debug socket")
        time.sleep(0.1)
    time.sleep(0.3)
    d = Debug(DEBUG_SOCK)
    end = time.time() + 10
    while True:
        s = d.call("debug.state")
        if s.get("tui_pid"):
            app_pid, tui_pid = s["pid"], s["tui_pid"]
            break
        if time.time() > end:
            sys.exit("the app did not start its TUI")
        time.sleep(0.2)
    if BACKGROUND:
        return d
    # Synthesised clicks only reach the views of the active app, and an app
    # opened from a background process is not always activated.
    end = time.time() + 10
    while not d.call("debug.state").get("app_active"):
        if time.time() > end:
            sys.exit("could not bring Tilde to the front (is the screen locked or asleep?)")
        d.call("debug.activate")
        time.sleep(0.3)
    return d


def state(d):
    return d.call("debug.state")


def active(s):
    return next(t for t in s["tabs"] if t["tab_id"] == s.get("active"))


def wait(d, predicate, what, timeout=30):
    end = time.time() + timeout
    while time.time() < end:
        s = state(d)
        try:
            if predicate(s):
                return s
        except (KeyError, StopIteration):
            pass
        time.sleep(0.2)
    print(json.dumps(state(d), indent=1))
    sys.exit("TIMEOUT waiting for: " + what)


def typ(d, text):
    """Types into the TUI (bytes to its pty)."""
    d.call("debug.type", text=text)
    time.sleep(0.25)


def key(d, chars, mods="", code=0):
    """A key press through AppKit: reaches the menu, the window or the page."""
    d.call("debug.key", chars=chars, mods=mods, key_code=code)
    time.sleep(0.3)


def js(d, script):
    return d.call("debug.js", script=script).get("result")


def nav(d):
    if state(d)["mode"] == "page":
        key(d, ".", "cmd", 47)
    wait(d, lambda s: s["mode"] == "nav", "NAV mode")


def frame_matches_cells(s):
    vp, cell, frame, content = s["viewport"], s["cell"], s["web_frame"], s["content"]
    expected = {
        "x": vp["col"] * cell["w"],
        "w": vp["cols"] * cell["w"],
        "h": vp["rows"] * cell["h"],
        "y": content["h"] - (vp["row"] + vp["rows"]) * cell["h"],
    }
    return all(abs(frame[k] - expected[k]) < 0.01 for k in expected)


def session():
    with open(SESSION) as f:
        return json.load(f)


def cur(j):
    """The space that was showing when the session was written."""
    return j["spaces"][j.get("current", 0)]


def session_tabs(j):
    return [(space["name"], space["profile"], space.get("color", 0),
             [(t["url"], t["title"], t.get("pinned", False), t.get("name")) for t in space["tabs"]])
            for space in j["spaces"]]


def wait_session(predicate, what, timeout=10):
    """The TUI writes the session a moment after each change."""
    end = time.time() + timeout
    while time.time() < end:
        try:
            if predicate(session()):
                return session()
        except (OSError, ValueError, KeyError, IndexError):
            pass
        time.sleep(0.2)
    sys.exit("TIMEOUT waiting for session: " + what)


def screen(d):
    """The text the TUI has on screen."""
    return d.call("debug.screen").get("text", "")


def wait_screen(d, predicate, what, timeout=15):
    end = time.time() + timeout
    while time.time() < end:
        text = screen(d)
        if predicate(text):
            return text
        time.sleep(0.2)
    print(screen(d))
    sys.exit("TIMEOUT waiting for the screen: " + what)


class DevToolsSite(http.server.BaseHTTPRequestHandler):
    """A page that makes requests and stores things, for the dev tools to show."""
    PAGE = b"""<!doctype html><title>devtools</title><body>dev tools test<img src="/pixel.png"><script>
document.cookie = 'e2e_cookie=chocolate; path=/';
localStorage.setItem('e2e_key', JSON.stringify({a: 1, list: [1, 2]}));
localStorage.setItem('e2e_other', 'stays');
sessionStorage.setItem('e2e_session', 'yes');
indexedDB.open('e2e_db', 1).onupgradeneeded = (e) => e.target.result.createObjectStore('things', {keyPath: 'id'}).add({id: 1, name: 'first'});
fetch('/api/echo?x=1', {method: 'POST', headers: {'Content-Type': 'application/json', 'X-Test': 'yes'}, body: JSON.stringify({hello: 'world'})})
  .then((r) => r.json()).then((j) => { window.echoed = j.seen; });
const x = new XMLHttpRequest(); x.open('GET', '/api/data?n=3'); x.send();
fetch('/missing');
// As libraries do: a reference to fetch taken while the page loads.
window.keptFetch = window.fetch.bind(window);
const ws = new WebSocket('ws://' + location.host + '/ws');
ws.onopen = () => ws.send('ping-e2e');
ws.onmessage = (e) => { window.wsGot = e.data + (ws instanceof WebSocket ? '' : ' (not a WebSocket)'); };
</script>"""
    # WebSockets need HTTP/1.1.
    protocol_version = "HTTP/1.1"
    # The smallest PNG there is: one transparent pixel.
    PIXEL = bytes.fromhex("89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c489"
                          "0000000d49444154789c6300010000000500010d0a2db40000000049454e44ae426082")

    def log_message(self, *args):
        pass

    def reply(self, code, kind, body, headers=()):
        self.send_response(code)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        for name, value in headers:
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/":
            self.reply(200, "text/html; charset=utf-8", self.PAGE, [("X-Served-By", "e2e")])
        elif self.path == "/ws":
            self.websocket()
        elif self.path == "/pixel.png":
            self.reply(200, "image/png", self.PIXEL)
        elif self.path.startswith("/api/data"):
            self.reply(200, "application/json", b'{"items":[1,2,3],"from":"xhr"}')
        else:
            self.reply(404, "text/plain", b"nothing here")

    def websocket(self):
        """Accepts the connection, sends back the first message, and waits for the end."""
        key = self.headers.get("Sec-WebSocket-Key", "") + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
        self.send_response(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", base64.b64encode(hashlib.sha1(key.encode()).digest()).decode())
        self.end_headers()
        head = self.rfile.read(2)
        mask = self.rfile.read(4)
        text = bytes(b ^ mask[i % 4] for i, b in enumerate(self.rfile.read(head[1] & 0x7F)))
        self.wfile.write(bytes([0x81, len(text)]) + text)
        self.wfile.flush()
        self.close_connection = True
        self.rfile.read(2)

    def do_POST(self):
        sent = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        answer = json.dumps({"echo": json.loads(sent or b"null"), "seen": self.headers.get("X-Test")}).encode()
        self.reply(200, "application/json", answer, [("Set-Cookie", "http_only_cookie=secret; HttpOnly; Path=/")])


def devtools_checks(d):
    """The dev tools pane, against a page served from this script."""
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), DevToolsSite)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    port = server.server_address[1]
    count = len(state(d)["tabs"])
    open_tab(d, f"http://127.0.0.1:{port}/")
    wait(d, lambda s: len(s["tabs"]) == count + 1 and active(s)["title"] == "devtools" and not active(s)["loading"], "dev tools test page")
    for _ in range(50):
        if js(d, "String(window.echoed)") == "yes":
            break
        time.sleep(0.2)
    nav(d)
    before = state(d)["viewport"]

    typ(d, "I")
    s = wait(d, lambda s: s["viewport"]["cols"] < before["cols"], "the page making room for the pane")
    check("I opens dev tools beside the page, which stays on screen", frame_matches_cells(s) and s["web_hidden"] is False, str(s["web_frame"]))
    text = wait_screen(d, lambda t: "pixel.png" in t and " DEV " in t, "requests the page had already made")
    check("requests made before dev tools were opened are listed from WebKit's timings", "Network" in text and "Storage" in text)

    check("a page loaded before dev tools opened says that a reload records it in full", "Loaded before dev tools opened" in text, text[:900])
    native = "String(Function.prototype.toString.call(window.fetch).includes('[native code]'))"
    check("fetch is only wrapped while dev tools are open", js(d, native) == "false", str(js(d, native)))

    typ(d, "r")
    text = wait_screen(d, lambda t: "echo?x=1" in t and "data?n=3" in t and "missing" in t and "pixel.png" in t, "requests after a reload")
    # The pane is to the right of the last rule on each line.
    panes = [line.split("│")[-1] for line in text.splitlines() if line.count("│") > 1]
    row = lambda name: next((line for line in panes if name in line), "")
    check("a reload lists the document, with its status", "200" in row(f"127.0.0.1:{port}/"), row(f"127.0.0.1:{port}/"))
    check("a fetch is listed with its method and status", "200" in row("echo?x=1") and "POST" in row("echo?x=1"), row("echo?x=1"))
    check("an XMLHttpRequest is listed", "200" in row("data?n=3"), row("data?n=3"))
    check("a failed request shows its status", "404" in row("missing"), row("missing"))
    check("after the reload the notice is gone", "Loaded before dev tools opened" not in text)
    check("a WebSocket is listed, and still works for the page", "101" in row("ws") and js(d, "String(window.wsGot)") == "ping-e2e",
          row("ws") + " / " + str(js(d, "String(window.wsGot)")))

    typ(d, "/echo\r")
    typ(d, "\r")
    text = wait_screen(d, lambda t: "x-test" in t and "General" in t, "the request's headers")
    check("a request's details show the headers its script set and the response's",
          "application/json" in text and "Response headers" in text and "Request headers" in text, text[-600:])
    typ(d, "l")
    text = wait_screen(d, lambda t: "Query parameters" in t and '"hello"' in t, "the request's payload")
    check("the payload shows the query and the body that was sent", '"world"' in text)
    typ(d, "l")
    text = wait_screen(d, lambda t: '"seen"' in t and '"echo"' in t, "the response body")
    check("the response body is shown as the server sent it", '"yes"' in text)
    typ(d, "\x1b"); typ(d, "\x1b")
    # As libraries do, the page calls fetch through the reference it took while loading.
    js(d, "keptFetch('/api/echo?kept=1', {method: 'POST', body: JSON.stringify({late: 'call'})}), 1")
    wait_screen(d, lambda t: "echo?kept=1" in t, "a request through the kept reference")
    # The details are still on the response, where the last request left them.
    typ(d, "/kept\r"); typ(d, "\r")
    text = wait_screen(d, lambda t: '"echo"' in t or "Only the timing" in t, "that request's response")
    check("a fetch through a reference the page kept is recorded with its body", '"late"' in text and '"call"' in text, text[-500:])
    typ(d, "\x1b"); typ(d, "\x1b")
    typ(d, "/ws\r"); typ(d, "\r")
    text = wait_screen(d, lambda t: "Messages (2)" in t, "the socket's messages")
    check("a WebSocket's messages are shown, sent and received", "↑ ping-e2e" in text and "↓ ping-e2e" in text, text[-500:])
    typ(d, "\x1b"); typ(d, "\x1b")

    typ(d, "\t")
    text = wait_screen(d, lambda t: "e2e_cookie" in t and "http_only_cookie" in t, "the site's cookies")
    check("storage lists the site's cookies, including one scripts cannot read", "chocolate" in text and "secret" in text)
    typ(d, "j"); typ(d, "\r")
    text = wait_screen(d, lambda t: "HttpOnly" in t and "SameSite" in t, "a cookie's details")
    check("a cookie's details say it is HttpOnly", any("HttpOnly" in line and "Yes" in line for line in text.splitlines()))
    typ(d, "\x1b")
    typ(d, "l")
    wait_screen(d, lambda t: "e2e_key" in t and "e2e_other" in t, "local storage")
    typ(d, "/e2e_key\r"); typ(d, "x")
    for _ in range(25):
        if js(d, "String(localStorage.getItem('e2e_key'))") == "null":
            break
        time.sleep(0.2)
    check("x deletes a local storage key and leaves the others",
          js(d, "String(localStorage.getItem('e2e_key'))") == "null" and js(d, "localStorage.getItem('e2e_other')") == "stays")
    typ(d, "\x1b")
    wait_screen(d, lambda t: "e2e_other" in t and "e2e_key" not in t, "the list without the deleted key")
    typ(d, "l")
    text = wait_screen(d, lambda t: "e2e_session" in t, "session storage")
    typ(d, "l")
    text = wait_screen(d, lambda t: "e2e_db" in t and "things" in t, "IndexedDB")
    check("session storage and IndexedDB of the page are listed", "1 record" in text, text[-400:])

    typ(d, "q")
    s = wait(d, lambda s: s["viewport"] == before, "the page taking its room back")
    check("q closes dev tools and the page takes the room back", frame_matches_cells(s))
    check("closing dev tools gives the page its own fetch back", js(d, native) == "true", str(js(d, native)))
    typ(d, "x")
    wait(d, lambda s: len(s["tabs"]) == count, "the dev tools test tab closed")
    server.shutdown()


def reach_checks(d):
    """Scrolling and following links from NAV, the palette, dialogs, blocking, sleep."""
    check("pages are told the Safari version of this Mac", state(d)["user_agent"] in (js(d, "navigator.userAgent") or ""))

    open_tab(d, "data:text/html,<title>reach</title><body style='margin:0'><a id=l href='https://example.org/' style='display:block;font-size:30px'>link</a>"
                "<button id=b onclick=\"document.title='pressed'\" style='font-size:30px'>press</button><input id=i style='font-size:30px'>"
                "<div style='height:6000px'></div>")
    wait(d, lambda s: active(s)["title"] == "reach" and s["mode"] == "page", "reach page")
    nav(d)
    reach_tab = state(d)["active"]

    def scrolled(test, what):
        end = time.time() + 5
        while time.time() < end:
            if test(float(js(d, "scrollY") or 0)):
                return True
            time.sleep(0.2)
        return False
    typ(d, "G")
    down = scrolled(lambda y: y > 3000, "bottom")
    typ(d, "g")
    up = scrolled(lambda y: y == 0, "top")
    typ(d, "\x04")
    half = scrolled(lambda y: 100 < y < 3000, "half a page")
    check("G, g and Ctrl+D scroll the page from NAV mode", down and up and half and state(d)["mode"] == "nav",
          f"{down} {up} {half}")
    typ(d, "g"); scrolled(lambda y: y == 0, "top")

    # Hints: the link is S, the button A, the field D.
    typ(d, "f")
    text = wait_screen(d, lambda t: "HINT" in t and "3 labels" in t, "labels on the page")
    check("f labels what can be followed", "3 labels" in text)
    typ(d, "a")
    wait(d, lambda s: active(s)["title"] == "pressed", "the button was pressed")
    check("typing a label presses its button, and the keys stay with the tabs", state(d)["mode"] == "nav")
    typ(d, "f"); wait_screen(d, lambda t: "3 labels" in t, "labels again"); typ(d, "d")
    wait(d, lambda s: s["mode"] == "page", "a field takes the keys")
    check("a field's label gives the page the keys, in the field", js(d, "document.activeElement.id") == "i")
    nav(d)
    count = len(state(d)["tabs"])
    typ(d, "F"); wait_screen(d, lambda t: "3 labels" in t, "labels for a new tab"); typ(d, "s")
    s = wait(d, lambda s: len(s["tabs"]) == count + 1, "a link opened behind")
    check("F opens a link in a tab behind this one", s["active"] == reach_tab)
    typ(d, "j"); wait(d, lambda s: s["active"] != reach_tab, "the tab behind"); typ(d, "x")
    # Closing a tab shows the one after it, when there is one.
    if wait(d, lambda s: len(s["tabs"]) == count and s.get("active"), "the tab behind closed")["active"] != reach_tab:
        typ(d, "k")
    wait(d, lambda s: s["active"] == reach_tab, "back on the reach tab")

    # The palette is drawn over a still of the page.
    key(d, "P", "cmd+shift", 35)
    s = wait(d, lambda s: s["overlay"] and s["covered"] and s["web_hidden"], "palette over the page")
    check("the palette opens over a still of the page", "Find a tab" in screen(d))
    typ(d, "\x1b")
    s = wait(d, lambda s: not s["overlay"] and not s["covered"] and s["web_hidden"] is False, "the page again")
    check("closing the palette puts the page back", True)

    # A page's dialogs are shown and answered.
    js(d, "setTimeout(() => { window.__sure = confirm('sure?') }, 0); 1")
    wait(d, lambda s: s["sheet"], "the page's confirm is shown")
    d.call("debug.sheet", accept=True)
    wait(d, lambda s: not s["sheet"], "the sheet is gone")
    time.sleep(0.3)
    check("confirm() is asked and its answer reaches the page", js(d, "String(window.__sure)") == "true")

    # Sound.
    typ(d, "m")
    wait(d, lambda s: active(s)["muted"], "muted")
    typ(d, "m")
    wait(d, lambda s: not active(s)["muted"], "unmuted")
    check("m mutes and unmutes the tab", True)

    # A closed tab comes back with its pages back and forward.
    js(d, "location.hash = 'two'; 1")
    wait(d, lambda s: active(s)["can_go_back"], "a second page in the tab")
    typ(d, "x")
    wait(d, lambda s: len(s["tabs"]) == count - 1, "the reach tab closed")
    typ(d, "u")
    s = wait(d, lambda s: len(s["tabs"]) == count and active(s)["url"].endswith("#two") and active(s)["can_go_back"],
             "the closed tab reopened with its history")
    check("u reopens the closed tab with its pages back and forward", True)

    # Requests to advertising networks from another site's page are blocked.
    typ(d, "o")
    typ(d, "data:text/html,<title>ads</title><script src='https://www.googletagmanager.com/gtag/js?id=G-TILDE' "
           "onerror=\"document.title='blocked'\" onload=\"document.title='loaded'\"></script>")
    typ(d, "\r")
    s = wait(d, lambda s: active(s)["title"] in ("blocked", "loaded"), "the tracker's script was decided")
    check("a third-party request to a tracker is blocked", active(s)["title"] == "blocked")
    nav(d)

    # The icons of sites are laid beside the tabs.
    s = wait(d, lambda s: s["icon_cells"] > 0, "icon cells")
    try:
        s = wait(d, lambda s: s["icons_shown"] > 0, "an icon", timeout=15)
    except SystemExit:
        pass
    check("the sidebar shows the icons of sites", state(d)["icons_shown"] > 0, str(state(d)["icons_shown"]))

    # Sleep: the other tabs' pages are let go, and come back when shown.
    loaded = len(state(d)["tabs"])
    listed = sum(len(space["tabs"]) for space in wait_session(lambda j: True, "session")["spaces"])
    key(d, "P", "cmd+shift", 35)
    wait(d, lambda s: s["overlay"], "palette")
    typ(d, ">sleep"); typ(d, "\r")
    s = wait(d, lambda s: len(s["tabs"]) < loaded and not s["overlay"], "other tabs asleep")
    check("the other tabs are put to sleep, and the tabs stay in the sidebar",
          sum(len(space["tabs"]) for space in wait_session(lambda j: True, "session")["spaces"]) == listed,
          f"{len(s['tabs'])} of {loaded} loaded")
    asleep = len(s["tabs"])
    typ(d, "k")
    s = wait(d, lambda s: len(s["tabs"]) == asleep + 1 and active(s)["url"] and not active(s)["loading"], "a sleeping tab wakes")
    check("a sleeping tab wakes when it is shown", True)
    typ(d, "j")
    wait(d, lambda s: active(s)["title"] in ("blocked", "loaded", "ads"), "back on the last tab")
    typ(d, "x"); time.sleep(0.5)


def open_tab(d, text):
    key(d, "t", "cmd", 17)
    wait(d, lambda s: s["mode"] == "nav" and s.get("web_hidden") is True, "new tab bar")
    typ(d, text)
    typ(d, "\r")


def main():
    global tui_pid
    if len(DEBUG_SOCK) > 100:
        sys.exit("temporary directory path too long for a Unix socket: " + WORK)
    if not os.path.isdir(SOURCE_APP):
        sys.exit("build/Tilde.app not found: run scripts/bundle.sh first")
    prepare_app()

    d = launch()
    s = wait(d, lambda s: s.get("active") and not active(s)["loading"], "home page")
    check("first launch opens one tab in NAV mode", len(s["tabs"]) == 1 and s["mode"] == "nav")
    check("page frame equals its cell rectangle", frame_matches_cells(s), str(s["web_frame"]))

    # URL bar: host, then search
    typ(d, "t"); typ(d, "example.com"); typ(d, "\r")
    s = wait(d, lambda s: len(s["tabs"]) == 2 and active(s)["url"].startswith("https://example.com") and not active(s)["loading"], "example.com")
    s = wait(d, lambda s: s["mode"] == "page", "PAGE after submitting a URL")
    check("typing a host opens it and moves to PAGE", s["first_responder"] == "WKWebView")
    key(d, "l", "cmd", 37)
    wait(d, lambda s: s["mode"] == "nav", "NAV after Cmd+L")
    typ(d, "swift concurrency"); typ(d, "\r")
    wait(d, lambda s: "duckduckgo.com/?q=swift%20concurrency" in active(s)["url"], "search")
    check("Cmd+L from the page, then free text, searches", True)

    # Modes
    wait(d, lambda s: s["mode"] == "page", "PAGE")
    key(d, ".", "cmd", 47)
    s = wait(d, lambda s: s["mode"] == "nav", "NAV after Cmd+.")
    check("Cmd+. returns to NAV", "Terminal" in s["first_responder"])
    typ(d, "\r")
    wait(d, lambda s: s["mode"] == "page", "PAGE after Enter")
    if BACKGROUND:
        skip("clicking the TUI reports NAV", "needs focus")
        skip("clicking the page reports PAGE", "needs focus")
    else:
        d.call("debug.click", target="cell", col=10, row=20); time.sleep(0.6)
        check("clicking the TUI reports NAV", state(d)["mode"] == "nav")
        d.call("debug.click", target="page", x=400, y=500); time.sleep(0.6)
        check("clicking the page reports PAGE", state(d)["mode"] == "page")
    nav(d)

    # target=_blank, tab cycling, typing into a page
    typ(d, "o")
    typ(d, "data:text/html,<title>form</title><input id=i style='font-size:30px'><a id=a href='https://example.org/' target=_blank style='display:block;font-size:40px;margin-top:40px'>open</a>")
    typ(d, "\r")
    wait(d, lambda s: active(s)["title"] == "form" and s["mode"] == "page", "form page")
    count = len(state(d)["tabs"])
    if BACKGROUND:
        skip("a target=_blank link opens and shows a new tab", "needs focus")
        open_tab(d, "example.org")
    else:
        rect = json.loads(js(d, "JSON.stringify(document.getElementById('a').getBoundingClientRect())"))
        d.call("debug.click", target="page", x=int(rect["x"] + 30), y=int(rect["y"] + rect["height"] / 2))
    wait(d, lambda s: len(s["tabs"]) == count + 1 and "example.org" in active(s)["url"] and s["mode"] == "page", "example.org tab")
    if not BACKGROUND:
        check("a target=_blank link opens and shows a new tab", True)
    if BACKGROUND:
        # Without a key window AppKit only delivers menu shortcuts.
        skip("Ctrl+Tab and Ctrl+Shift+Tab switch tabs from the page", "needs focus")
    else:
        key(d, "\x19", "ctrl+shift", 48)
        wait(d, lambda s: active(s)["title"] == "form", "Ctrl+Shift+Tab")
        key(d, "\t", "ctrl", 48)
        wait(d, lambda s: "example.org" in active(s)["url"], "Ctrl+Tab")
        check("Ctrl+Tab and Ctrl+Shift+Tab switch tabs from the page", state(d)["mode"] == "page")
    key(d, "1", "cmd", 18)
    wait(d, lambda s: "webkit.org" in active(s)["url"], "Cmd+1")
    key(d, "2", "cmd", 19)
    wait(d, lambda s: active(s)["title"] == "form", "Cmd+2")
    check("Cmd+digit switches tabs from the page", state(d)["mode"] == "page")
    if BACKGROUND:
        skip("typing reaches a text field in the page", "needs focus")
    else:
        js(d, "document.getElementById('i').focus()")
        key(d, "h", "", 4); key(d, "i", "", 34)
        check("typing reaches a text field in the page", js(d, "document.getElementById('i').value") == "hi")

    # A page that swallows every key cannot block browser shortcuts
    key(d, "l", "cmd", 37); wait(d, lambda s: s["mode"] == "nav", "NAV")
    typ(d, "data:text/html,<title>hostile</title><script>for (const t of ['keydown','keypress','keyup']) addEventListener(t, e => { e.preventDefault(); e.stopImmediatePropagation() }, true)</script>hostile")
    typ(d, "\r")
    wait(d, lambda s: active(s)["title"] == "hostile" and s["mode"] == "page", "hostile page")
    count = len(state(d)["tabs"])
    key(d, "t", "cmd", 17)
    wait(d, lambda s: s["mode"] == "nav" and s.get("web_hidden") is True, "Cmd+T on hostile page")
    typ(d, "\x1b")
    wait(d, lambda s: s.get("web_hidden") is False, "back on hostile page")
    typ(d, "\r"); wait(d, lambda s: s["mode"] == "page", "PAGE")
    key(d, "l", "cmd", 37)
    wait(d, lambda s: s["mode"] == "nav", "Cmd+L on hostile page")
    typ(d, "\x1b"); typ(d, "\r"); wait(d, lambda s: s["mode"] == "page", "PAGE")
    key(d, "w", "cmd", 13)
    wait(d, lambda s: len(s["tabs"]) == count - 1 and all(t["title"] != "hostile" for t in s["tabs"]), "Cmd+W on hostile page")
    check("Cmd+T, Cmd+L and Cmd+W work on a page that blocks all keys", True)

    # A page that focuses itself must not take the keyboard from the TUI
    nav(d)
    typ(d, "o"); typ(d, "data:text/html,<title>grabby</title><input id=i><script>setInterval(() => document.getElementById('i').focus(), 100)</script>"); typ(d, "\r")
    wait(d, lambda s: active(s)["title"] == "grabby" and s["mode"] == "page", "grabby page")
    nav(d); time.sleep(1)
    check("a page calling focus() cannot leave NAV mode", state(d)["mode"] == "nav")
    typ(d, "x"); time.sleep(0.3)

    # Sidebar toggle
    typ(d, "\r"); wait(d, lambda s: s["mode"] == "page", "PAGE")
    key(d, "b", "cmd", 11)
    s = wait(d, lambda s: s["viewport"]["col"] == 0, "Cmd+B hides the sidebar")
    time.sleep(0.3); s = state(d)
    check("Cmd+B hides the sidebar and the page takes the full width",
          frame_matches_cells(s) and s["web_frame"]["x"] == 0 and s["viewport"]["cols"] == s["grid"]["cols"] and s["mode"] == "page")
    key(d, "b", "cmd", 11)
    s = wait(d, lambda s: s["viewport"]["col"] == 28, "Cmd+B shows the sidebar")
    time.sleep(0.3)
    check("Cmd+B shows it again", frame_matches_cells(state(d)))

    # Copying the address out of the URL field
    key(d, "l", "cmd", 37); wait(d, lambda s: s["mode"] == "nav", "NAV")
    editing = active(state(d))["url"]
    key(d, "c", "cmd", 8); time.sleep(0.3)
    check("Cmd+L then Cmd+C copies the address", subprocess.run(["pbpaste"], capture_output=True, text=True).stdout == editing)
    typ(d, "\x1b"); typ(d, "\r"); wait(d, lambda s: s["mode"] == "page", "PAGE")

    # A download: a link with the download attribute, clicked by the page itself
    js(d, "var a = document.createElement('a'); a.href = 'data:application/octet-stream;base64,aGVsbG8gdGlsZGU='; "
          "a.download = 'tilde-e2e.txt'; document.body.appendChild(a); a.click(); 1")
    s = wait(d, lambda s: any(x["state"] == "done" for x in s["downloads"]), "download finished")
    saved = os.path.join(DOWNLOADS, "tilde-e2e.txt")
    check("a download is saved and listed",
          os.path.exists(saved) and open(saved).read() == "hello tilde" and s["downloads"][0]["filename"] == "tilde-e2e.txt"
          and s["downloads"][0]["received"] == 11)
    tabs = len(s["tabs"])
    key(d, "l", "cmd+alt", 37)
    wait(d, lambda s: s["mode"] == "nav" and s.get("web_hidden") is True, "Opt+Cmd+L shows the downloads")
    typ(d, "x")
    wait(d, lambda s: not s["downloads"], "x takes the download off the list")
    check("removing a download from the list keeps the file", os.path.exists(saved))
    key(d, "l", "cmd+alt", 37)
    wait(d, lambda s: s["mode"] == "page" and s.get("web_hidden") is False, "Opt+Cmd+L closes the downloads")

    # A search is one visit, however often the results page rewrites its address
    key(d, "l", "cmd", 37); wait(d, lambda s: s["mode"] == "nav", "NAV")
    typ(d, "tilde e2e search"); typ(d, "\r")
    wait(d, lambda s: "duckduckgo.com" in active(s)["url"] and not active(s)["loading"] and s["mode"] == "page", "search results")
    time.sleep(3)
    searches = [v["url"] for v in json.load(open(os.path.join(WORK, "history.json"))) if "duckduckgo.com" in v["url"]]
    # One line for this search and one for the search made earlier in this run.
    check("a search from the address bar is one line of history",
          sum("tilde+e2e+search" in u for u in searches) == 1 and len(searches) == len({u.split("&")[0] for u in searches}), str(searches))
    key(d, "[", "cmd", 33)
    wait(d, lambda s: "duckduckgo.com" not in active(s)["url"], "back from the search")

    # History: the view opens a visited page in a new tab
    key(d, "y", "cmd", 16)
    wait(d, lambda s: s["mode"] == "nav" and s.get("web_hidden") is True, "Cmd+Y shows the history")
    history = json.load(open(os.path.join(WORK, "history.json"))) if os.path.exists(os.path.join(WORK, "history.json")) else []
    typ(d, "\r")
    s = wait(d, lambda s: len(s["tabs"]) == tabs + 1 and s["mode"] == "page", "a page opened from the history")
    check("history records visits and opens one in a new tab",
          len(history) >= 2 and all(v.get("space") == "Home" for v in history) and active(s)["url"] == history[-1]["url"])
    key(d, "w", "cmd", 13)
    wait(d, lambda s: len(s["tabs"]) == tabs, "the history tab closed")
    typ(d, "\r"); wait(d, lambda s: s["mode"] == "page", "PAGE")

    # A link clicked in another app opens in a new tab and shows it
    tabs = len(state(d)["tabs"])
    d.call("debug.open_url", url="https://example.org/?from=another-app")
    s = wait(d, lambda s: len(s["tabs"]) == tabs + 1 and "from=another-app" in active(s)["url"] and s["mode"] == "page",
             "a link from another app")
    check("a link from another app opens in a new tab", frame_matches_cells(s) and s.get("web_hidden") is False)
    key(d, "w", "cmd", 13)
    wait(d, lambda s: len(s["tabs"]) == tabs, "the link's tab closed")
    typ(d, "\r"); wait(d, lambda s: s["mode"] == "page", "PAGE")

    # Page zoom
    check("pinch to zoom is enabled on the page", state(d).get("allows_magnification") is True)
    key(d, "=", "cmd", 24)
    wait(d, lambda s: s["page_zoom"] > 1, "Cmd+= zooms in")
    key(d, "+", "cmd+shift", 24)
    larger = wait(d, lambda s: s["page_zoom"] > 1.2, "Cmd+Plus zooms in")["page_zoom"]
    key(d, "-", "cmd", 27)
    wait(d, lambda s: 1 < s["page_zoom"] < larger, "Cmd+Minus zooms out")
    key(d, "-", "cmd", 27); key(d, "-", "cmd", 27)
    wait(d, lambda s: s["page_zoom"] < 1, "Cmd+Minus zooms below actual size")
    key(d, "0", "cmd", 29)
    s = wait(d, lambda s: s["page_zoom"] == 1 and s["magnification"] == 1, "Cmd+0 restores actual size")
    check("Cmd+Plus, Cmd+Minus and Cmd+0 zoom the page and leave it in place",
          frame_matches_cells(s) and s["mode"] == "page")

    # Pin, browse away, return to the pinned page
    key(d, "1", "cmd", 18)
    s = wait(d, lambda s: "webkit.org" in active(s)["url"], "first tab")
    pinned_url = active(s)["url"]
    key(d, "d", "cmd", 2)
    saved = wait_session(lambda j: cur(j)["tabs"][0].get("pinned") and cur(j)["tabs"][0].get("pin_url") == pinned_url, "pin saved")
    check("Cmd+D pins the tab and the pin is saved", sum(1 for t in cur(saved)["tabs"] if t.get("pinned")) == 1)
    key(d, "l", "cmd", 37); wait(d, lambda s: s["mode"] == "nav", "NAV")
    typ(d, "example.com/elsewhere"); typ(d, "\r")
    wait(d, lambda s: "example.com/elsewhere" in active(s)["url"] and s["mode"] == "page", "browsed away from the pin")
    key(d, "H", "cmd+shift", 4)
    wait(d, lambda s: active(s)["url"] == pinned_url, "Cmd+Shift+H")
    check("Cmd+Shift+H returns a pinned tab to its pinned page", state(d)["mode"] == "page")

    # Rename
    nav(d)
    typ(d, "n"); typ(d, "Docs"); typ(d, "\r")
    wait_session(lambda j: cur(j)["tabs"][0].get("name") == "Docs", "name saved")
    check("n renames a tab and the name is saved", True)

    # Key reference
    typ(d, "\r"); wait(d, lambda s: s["mode"] == "page", "PAGE")
    key(d, "/", "cmd", 44)
    wait(d, lambda s: s.get("web_hidden") is True and s["mode"] == "nav", "Cmd+/ shows the key reference")
    typ(d, " ")
    s = wait(d, lambda s: s.get("web_hidden") is False and s["mode"] == "page", "a key closes the key reference")
    check("Cmd+/ shows the key reference; a key returns to the page", active(s)["url"] == pinned_url)
    nav(d)
    typ(d, "?")
    wait(d, lambda s: s.get("web_hidden") is True, "? shows the key reference")
    typ(d, "q")
    s = wait(d, lambda s: s.get("web_hidden") is False, "a key closes it")
    check("? works in NAV mode, and the closing key is not acted on", s["tui_running"] and s["mode"] == "nav")

    # Spaces: own tabs, own profile
    js(d, "localStorage.setItem('tilde-e2e', 'home'); 1")
    home_tabs = len(state(d)["tabs"])
    typ(d, "N")
    wait(d, lambda s: s.get("web_hidden") is True, "space editor")
    typ(d, "E2E Space"); typ(d, "\t"); typ(d, "\x1b[C"); typ(d, "\r")
    saved = wait_session(lambda j: len(j["spaces"]) == 2 and j.get("current") == 1, "new space saved")
    check("N creates a space with its own profile and the chosen colour",
          (saved["spaces"][1]["name"], saved["spaces"][1]["profile"], saved["spaces"][1]["color"]) == ("E2E Space", "e2e-space", 2), str(saved["spaces"][1]))
    typ(d, pinned_url); typ(d, "\r")
    s = wait(d, lambda s: active(s)["url"] == pinned_url and s["mode"] == "page" and not active(s)["loading"], "same site in the new space")
    space_tab = s["active"]
    check("the other space's tabs stay loaded", len(s["tabs"]) == home_tabs + 1)
    check("a space with its own profile does not see the first space's site data",
          js(d, "localStorage.getItem('tilde-e2e')") != "home")
    js(d, "localStorage.setItem('tilde-e2e', 'space'); window.__mark = 'space page'; 1")
    key(d, "\uf702", "cmd+alt", 123)
    s = wait(d, lambda s: s["active"] != space_tab and active(s)["url"] == pinned_url, "Opt+Cmd+Left")
    check("Opt+Cmd+Left switches space and stays in PAGE", s["mode"] == "page" and js(d, "localStorage.getItem('tilde-e2e')") == "home")
    key(d, "2", "ctrl", 19)
    wait(d, lambda s: s["active"] == space_tab, "Ctrl+2")
    check("Ctrl+2 returns to the space without reloading its page",
          js(d, "window.__mark") == "space page" and js(d, "localStorage.getItem('tilde-e2e')") == "space")
    js(d, "localStorage.removeItem('tilde-e2e'); 1")
    key(d, "\uf703", "cmd+alt", 124)
    wait(d, lambda s: s["active"] != space_tab, "Opt+Cmd+Right")
    js(d, "localStorage.removeItem('tilde-e2e'); 1")
    wait_session(lambda j: j.get("current", 0) == 0, "back in the first space")

    # Ten tabs, no reload when switching
    nav(d)
    have = len(state(d)["tabs"]) - 1  # not counting the other space's tab
    sites = ["example.net", "apple.com", "wikipedia.org", "nonexistent-host-tilde-test.invalid", "swift.org", "rust-lang.org", "duckduckgo.com", "example.com", "example.org"]
    for site in sites[: 10 - have]:
        open_tab(d, site)
        wait(d, lambda s: s["mode"] == "page", "PAGE after opening " + site)
    s = wait(d, lambda s: len(s["tabs"]) == 11 and all(not t["loading"] for t in s["tabs"]), "ten tabs loaded", 90)
    failed = [t for t in s["tabs"] if "nonexistent-host" in t["url"]]
    check("a failed load keeps the address it failed on", len(failed) == 1 and failed[0]["title"] == "Can't open page")
    nav(d)
    for _ in range(10):
        typ(d, "j")
    for _ in range(10):
        js(d, f"window.__mark = 'm{state(d)['active']}'; 1")
        typ(d, "k")
    for _ in range(10):
        typ(d, "j")
    seen, intact = set(), True
    for _ in range(10):
        s = state(d)
        seen.add(s["active"])
        intact = intact and js(d, "window.__mark || 'none'") == f"m{s['active']}"
        typ(d, "k")
    check("ten tabs switch with j/k without reloading", intact and len(seen) == 10)
    typ(d, "J"); typ(d, "J")

    # Resize
    d.call("debug.resize", cols=110, rows=34); time.sleep(1)
    s = state(d)
    check("after a resize the page matches its new cell rectangle",
          frame_matches_cells(s) and s["viewport"]["cols"] == 110 - 28 and s["viewport"]["rows"] == 34 - 2, str(s["viewport"]))
    d.call("debug.resize", cols=150, rows=46); time.sleep(1)
    check("and again after resizing back", frame_matches_cells(state(d)))

    # TUI crash
    tabs_before = [(t["tab_id"], t["url"]) for t in state(d)["tabs"]]
    active_before = state(d)["active"]
    js(d, "window.__mark = 'survivor'; 1")
    os.kill(state(d)["tui_pid"], signal.SIGKILL)
    s = wait(d, lambda s: s["awaiting_restart"] and not s["tui_running"], "restart prompt")
    check("kill -9 of tilde-tui shows the restart prompt and hides the page", s["web_hidden"] is True)
    typ(d, " ")
    wait(d, lambda s: s["tui_running"] and s["tui_connected"] and s.get("web_hidden") is False, "TUI restarted")
    time.sleep(1)
    tui_pid = state(d)["tui_pid"]
    s = state(d)
    check("after restart the same tabs are present, not recreated",
          [(t["tab_id"], t["url"]) for t in s["tabs"]] == tabs_before and s["active"] == active_before)
    check("the active page was not reloaded", js(d, "window.__mark") == "survivor")

    # Web content process crash
    d.call("debug.kill_web"); time.sleep(1.5)
    typ(d, "r")
    wait(d, lambda s: not active(s)["loading"] and active(s)["title"], "reload after web process crash")
    check("r reloads a tab whose web process crashed", js(d, "window.__mark || 'fresh'") == "fresh")

    # Video (muted, so the test is silent; sound has to be checked by ear)
    open_tab(d, "data:text/html,<title>video</title><video id=v muted playsinline autoplay loop width=600 src='https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4'></video>")
    wait(d, lambda s: active(s)["title"] == "video", "video page")
    playing = False
    for _ in range(40):
        t = js(d, "document.getElementById('v').currentTime")
        if t and float(t) > 1.0:
            playing = True
            break
        time.sleep(0.5)
    check("a video plays", playing)
    nav(d); typ(d, "x"); time.sleep(0.5)

    # Dev tools: network and storage
    devtools_checks(d)

    # The keyboard's reach into the page, the palette, dialogs, blocking, sleep
    reach_checks(d)

    # Quit and restore
    time.sleep(1)
    typ(d, "\r"); wait(d, lambda s: s["mode"] == "page", "PAGE")
    key(d, "q", "cmd", 12)
    check("Cmd+Q leaves no Tilde or tilde-tui process", wait_gone(), str(processes()))
    before = session()
    check("the session has ten tabs in the first space and one in the second",
          [len(space["tabs"]) for space in before["spaces"]] == [10, 1] and before.get("current", 0) == 0)

    d = launch()
    s = wait(d, lambda s: s.get("active") and s["tui_connected"] and active(s)["url"], "relaunch")
    time.sleep(1.5)
    s, after = state(d), session()
    check("relaunch restores spaces, tabs and order", session_tabs(before) == session_tabs(after))
    check("relaunch restores the active tab",
          cur(before)["active"] == cur(after)["active"] and active(s)["url"] == cur(before)["tabs"][cur(before)["active"]]["url"])
    check("only the active tab is loaded at launch", len(s["tabs"]) == 1)
    first = cur(after)["tabs"][0]
    check("the pin and its name survive the relaunch",
          first.get("pinned") is True and first.get("name") == "Docs" and "webkit.org" in first.get("pin_url", ""))
    typ(d, "l")
    s = wait(d, lambda s: len(s["tabs"]) == 2, "second space loads its tab on first visit")
    typ(d, "D"); wait(d, lambda s: s.get("web_hidden") is True, "delete question"); typ(d, "y")
    wait(d, lambda s: len(s["tabs"]) == 1, "the deleted space's tab is closed")
    check("D then y deletes a space and closes its tabs", len(wait_session(lambda j: len(j["spaces"]) == 1, "one space left")["spaces"]) == 1)
    typ(d, "q")
    check("q quits everything", wait_gone(), str(processes()))

    print()
    if skipped:
        print(f"{len(skipped)} skipped (run without --background to include them)")
    if failures:
        print(f"{len(failures)} FAILED: " + "; ".join(failures))
        sys.exit(1)
    print("all end-to-end checks passed")


if __name__ == "__main__":
    try:
        main()
    finally:
        # Only this run's own app; a Tilde the user has open is never touched.
        if alive(app_pid):
            os.kill(app_pid, signal.SIGTERM)
        subprocess.run(["rm", "-rf", WORK], capture_output=True)
        # The private copy's own site data, cookies and caches.
        library = os.path.expanduser("~/Library")
        for leftover in ["WebKit/dev.tilde.Tilde.e2e", "Caches/dev.tilde.Tilde.e2e",
                         "HTTPStorages/dev.tilde.Tilde.e2e", "HTTPStorages/dev.tilde.Tilde.e2e.binarycookies"]:
            subprocess.run(["rm", "-rf", os.path.join(library, leftover)], capture_output=True)
