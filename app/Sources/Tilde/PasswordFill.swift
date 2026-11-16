import AppKit
import WebKit

/// Fills the focused page field from the system Passwords picker.
///
/// macOS offers Edit ▸ AutoFill ▸ Passwords… for native text fields only, never
/// for a web view. So a native field is laid over the page field and focused,
/// the system picker types into it, and the text is copied into the page.
final class PasswordFill: NSObject, NSTextFieldDelegate {
    /// Page scripts cannot see this world, so they cannot reach the remembered field.
    private static let world = WKContentWorld.world(name: "tilde-fill")
    /// The action of the menu item AppKit adds for the picker. Undocumented.
    private static let pickerAction = NSSelectorFromString("_handleInsertFromPasswordsCommand:")
    private static let timeout: TimeInterval = 120

    private weak var window: BrowserWindow?
    private weak var web: WKWebView?
    private var field: NSTextField?
    private var poll: Timer?
    private var started = Date()
    private var lastSeen = ""
    private var starting = false
    private var done: ((String) -> Void)?
    private var observers: [NSObjectProtocol] = []
    /// True once the app or window has lost focus to the picker.
    private var pickerTookFocus = false

    var isActive: Bool { starting || field != nil }

    /// `done` receives a short outcome, never the value.
    func begin(web: WKWebView, window: BrowserWindow, container: NSView, done: @escaping (String) -> Void) {
        guard !isActive else { return done("failed: already filling") }
        starting = true
        self.web = web
        self.window = window
        self.done = done

        let remember = """
            let el = document.activeElement;
            while (el && el.shadowRoot && el.shadowRoot.activeElement) el = el.shadowRoot.activeElement;
            if (!(el instanceof HTMLInputElement) && !(el instanceof HTMLTextAreaElement)) return { ok: false };
            window.__tildeFillTarget = el;
            const r = el.getBoundingClientRect();
            return { ok: true, x: r.left, y: r.top, w: r.width, h: r.height, password: el.type === 'password' };
            """
        web.callAsyncJavaScript(remember, arguments: [:], in: nil, in: Self.world) { [weak self] result in
            guard let self else { return }
            guard case .success(let value) = result, let info = value as? [String: Any], info["ok"] as? Bool == true else {
                return self.end(nil, "failed: no text field is focused in the page")
            }
            func number(_ key: String) -> CGFloat { CGFloat((info[key] as? NSNumber)?.doubleValue ?? 0) }
            let scale = web.pageZoom * web.magnification
            var rect = NSRect(x: number("x") * scale, y: number("y") * scale,
                              width: number("w") * scale, height: number("h") * scale)
            if rect.width < 1 || rect.height < 1 {
                rect = NSRect(x: web.bounds.midX - 100, y: web.bounds.midY - 11, width: 200, height: 22)
            } else if !web.isFlipped {
                rect.origin.y = web.bounds.height - rect.maxY
            }
            self.present(over: container.convert(rect, from: web), in: container,
                         secure: info["password"] as? Bool == true)
        }
    }

    private func present(over frame: NSRect, in container: NSView, secure: Bool) {
        guard let window, let web else { return end(nil, "failed: the tab went away") }
        let field: NSTextField = secure ? NSSecureTextField(frame: frame) : NSTextField(frame: frame)
        field.delegate = self
        field.focusRingType = .none
        field.isBordered = false
        field.drawsBackground = false
        field.alphaValue = 0
        container.addSubview(field, positioned: .above, relativeTo: web)
        self.field = field
        starting = false
        guard window.makeFirstResponder(field) else { return end(nil, "failed: could not focus the relay field") }

        // After this turn of the run loop, so the field editor is in place.
        DispatchQueue.main.async { [weak self] in
            guard let self, self.field === field else { return }
            let item = Self.pickerItem()
            guard let item, item.isEnabled, let menu = item.menu else {
                return self.end(nil, "failed: the system Passwords item is \(item == nil ? "missing" : "disabled")")
            }
            menu.performActionForItem(at: menu.index(of: item))
            self.started = Date()
            self.lastSeen = ""
            self.poll = Timer.scheduledTimer(withTimeInterval: 0.15, repeats: true) { [weak self] _ in self?.check() }
            self.watchPicker()
        }
    }

    /// The picker gives no callback, so the field is watched: text that has
    /// stopped changing is taken as the picked value.
    private func check() {
        guard let field else { return }
        let text = field.currentEditor()?.string ?? field.stringValue
        if !text.isEmpty, text == lastSeen {
            end(text, "filled")
        } else if Date().timeIntervalSince(started) > Self.timeout {
            end(nil, "cancelled: timed out")
        }
        lastSeen = text
    }

    /// The picker takes focus from the app while it is open. Focus coming
    /// back with nothing typed means it was dismissed without a choice.
    private func watchPicker() {
        let center = NotificationCenter.default
        func observe(_ name: Notification.Name, _ object: Any?, _ body: @escaping () -> Void) {
            observers.append(center.addObserver(forName: name, object: object, queue: .main) { _ in body() })
        }
        let left: () -> Void = { [weak self] in
            self?.pickerTookFocus = true
        }
        let returned = { [weak self] in
            guard let self, self.pickerTookFocus else { return }
            let field = self.field
            // A picked value arrives around the same moment; give it time to land.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { [weak self] in
                guard let self, let current = self.field, current === field, NSApp.isActive,
                      self.window?.isKeyWindow == true else { return }
                if (current.currentEditor()?.string ?? current.stringValue).isEmpty {
                    self.end(nil, "cancelled: the picker closed")
                }
            }
        }
        observe(NSApplication.didResignActiveNotification, nil, left)
        observe(NSApplication.didBecomeActiveNotification, nil, returned)
        observe(NSWindow.didResignKeyNotification, window, left)
        observe(NSWindow.didBecomeKeyNotification, window, returned)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        if selector == #selector(NSResponder.cancelOperation(_:)) {
            end(nil, "cancelled: escape")
            return true
        }
        return selector == #selector(NSResponder.insertNewline(_:))
    }

    func controlTextDidEndEditing(_ notification: Notification) {
        if field != nil { end(nil, "cancelled: focus moved away") }
    }

    private func end(_ value: String?, _ outcome: String) {
        poll?.invalidate()
        poll = nil
        starting = false
        observers.forEach(NotificationCenter.default.removeObserver)
        observers = []
        pickerTookFocus = false
        let hadField = field != nil
        if let field {
            self.field = nil  // first, so removing it is not taken for a cancel
            field.stringValue = ""
            field.removeFromSuperview()
        }
        let done = self.done
        self.done = nil

        // Focus returns to the page only if nothing else has taken it meanwhile.
        if hadField, let window, let web, window.firstResponder === window {
            window.allowPageFocus = true
            window.makeFirstResponder(web)
            window.allowPageFocus = false
        }
        guard let web else { done?(outcome); return }
        let apply = """
            const el = window.__tildeFillTarget;
            window.__tildeFillTarget = null;
            if (value === null || !el || !el.isConnected) return false;
            el.focus();
            const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
            Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, value);
            el.dispatchEvent(new Event('input', { bubbles: true }));
            el.dispatchEvent(new Event('change', { bubbles: true }));
            return true;
            """
        web.callAsyncJavaScript(apply, arguments: ["value": value ?? NSNull()], in: nil, in: Self.world) { result in
            if value == nil { return done?(outcome) ?? () }
            if case .success(let filled) = result, filled as? Bool == true {
                done?(outcome)
            } else {
                done?("failed: the page field went away")
            }
        }
    }

    /// The Passwords… item AppKit added to the main menu, validated for the
    /// current first responder.
    private static func pickerItem() -> NSMenuItem? {
        func find(in menu: NSMenu) -> NSMenuItem? {
            menu.update()
            for item in menu.items {
                if item.action == pickerAction { return item }
                if let submenu = item.submenu, let found = find(in: submenu) { return found }
            }
            return nil
        }
        return NSApp.mainMenu.flatMap { find(in: $0) }
    }
}
