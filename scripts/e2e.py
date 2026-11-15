#!/usr/bin/env python3
"""End-to-end acceptance checks for Weft.

Launches build/Weft.app with a throwaway session and drives it through the
debug socket (enabled only by WEFT_DEBUG_SOCKET): typing into the TUI,
synthesised key presses and clicks, and JavaScript probes in the page.

Needs network access. Opens a real window and takes keyboard focus while it
runs (about a minute). Run `scripts/bundle.sh` first.

    python3 scripts/e2e.py
"""
import json, os, signal, socket, subprocess, sys, tempfile, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
APP = os.path.join(ROOT, "build/Weft.app")
WORK = tempfile.mkdtemp(prefix="weft-e2e-", dir="/tmp")
DEBUG_SOCK = os.path.join(WORK, "debug.sock")
SESSION = os.path.join(WORK, "session.json")
failures = []


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


def processes():
    return subprocess.run(["pgrep", "-x", "Weft|weft-tui"], capture_output=True, text=True).stdout.split()


def wait_gone(timeout=10):
    end = time.time() + timeout
    while time.time() < end:
        if not processes():
            return True
        time.sleep(0.2)
    return False


def launch():
    if os.path.exists(DEBUG_SOCK):
        os.remove(DEBUG_SOCK)
    subprocess.run(["open", APP, "--env", f"WEFT_DEBUG_SOCKET={DEBUG_SOCK}", "--env", f"WEFT_SESSION={SESSION}"], check=True)
    end = time.time() + 15
    while not os.path.exists(DEBUG_SOCK):
        if time.time() > end:
            sys.exit("the app did not create its debug socket")
        time.sleep(0.1)
    time.sleep(0.3)
    d = Debug(DEBUG_SOCK)
    # Synthesised clicks only reach the views of the active app, and an app
    # opened from a background process is not always activated.
    end = time.time() + 10
    while not d.call("debug.state").get("app_active"):
        if time.time() > end:
            sys.exit("could not bring Weft to the front (is the screen locked or asleep?)")
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


def session_tabs(s):
    return [(t["url"], t["title"]) for t in s["tabs"]]


def open_tab(d, text):
    key(d, "t", "cmd", 17)
    wait(d, lambda s: s["mode"] == "nav" and s.get("web_hidden") is True, "new tab bar")
    typ(d, text)
    typ(d, "\r")


def main():
    if not os.path.isdir(APP):
        sys.exit("build/Weft.app not found: run scripts/bundle.sh first")
    if processes():
        sys.exit("Weft is already running; quit it first")

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
    rect = json.loads(js(d, "JSON.stringify(document.getElementById('a').getBoundingClientRect())"))
    d.call("debug.click", target="page", x=int(rect["x"] + 30), y=int(rect["y"] + rect["height"] / 2))
    wait(d, lambda s: len(s["tabs"]) == count + 1 and "example.org" in active(s)["url"], "target=_blank tab")
    check("a target=_blank link opens and shows a new tab", True)
    key(d, "\x19", "ctrl+shift", 48)
    wait(d, lambda s: active(s)["title"] == "form", "Ctrl+Shift+Tab")
    key(d, "\t", "ctrl", 48)
    wait(d, lambda s: "example.org" in active(s)["url"], "Ctrl+Tab")
    key(d, "1", "cmd", 18)
    wait(d, lambda s: "webkit.org" in active(s)["url"], "Cmd+1")
    key(d, "2", "cmd", 19)
    wait(d, lambda s: active(s)["title"] == "form", "Cmd+2")
    check("Ctrl+Tab, Ctrl+Shift+Tab and Cmd+digit switch tabs from the page", state(d)["mode"] == "page")
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

    # Ten tabs, no reload when switching
    nav(d)
    have = len(state(d)["tabs"])
    sites = ["example.net", "apple.com", "wikipedia.org", "nonexistent-host-weft-test.invalid", "swift.org", "rust-lang.org", "duckduckgo.com", "example.com", "example.org"]
    for site in sites[: 10 - have]:
        open_tab(d, site)
        wait(d, lambda s: s["mode"] == "page", "PAGE after opening " + site)
    s = wait(d, lambda s: len(s["tabs"]) == 10 and all(not t["loading"] for t in s["tabs"]), "ten tabs loaded", 90)
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
    os.kill(int(subprocess.check_output(["pgrep", "-x", "weft-tui"]).split()[0]), signal.SIGKILL)
    s = wait(d, lambda s: s["awaiting_restart"] and not s["tui_running"], "restart prompt")
    check("kill -9 of weft-tui shows the restart prompt and hides the page", s["web_hidden"] is True)
    typ(d, " ")
    wait(d, lambda s: s["tui_running"] and s["tui_connected"] and s.get("web_hidden") is False, "TUI restarted")
    time.sleep(1)
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

    # Quit and restore
    time.sleep(1)
    typ(d, "\r"); wait(d, lambda s: s["mode"] == "page", "PAGE")
    key(d, "q", "cmd", 12)
    check("Cmd+Q leaves no Weft or weft-tui process", wait_gone(), str(processes()))
    before = session()
    check("the session has ten tabs", len(before["tabs"]) == 10)

    d = launch()
    s = wait(d, lambda s: s.get("active") and s["tui_connected"] and active(s)["url"], "relaunch")
    time.sleep(1.5)
    s, after = state(d), session()
    check("relaunch restores tabs and order", session_tabs(before) == session_tabs(after))
    check("relaunch restores the active tab",
          before["active"] == after["active"] and active(s)["url"] == before["tabs"][before["active"]]["url"])
    check("only the active tab is loaded at launch", len(s["tabs"]) == 1)
    typ(d, "q")
    check("q quits everything", wait_gone(), str(processes()))

    print()
    if failures:
        print(f"{len(failures)} FAILED: " + "; ".join(failures))
        sys.exit(1)
    print("all end-to-end checks passed")


if __name__ == "__main__":
    try:
        main()
    finally:
        subprocess.run(["pkill", "-x", "Weft"], capture_output=True)
