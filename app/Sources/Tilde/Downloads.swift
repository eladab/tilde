import AppKit
import WebKit

/// One download, current or past, as it is saved and as the TUI is told about it.
struct DownloadRecord: Codable {
    enum State: String, Codable {
        case active, done, failed, cancelled
    }

    var id: Int
    /// Where it came from. A `data:` or `blob:` address is cut to its scheme.
    var url: String
    var filename: String
    var path: String
    var state: State
    var received: Int64
    /// Zero while the size is not known.
    var total: Int64
    /// Seconds since 1970.
    var started: Double
    var error: String?

    var wire: [String: Any] {
        var fields: [String: Any] = [
            "download_id": id,
            "url": url,
            "filename": filename,
            "path": path,
            "state": state.rawValue,
            "received": received,
            "total": total,
            "started": Int(started),
        ]
        if let error { fields["error"] = error }
        return fields
    }
}

/// Runs downloads and keeps the list of them, which outlives the app.
/// The list is one for the whole browser: it does not belong to a space.
final class DownloadStore: NSObject, WKDownloadDelegate {
    /// Oldest first.
    private(set) var records: [DownloadRecord] = []
    /// One record changed.
    var onUpdate: ((DownloadRecord) -> Void)?
    /// Records were removed.
    var onListChange: (() -> Void)?

    private struct Live {
        let download: WKDownload
        var id: Int?
    }

    private var live: [ObjectIdentifier: Live] = [:]
    private var nextID = 1
    private var timer: Timer?
    private let file: URL
    private let directory: URL
    private static let limit = 500

    /// `file` holds the list; `directory` is where downloaded files go.
    init(file: URL, directory: URL) {
        self.file = file
        self.directory = directory
        super.init()
        load()
    }

    // MARK: Starting

    func adopt(_ download: WKDownload) {
        download.delegate = self
        live[ObjectIdentifier(download)] = Live(download: download, id: nil)
    }

    func download(_ download: WKDownload, decideDestinationUsing response: URLResponse, suggestedFilename: String,
                  completionHandler: @escaping (URL?) -> Void) {
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        } catch {
            Log.write("download: cannot create \(directory.path): \(error.localizedDescription)")
            live[ObjectIdentifier(download)] = nil
            return completionHandler(nil)
        }
        let destination = freeDestination(for: suggestedFilename)
        let source = download.originalRequest?.url ?? response.url
        let record = DownloadRecord(
            id: nextID, url: Self.describe(source), filename: destination.lastPathComponent, path: destination.path,
            state: .active, received: 0, total: max(0, response.expectedContentLength),
            started: Date().timeIntervalSince1970, error: nil)
        nextID += 1
        records.append(record)
        trim()
        live[ObjectIdentifier(download)] = Live(download: download, id: record.id)
        Log.write("download \(record.id) started: \(record.filename)")
        save()
        onUpdate?(record)
        startTimer()
        completionHandler(destination)
    }

    /// Saves a file that arrived whole, such as a PDF the page was showing.
    func save(_ data: Data, suggestedFilename: String, from source: URL?) {
        var record = DownloadRecord(
            id: nextID, url: Self.describe(source), filename: suggestedFilename, path: "", state: .done,
            received: Int64(data.count), total: Int64(data.count), started: Date().timeIntervalSince1970, error: nil)
        nextID += 1
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            let destination = freeDestination(for: suggestedFilename)
            record.filename = destination.lastPathComponent
            record.path = destination.path
            try data.write(to: destination, options: .atomic)
        } catch {
            record.state = .failed
            record.error = error.localizedDescription
        }
        records.append(record)
        trim()
        Log.write("download \(record.id) \(record.state.rawValue): \(record.filename)")
        save()
        onUpdate?(record)
    }

    private static func describe(_ url: URL?) -> String {
        guard let url else { return "" }
        let scheme = url.scheme?.lowercased() ?? ""
        return scheme == "data" || scheme == "blob" ? "\(scheme):" : url.absoluteString
    }

    /// A path in the downloads folder that nothing occupies: `name.ext`, `name 2.ext`, …
    private func freeDestination(for suggested: String) -> URL {
        var name = (suggested as NSString).lastPathComponent.replacingOccurrences(of: ":", with: "-")
        if name.isEmpty || name == "/" || name.hasPrefix(".") { name = "download" + (name.hasPrefix(".") ? name : "") }
        let base = (name as NSString).deletingPathExtension
        let ext = (name as NSString).pathExtension
        var candidate = directory.appendingPathComponent(name)
        var n = 2
        while FileManager.default.fileExists(atPath: candidate.path) {
            candidate = directory.appendingPathComponent(ext.isEmpty ? "\(base) \(n)" : "\(base) \(n).\(ext)")
            n += 1
        }
        return candidate
    }

    // MARK: Progress and completion

    private func startTimer() {
        guard timer == nil else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 0.3, repeats: true) { [weak self] _ in self?.poll() }
    }

    private func poll() {
        for entry in live.values {
            guard let id = entry.id, let index = index(of: id) else { continue }
            let progress = entry.download.progress
            let received = progress.completedUnitCount
            let total = max(records[index].total, progress.totalUnitCount)
            if received != records[index].received || total != records[index].total {
                records[index].received = received
                records[index].total = total
                onUpdate?(records[index])
            }
        }
        if !live.values.contains(where: { $0.id != nil }) {
            timer?.invalidate()
            timer = nil
        }
    }

    private func index(of id: Int) -> Int? {
        records.firstIndex { $0.id == id }
    }

    /// Ends a running download with `state`, unless it has ended already.
    private func finish(_ download: WKDownload, state: DownloadRecord.State, error: String? = nil) {
        guard let entry = live.removeValue(forKey: ObjectIdentifier(download)),
              let id = entry.id, let index = index(of: id), records[index].state == .active else { return }
        records[index].state = state
        records[index].error = error
        if state == .done {
            let size = (try? FileManager.default.attributesOfItem(atPath: records[index].path)[.size] as? NSNumber)?.int64Value
            records[index].received = size ?? max(records[index].received, records[index].total)
            records[index].total = records[index].received
        } else {
            // Whatever part arrived is of no use.
            try? FileManager.default.removeItem(atPath: records[index].path)
        }
        Log.write("download \(id) \(state.rawValue)\(error.map { ": \($0)" } ?? "")")
        save()
        onUpdate?(records[index])
    }

    func downloadDidFinish(_ download: WKDownload) {
        finish(download, state: .done)
    }

    func download(_ download: WKDownload, didFailWithError error: Error, resumeData: Data?) {
        let nsError = error as NSError
        if nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled {
            finish(download, state: .cancelled)
        } else {
            finish(download, state: .failed, error: nsError.localizedDescription)
        }
    }

    // MARK: Actions on the list

    var activeCount: Int { records.filter { $0.state == .active }.count }

    /// Opens the downloaded file. Returns what went wrong, if anything.
    func open(_ id: Int) -> String? {
        guard let index = index(of: id) else { return "no such download" }
        let record = records[index]
        guard record.state == .done else { return "This download did not finish." }
        guard FileManager.default.fileExists(atPath: record.path) else { return "The file is no longer where it was saved." }
        return NSWorkspace.shared.open(URL(fileURLWithPath: record.path)) ? nil : "The file could not be opened."
    }

    /// Shows the file in the Finder, or its folder when the file is gone.
    func reveal(_ id: Int) -> String? {
        guard let index = index(of: id) else { return "no such download" }
        let url = URL(fileURLWithPath: records[index].path)
        if FileManager.default.fileExists(atPath: url.path) {
            NSWorkspace.shared.activateFileViewerSelecting([url])
        } else {
            NSWorkspace.shared.open(url.deletingLastPathComponent())
        }
        return nil
    }

    func cancel(_ id: Int) {
        guard let entry = live.values.first(where: { $0.id == id }) else { return }
        finish(entry.download, state: .cancelled)
        entry.download.cancel { _ in }
    }

    /// Takes a download off the list, stopping it first if it is running. The file stays.
    func remove(_ id: Int) {
        cancel(id)
        records.removeAll { $0.id == id }
        save()
        onListChange?()
    }

    /// Takes every download that is not running off the list. The files stay.
    func clear() {
        records.removeAll { $0.state != .active }
        save()
        onListChange?()
    }

    // MARK: Persistence

    private func trim() {
        var extra = records.count - Self.limit
        guard extra > 0 else { return }
        records.removeAll { record in
            guard extra > 0, record.state != .active else { return false }
            extra -= 1
            return true
        }
    }

    private func load() {
        guard let data = try? Data(contentsOf: file),
              let saved = try? JSONDecoder().decode([DownloadRecord].self, from: data) else { return }
        records = saved
        for index in records.indices where records[index].state == .active {
            // The app quit while it was running.
            records[index].state = .failed
            records[index].error = "Interrupted when Tilde quit."
        }
        nextID = (records.map(\.id).max() ?? 0) + 1
    }

    private func save() {
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(records).write(to: file, options: .atomic)
        } catch {
            Log.write("cannot save downloads: \(error.localizedDescription)")
        }
    }
}
