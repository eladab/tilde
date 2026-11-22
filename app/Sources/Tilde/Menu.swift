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

        let app = submenu("Tilde")
        let makeDefault = NSMenuItem(title: "Make Tilde the Default Browser…", action: #selector(AppController.makeDefaultBrowser(_:)), keyEquivalent: "")
        makeDefault.target = target
        app.addItem(makeDefault)
        let update = NSMenuItem(title: "Check for Updates…", action: #selector(AppController.checkForUpdates(_:)), keyEquivalent: "")
        update.target = target
        app.addItem(update)
        app.addItem(.separator())
        standard(app, "Hide Tilde", #selector(NSApplication.hide(_:)), key: "h")
        app.addItem(.separator())
        let quit = NSMenuItem(title: "Quit Tilde", action: #selector(AppController.quitRequested(_:)), keyEquivalent: "q")
        quit.target = target
        app.addItem(quit)

        let file = submenu("File")
        command(file, "New Tab", "new-tab", key: "t")
        command(file, "Open Location…", "edit-url", key: "l")
        file.addItem(.separator())
        command(file, "Close Tab", "close-tab", key: "w")
        file.addItem(.separator())
        let print = NSMenuItem(title: "Print…", action: #selector(AppController.printPage(_:)), keyEquivalent: "p")
        print.target = target
        file.addItem(print)

        let edit = submenu("Edit")
        standard(edit, "Undo", Selector(("undo:")), key: "z")
        standard(edit, "Redo", Selector(("redo:")), key: "z", mods: [.command, .shift])
        edit.addItem(.separator())
        standard(edit, "Cut", #selector(NSText.cut(_:)), key: "x")
        let copy = NSMenuItem(title: "Copy", action: #selector(AppController.copyRequested(_:)), keyEquivalent: "c")
        copy.target = target
        edit.addItem(copy)
        standard(edit, "Paste", #selector(NSText.paste(_:)), key: "v")
        standard(edit, "Select All", #selector(NSText.selectAll(_:)), key: "a")
        edit.addItem(.separator())
        let find = NSMenu(title: "Find")
        let findItem = NSMenuItem(title: "Find", action: nil, keyEquivalent: "")
        findItem.submenu = find
        edit.addItem(findItem)
        command(find, "Find…", "find", key: "f")
        command(find, "Find Next", "find-next", key: "g")
        // An uppercase key equivalent is how AppKit spells Shift for letters.
        command(find, "Find Previous", "find-prev", key: "G")
        let useSelection = NSMenuItem(title: "Use Selection for Find", action: #selector(AppController.findSelection(_:)), keyEquivalent: "e")
        useSelection.target = target
        find.addItem(useSelection)
        edit.addItem(.separator())
        let fill = NSMenuItem(title: "Fill from Passwords…", action: #selector(AppController.fillFromPasswords(_:)), keyEquivalent: "\\")
        fill.target = target
        edit.addItem(fill)

        let view = submenu("View")
        command(view, "Reload Page", "reload", key: "r")
        command(view, "Switch Between Tabs and Page", "toggle-mode", key: ".")
        command(view, "Show or Hide Sidebar", "toggle-sidebar", key: "b")
        command(view, "Downloads", "downloads", key: "l", mods: [.command, .option])
        command(view, "Developer Tools", "devtools", key: "i", mods: [.command, .option])
        view.addItem(.separator())
        func zoom(_ title: String, _ action: Selector, key: String, hidden: Bool = false) {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
            item.target = target
            item.isHidden = hidden
            item.allowsKeyEquivalentWhenHidden = true
            view.addItem(item)
        }
        zoom("Actual Size", #selector(AppController.zoomReset(_:)), key: "0")
        zoom("Zoom In", #selector(AppController.zoomIn(_:)), key: "+")
        // Cmd+= is the same key without Shift; nobody expects to need Shift.
        zoom("Zoom In", #selector(AppController.zoomIn(_:)), key: "=", hidden: true)
        zoom("Zoom Out", #selector(AppController.zoomOut(_:)), key: "-")

        let history = submenu("History")
        command(history, "Back", "back", key: "[")
        command(history, "Forward", "forward", key: "]")
        history.addItem(.separator())
        command(history, "Show All History", "history", key: "y")

        let tabs = submenu("Tabs")
        command(tabs, "Next Tab", "next-tab", key: "\t", mods: .control)
        command(tabs, "Previous Tab", "prev-tab", key: "\t", mods: [.control, .shift])
        tabs.addItem(.separator())
        command(tabs, "Pin or Unpin Tab", "toggle-pin", key: "d")
        // An uppercase key equivalent is how AppKit spells Shift for letters.
        command(tabs, "Return to Pinned Page", "pin-home", key: "H")
        command(tabs, "Rename Tab…", "rename-tab", key: "")
        tabs.addItem(.separator())
        for n in 1...9 {
            command(tabs, n == 9 ? "Last Tab" : "Tab \(n)", "select-tab", key: "\(n)", arg: n)
        }

        let spaces = submenu("Spaces")
        command(spaces, "New Space…", "new-space", key: "N")
        command(spaces, "Edit Space…", "edit-space", key: "")
        command(spaces, "Delete Space…", "delete-space", key: "")
        spaces.addItem(.separator())
        let left = String(UnicodeScalar(NSLeftArrowFunctionKey)!)
        let right = String(UnicodeScalar(NSRightArrowFunctionKey)!)
        command(spaces, "Previous Space", "prev-space", key: left, mods: [.command, .option])
        command(spaces, "Next Space", "next-space", key: right, mods: [.command, .option])
        spaces.addItem(.separator())
        for n in 1...9 {
            command(spaces, "Space \(n)", "select-space", key: "\(n)", mods: .control, arg: n)
        }

        let window = submenu("Window")
        standard(window, "Minimize", #selector(NSWindow.performMiniaturize(_:)), key: "m")
        standard(window, "Zoom", #selector(NSWindow.performZoom(_:)), key: "")
        NSApp.windowsMenu = window

        let help = submenu("Help")
        command(help, "Keyboard Shortcuts", "help", key: "/")

        return main
    }
}
