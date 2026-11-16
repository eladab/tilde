import Foundation

/// The app used to be called Weft. On first launch under the new name, the
/// session, the cookies and the site data are moved to where the app now
/// looks for them, so tabs and logins carry over. Nothing is moved if the
/// new location already exists, and nothing is ever deleted.
public enum Migration {
    public static let moves = [
        ("Application Support/weft", "Application Support/tilde"),
        // If the new folder already exists (a test run or a development
        // build made it), the folder above stays put; take the session alone.
        ("Application Support/weft/session.json", "Application Support/tilde/session.json"),
        ("WebKit/dev.weft.Weft", "WebKit/dev.tilde.Tilde"),
        ("HTTPStorages/dev.weft.Weft", "HTTPStorages/dev.tilde.Tilde"),
        ("HTTPStorages/dev.weft.Weft.binarycookies", "HTTPStorages/dev.tilde.Tilde.binarycookies"),
    ]

    /// `library` is the user's Library folder. Returns a line for each thing
    /// moved or left in place because of an error.
    @discardableResult
    public static func run(library: URL) -> [String] {
        var report: [String] = []
        for (old, new) in moves {
            let from = library.appendingPathComponent(old)
            let to = library.appendingPathComponent(new)
            guard FileManager.default.fileExists(atPath: from.path),
                  !FileManager.default.fileExists(atPath: to.path) else { continue }
            do {
                try FileManager.default.createDirectory(at: to.deletingLastPathComponent(), withIntermediateDirectories: true)
                try FileManager.default.moveItem(at: from, to: to)
                report.append("migrated \(old) to \(new)")
            } catch {
                report.append("could not migrate \(old): \(error.localizedDescription)")
            }
        }
        return report
    }
}
