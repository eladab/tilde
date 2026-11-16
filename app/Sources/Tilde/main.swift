import AppKit
import TildeCore

MainActor.assumeIsolated {
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    // Before anything opens the session or a web view. Only the real app
    // adopts the old data: test copies and unbundled builds leave it alone.
    if Bundle.main.bundleIdentifier == "dev.tilde.Tilde" {
        let library = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library")
        Migration.run(library: library).forEach(Log.write)
    }
    let controller = AppController()
    app.delegate = controller
    withExtendedLifetime(controller) {
        app.run()
    }
}
