//! URL-bar input resolution. Mirrors `InputResolver` in the app; the app's
//! version is authoritative, this one drives the mock engine.

pub const SEARCH_BASE: &str = "https://duckduckgo.com/?q=";

pub fn resolve(raw: &str) -> Option<String> {
    let input = raw.trim();
    if input.is_empty() {
        return None;
    }
    if has_scheme(input) {
        return Some(input.to_string());
    }
    if looks_like_host(input) {
        return Some(format!("https://{input}"));
    }
    Some(search_url(input))
}

fn has_scheme(s: &str) -> bool {
    let lower = s.to_lowercase();
    // These carry a payload, not an address, so spaces are legitimate.
    if lower.starts_with("data:") || lower.starts_with("javascript:") {
        return true;
    }
    if s.contains(' ') {
        return false;
    }
    for prefix in ["about:", "file:", "blob:", "mailto:"] {
        if lower.starts_with(prefix) {
            return true;
        }
    }
    let Some(pos) = s.find("://") else { return false };
    let scheme = &s[..pos];
    let mut chars = scheme.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() => {}
        _ => return false,
    }
    scheme.chars().all(|c| c.is_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn looks_like_host(s: &str) -> bool {
    if s.contains(' ') {
        return false;
    }
    let authority = s.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return false;
    }
    if authority.starts_with('[') {
        return authority.contains(']');
    }
    let mut host = authority;
    if let Some(colon) = host.rfind(':') {
        let port = &host[colon + 1..];
        if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        host = &host[..colon];
    }
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty()) {
        return false;
    }
    host.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.')
}

fn search_url(query: &str) -> String {
    let mut out = String::from(SEARCH_BASE);
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Short label for a URL when a tab has no title yet.
pub fn display_host(url: &str) -> String {
    // A data: URL has no host, and its payload makes a useless label.
    if url.len() >= 5 && url[..5].eq_ignore_ascii_case("data:") {
        return "data: page".to_string();
    }
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> Option<String> {
        resolve(s)
    }

    #[test]
    fn scheme_is_kept() {
        assert_eq!(r("https://example.com/a?b=1").as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(r("http://localhost:3000").as_deref(), Some("http://localhost:3000"));
        assert_eq!(r("about:blank").as_deref(), Some("about:blank"));
        assert_eq!(r("file:///tmp/x.html").as_deref(), Some("file:///tmp/x.html"));
        assert_eq!(r("data:text/html,<b>a b</b>").as_deref(), Some("data:text/html,<b>a b</b>"));
        assert_eq!(r("about: blank").as_deref(), Some("https://duckduckgo.com/?q=about%3A%20blank"));
    }

    #[test]
    fn hosts_get_https() {
        assert_eq!(r("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(r("  example.com/path?q=1 ").as_deref(), Some("https://example.com/path?q=1"));
        assert_eq!(r("localhost").as_deref(), Some("https://localhost"));
        assert_eq!(r("localhost:8080/x").as_deref(), Some("https://localhost:8080/x"));
        assert_eq!(r("192.168.1.1").as_deref(), Some("https://192.168.1.1"));
        assert_eq!(r("sub.domain.co.uk").as_deref(), Some("https://sub.domain.co.uk"));
    }

    #[test]
    fn everything_else_searches() {
        assert_eq!(r("swift concurrency").as_deref(), Some("https://duckduckgo.com/?q=swift%20concurrency"));
        assert_eq!(r("webkit").as_deref(), Some("https://duckduckgo.com/?q=webkit"));
        assert_eq!(r("what is a.b c").as_deref(), Some("https://duckduckgo.com/?q=what%20is%20a.b%20c"));
        assert_eq!(r("foo:bar").as_deref(), Some("https://duckduckgo.com/?q=foo%3Abar"));
        assert_eq!(r("a&b=c").as_deref(), Some("https://duckduckgo.com/?q=a%26b%3Dc"));
        assert_eq!(r("example.").as_deref(), Some("https://duckduckgo.com/?q=example."));
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(r(""), None);
        assert_eq!(r("   "), None);
    }

    #[test]
    fn host_labels() {
        assert_eq!(display_host("https://www.example.com/a/b"), "example.com");
        assert_eq!(display_host("about:blank"), "about:blank");
        assert_eq!(display_host("data:text/html,<b>x</b>"), "data: page");
    }
}
