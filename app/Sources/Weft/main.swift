import AppKit

MainActor.assumeIsolated {
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    let controller = AppController()
    app.delegate = controller
    withExtendedLifetime(controller) {
        app.run()
    }
}
