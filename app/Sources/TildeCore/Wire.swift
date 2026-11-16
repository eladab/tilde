import Foundation

/// Newline-delimited JSON framing shared by the main and debug sockets.
public enum Wire {
    public static let protocolVersion = 1

    public struct Request {
        public let id: Any?
        public let method: String
        public let params: [String: Any]

        public func int(_ key: String) -> Int? { (params[key] as? NSNumber)?.intValue }
        public func string(_ key: String) -> String? { params[key] as? String }
        public func bool(_ key: String) -> Bool? { (params[key] as? NSNumber)?.boolValue }
    }

    public enum DecodeError: Error, Equatable {
        case notAnObject
        case missingMethod
    }

    public static func decodeRequest(_ line: String) throws -> Request {
        guard let data = line.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw DecodeError.notAnObject
        }
        guard let method = obj["method"] as? String else { throw DecodeError.missingMethod }
        return Request(id: obj["id"], method: method, params: obj)
    }

    public static func encode(_ object: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys, .withoutEscapingSlashes]),
              let s = String(data: data, encoding: .utf8) else { return "{}" }
        return s
    }

    public static func reply(id: Any?, _ fields: [String: Any] = [:]) -> String {
        var out = fields
        out["id"] = id ?? NSNull()
        out["ok"] = true
        return encode(out)
    }

    public static func failure(id: Any?, _ message: String) -> String {
        encode(["id": id ?? NSNull(), "ok": false, "error": message])
    }

    public static func event(_ name: String, _ fields: [String: Any] = [:]) -> String {
        var out = fields
        out["event"] = name
        return encode(out)
    }
}
