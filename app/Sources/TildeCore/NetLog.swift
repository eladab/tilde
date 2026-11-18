import Foundation

/// The requests one tab has made, as the dev tools show them.
///
/// Most of what is recorded comes from a script running in the page, so
/// every field is checked and cut to size on the way in: a page can say
/// what it likes about its own requests, but not flood the log.
public final class NetLog {
    /// The oldest requests make way once there are this many.
    public static let maxRecords = 2000
    public static let maxBody = 1_048_576
    public static let maxFrames = 200

    private struct Record {
        var fields: [String: Any] = [:]
        var frames: [[String: Any]] = []
        /// Its timing has come from WebKit's own measurement, which is
        /// more exact than what the script clocked.
        var timed = false
    }

    private var records: [Int: Record] = [:]
    private var order: [Int] = []
    private var nextID = 1
    /// The script's own numbering, per document: "document:number" to id.
    private var scriptIDs: [String: Int] = [:]
    /// Documents that have reported, and those of pages the tab has left.
    private var documents = Set<String>()
    private var retired = Set<String>()

    public init() {}

    public var count: Int { order.count }

    // MARK: Fields

    private static let strings: [String: Int] = [
        "kind": 16, "method": 16, "url": 8192, "state": 16, "detail": 16, "status_text": 128, "mime": 128,
        "protocol": 32, "error": 512, "request_note": 512, "response_note": 512,
        "request_body": maxBody, "response_body": maxBody,
    ]
    private static let integers: Set<String> = ["status", "size", "body_size"]
    private static let numbers: Set<String> = ["duration", "started"]
    private static let headers: Set<String> = ["request_headers", "response_headers"]
    private static let flags: Set<String> = ["response_truncated", "cached"]
    private static let summaryKeys = ["method", "url", "status", "kind", "state", "size", "cached", "duration", "started", "mime", "error"]

    private static func clipped(_ text: String, _ limit: Int) -> String {
        text.count > limit ? String(text.prefix(limit)) : text
    }

    /// What is kept of a set of fields: the known ones, of the right type, cut to size.
    static func sanitized(_ patch: [String: Any]) -> [String: Any] {
        var out: [String: Any] = [:]
        for (key, value) in patch {
            if let limit = strings[key] {
                if let text = value as? String { out[key] = clipped(text, limit) }
            } else if integers.contains(key) {
                if let number = value as? NSNumber, number.doubleValue.isFinite, number.doubleValue >= 0, number.doubleValue < 1e15 {
                    out[key] = Int(number.doubleValue)
                }
            } else if numbers.contains(key) {
                if let number = value as? NSNumber, number.doubleValue.isFinite, number.doubleValue >= 0 {
                    out[key] = number.doubleValue
                }
            } else if headers.contains(key) {
                guard let list = value as? [Any] else { continue }
                out[key] = list.prefix(200).compactMap { pair -> [String]? in
                    guard let pair = pair as? [Any], pair.count == 2,
                          let name = pair[0] as? String, let value = pair[1] as? String else { return nil }
                    return [clipped(name, 256), clipped(value, 8192)]
                }
            } else if flags.contains(key) {
                if let flag = value as? Bool { out[key] = flag }
            }
        }
        return out
    }

    // MARK: Changing the log

    /// Forgets every request, and whatever documents of the page being left
    /// still have to say.
    public func reset() {
        records = [:]
        order = []
        scriptIDs = [:]
        retired.formUnion(documents)
        documents = []
        if retired.count > 500 { retired = [] }
    }

    /// Forgets the requests but goes on listening to the same page.
    public func clear() {
        records = [:]
        order = []
        scriptIDs = [:]
    }

    /// Adds a request the app saw for itself. Returns its id.
    @discardableResult
    public func add(_ fields: [String: Any]) -> Int {
        let id = nextID
        nextID += 1
        records[id] = Record(fields: Self.sanitized(fields))
        order.append(id)
        if order.count > Self.maxRecords {
            let dropped = order.removeFirst()
            records[dropped] = nil
        }
        return id
    }

    public func update(_ id: Int, _ fields: [String: Any]) {
        guard records[id] != nil else { return }
        records[id]?.fields.merge(Self.sanitized(fields)) { _, new in new }
    }

    /// The newest document request for an address, if it has no timing yet.
    private func untimedDocument(at url: String) -> Int? {
        order.reversed().first { id in
            guard let record = records[id] else { return false }
            return !record.timed && record.fields["kind"] as? String == "doc" && record.fields["url"] as? String == url
        }
    }

    /// Takes in what a document's script reports. Each message is `i` (the
    /// script's number for a request) with `p` (fields to set) or `f` (a
    /// WebSocket message); `t` marks fields WebKit measured. Returns the ids
    /// of the requests that changed, in order.
    public func apply(document: String, messages: [[String: Any]]) -> [Int] {
        guard !document.isEmpty, document.count <= 64, !retired.contains(document) else { return [] }
        documents.insert(document)
        var changed: [Int] = []
        func note(_ id: Int) {
            if !changed.contains(id) { changed.append(id) }
        }
        for message in messages.prefix(500) {
            guard let number = (message["i"] as? NSNumber)?.intValue else { continue }
            let key = "\(document):\(number)"
            if let frame = message["f"] as? [String: Any] {
                guard let id = scriptIDs[key], let data = frame["data"] as? String else { continue }
                var frames = records[id]?.frames ?? []
                frames.append(["out": frame["out"] as? Bool ?? false, "data": Self.clipped(data, 4096)])
                if frames.count > Self.maxFrames { frames.removeFirst(frames.count - Self.maxFrames) }
                records[id]?.frames = frames
                note(id)
                continue
            }
            guard let patch = message["p"] as? [String: Any] else { continue }
            var fields = Self.sanitized(patch)
            let measured = message["t"] as? Bool == true
            if let id = scriptIDs[key] {
                if records[id]?.timed == true && !measured { fields["duration"] = nil }
                records[id]?.fields.merge(fields) { _, new in new }
                if measured { records[id]?.timed = true }
                note(id)
                continue
            }
            guard let url = fields["url"] as? String, !url.isEmpty else { continue }
            // A document's own timing belongs to the request the app recorded for it.
            if measured, fields["kind"] as? String == "doc", let id = untimedDocument(at: url) {
                for key in ["duration", "size", "body_size", "protocol", "cached"] {
                    if let value = fields[key] { records[id]?.fields[key] = value }
                }
                if let status = fields["status"] as? Int, status > 0, (records[id]?.fields["status"] as? Int ?? 0) == 0 {
                    records[id]?.fields["status"] = status
                }
                records[id]?.timed = true
                scriptIDs[key] = id
                note(id)
                continue
            }
            let id = add(fields)
            records[id]?.timed = measured
            scriptIDs[key] = id
            note(id)
        }
        return changed.filter { records[$0] != nil }
    }

    // MARK: Reading the log

    /// What the list shows of a request.
    public func summary(_ id: Int) -> [String: Any]? {
        guard let record = records[id] else { return nil }
        var out: [String: Any] = ["id": id]
        for key in Self.summaryKeys {
            if let value = record.fields[key] { out[key] = value }
        }
        return out
    }

    public var summaries: [[String: Any]] { order.compactMap(summary) }

    /// Everything known about a request.
    public func full(_ id: Int) -> [String: Any]? {
        guard let record = records[id] else { return nil }
        var out = record.fields
        out["id"] = id
        if !record.frames.isEmpty { out["frames"] = record.frames }
        if (out["status_text"] as? String ?? "").isEmpty, let status = out["status"] as? Int, let text = Self.statusText[status] {
            out["status_text"] = text
        }
        return out
    }

    /// HTTP/2 and later send no reason phrase, so the usual ones are supplied.
    static let statusText: [Int: String] = [
        200: "OK", 201: "Created", 202: "Accepted", 204: "No Content", 206: "Partial Content",
        301: "Moved Permanently", 302: "Found", 303: "See Other", 304: "Not Modified",
        307: "Temporary Redirect", 308: "Permanent Redirect",
        400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed",
        408: "Request Timeout", 409: "Conflict", 410: "Gone", 413: "Content Too Large", 415: "Unsupported Media Type",
        418: "I'm a teapot", 422: "Unprocessable Content", 429: "Too Many Requests",
        500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway", 503: "Service Unavailable",
        504: "Gateway Timeout",
    ]
}

/// Which site a host or a cookie belongs to. WebKit groups what sites store
/// by registrable domain ("example.com"), and names each group that way.
public enum SiteMatch {
    /// The site among `names` that `host` belongs to: the one it equals or
    /// is a subdomain of. With none, the host itself, less a leading "www.".
    public static func site(for host: String, among names: [String]) -> String {
        let host = host.lowercased()
        let match = names
            .map { $0.lowercased() }
            .filter { belongs(host, to: $0) }
            .max { $0.count < $1.count }
        return match ?? (host.hasPrefix("www.") ? String(host.dropFirst(4)) : host)
    }

    /// Whether a host, or a cookie's domain (which may begin with a dot), is of a site.
    public static func belongs(_ domain: String, to site: String) -> Bool {
        var domain = domain.lowercased()
        if domain.hasPrefix(".") { domain.removeFirst() }
        let site = site.lowercased()
        return !site.isEmpty && (domain == site || domain.hasSuffix("." + site))
    }
}
