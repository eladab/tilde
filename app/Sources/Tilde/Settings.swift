import Foundation

/// The few choices that are made by editing a file: `settings.json`, beside
/// the session. Every key is optional. The TUI reads the same file for the
/// keys that are its own (`discard_after_minutes`).
struct Settings {
    /// `content_blocking`: block requests to advertising and tracking networks.
    var contentBlocking = true
    /// `blocklist_url`: a list of domains to block, one per line or as a
    /// hosts file, fetched once a week. An empty string fetches nothing.
    var blocklistURL = URL(string: "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=nohtml&showintro=0&mimetype=plaintext")

    static func load(from directory: URL) -> Settings {
        var settings = Settings()
        guard let data = try? Data(contentsOf: directory.appendingPathComponent("settings.json")),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return settings }
        if let value = object["content_blocking"] as? Bool { settings.contentBlocking = value }
        if let value = object["blocklist_url"] as? String {
            let url = URL(string: value)
            settings.blocklistURL = url?.scheme == "https" ? url : nil
        }
        return settings
    }
}
