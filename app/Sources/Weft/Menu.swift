import AppKit

/// A browser shortcut: a menu item that is forwarded to the TUI as a `command` event.
struct MenuCommand {
    let name: String
    let arg: Int?
}

enum MainMenu {
    static func build(target: AppController) -> NSMenu {
        let main = NSMenu()

        func submenu(_ title: String) -> NSMenu {
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            let menu = NSMenu(title: title)
            item.submenu = menu
            main.addItem(item)
            return menu
        }

        func command(_ menu: NSMenu, _ title: String, _ name: String, key: String,
                     mods: NSEvent.ModifierFlags = .command, arg: Int? = nil) {
            let item = NSMenuItem(title: title, action: #selector(AppController.menuCommand(_:)), keyEquivalent: key)
            item.keyEquivalentModifierMask = mods
            item.target = target
            item.representedObject = MenuCommand(name: name, arg: arg)
            menu.addItem(item)
        }

        func standard(_ menu: NSMenu, _ title: String, _ action: Selector, key: String,
                      mods: NSEvent.ModifierFlags = .command) {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
            item.keyEquivalentModifierMask = mods
            menu.addItem(item)
        }

        let app = submenu("Weft")
        standard(app, "Hide Weft", #selector(NSApplication.hide(_:)), key: "h")
        app.addItem(.separator())
        let quit = NSMenuItem(title: "Quit Weft", action: #selector(AppController.quitRequested(_:)), keyEquivalent: "q")
        quit.target = target
        app.addItem(quit)

        let file = submenu("File")
        command(file, "New Tab", "new-tab", key: "t")
        command(file, "Open Location…", "edit-url", key: "l")
        file.addItem(.separator())
        command(file, "Close Tab", "close-tab", key: "w")

        let edit = submenu("Edit")
        standard(edit, "Undo", Selector(("undo:")), key: "z")
        standard(edit, "Redo", Selector(("redo:")), key: "z", mods: [.command, .shift])
        edit.addItem(.separator())
        standard(edit, "Cut", #selector(NSText.cut(_:)), key: "x")
        standard(edit, "Copy", #selector(NSText.copy(_:)), key: "c")
        standard(edit, "Paste", #selector(NSText.paste(_:)), key: "v")
        standard(edit, "Select All", #selector(NSText.selectAll(_:)), key: "a")

        let view = submenu("View")
        command(view, "Reload Page", "reload", key: "r")
        command(view, "Switch Between Tabs and Page", "toggle-mode", key: ".")

        let history = submenu("History")
        command(history, "Back", "back", key: "[")
        command(history, "Forward", "forward", key: "]")

        let tabs = submenu("Tabs")
        command(tabs, "Next Tab", "next-tab", key: "\t", mods: .control)
        command(tabs, "Previous Tab", "prev-tab", key: "\t", mods: [.control, .shift])
        tabs.addItem(.separator())
        for n in 1...9 {
            command(tabs, n == 9 ? "Last Tab" : "Tab \(n)", "select-tab", key: "\(n)", arg: n)
        }

        let window = submenu("Window")
        standard(window, "Minimize", #selector(NSWindow.performMiniaturize(_:)), key: "m")
        standard(window, "Zoom", #selector(NSWindow.performZoom(_:)), key: "")
        NSApp.windowsMenu = window

        return main
    }
}
