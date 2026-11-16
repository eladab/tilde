import Foundation

/// Turns raw URL-bar text into a URL: explicit scheme, bare host, or search.
public enum InputResolver {
    public static let searchBase = "https://duckduckgo.com/?q="

    public static func resolve(_ raw: String) -> URL? {
        let input = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if input.isEmpty { return nil }
        if hasScheme(input), let url = URL(string: input) { return url }
        if looksLikeHost(input), let url = URL(string: "https://" + input) { return url }
        return searchURL(input)
    }

    static func hasScheme(_ s: String) -> Bool {
        let lower = s.lowercased()
        // These carry a payload, not an address, so spaces are legitimate.
        for prefix in ["data:", "javascript:"] where lower.hasPrefix(prefix) {
            return true
        }
        if s.contains(" ") { return false }
        for prefix in ["about:", "file:", "blob:", "mailto:"] where lower.hasPrefix(prefix) {
            return true
        }
        guard let range = s.range(of: "://") else { return false }
        let scheme = s[s.startIndex..<range.lowerBound]
        guard let first = scheme.first, first.isLetter else { return false }
        return scheme.allSatisfy { $0.isLetter || $0.isNumber || $0 == "+" || $0 == "-" || $0 == "." }
    }

    static func looksLikeHost(_ s: String) -> Bool {
        if s.contains(" ") { return false }
        // Split off path/query/fragment, then an optional port.
        let authority = s.split(whereSeparator: { $0 == "/" || $0 == "?" || $0 == "#" }).first.map(String.init) ?? ""
        if authority.isEmpty { return false }
        var host = authority
        if host.hasPrefix("[") { return host.contains("]") } // IPv6 literal
        if let colon = host.lastIndex(of: ":") {
            let port = host[host.index(after: colon)...]
            guard !port.isEmpty, port.allSatisfy({ $0.isNumber }) else { return false }
            host = String(host[..<colon])
        }
        if host.lowercased() == "localhost" { return true }
        let labels = host.split(separator: ".", omittingEmptySubsequences: false)
        guard labels.count >= 2, labels.allSatisfy({ !$0.isEmpty }) else { return false }
        return host.allSatisfy { $0.isLetter || $0.isNumber || $0 == "-" || $0 == "." }
    }

    static func searchURL(_ query: String) -> URL? {
        var allowed = CharacterSet.alphanumerics
        allowed.insert(charactersIn: "-._~")
        let encoded = query.addingPercentEncoding(withAllowedCharacters: allowed) ?? ""
        return URL(string: searchBase + encoded)
    }
}
