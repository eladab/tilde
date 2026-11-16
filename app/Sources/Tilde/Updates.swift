import AppKit
import Sparkle

/// Updates through Sparkle: it reads the feed named by `SUFeedURL` in
/// Info.plist, and installs a newer release if its signature matches
/// `SUPublicEDKey`.
///
/// Only an installed Tilde.app updates itself. Unbundled debug builds have no
/// feed, and test copies must not ask about updates or replace themselves.
final class Updates {
    private let controller: SPUStandardUpdaterController?

    init() {
        let info = Bundle.main.infoDictionary ?? [:]
        let env = ProcessInfo.processInfo.environment
        let configured = info["SUFeedURL"] != nil && info["SUPublicEDKey"] != nil
        let underTest = env["TILDE_DEBUG_SOCKET"] != nil || env["TILDE_SESSION"] != nil
        if configured && !underTest {
            controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
        } else {
            controller = nil
        }
    }

    var canCheck: Bool { controller?.updater.canCheckForUpdates ?? false }

    func check() {
        controller?.checkForUpdates(nil)
    }
}
