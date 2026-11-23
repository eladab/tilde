import CryptoKit
import Foundation

/// Turns lists of advertising and tracking domains into the rules WebKit's
/// content blocker compiles (`WKContentRuleList`).
public enum BlockRules {
    /// Blocked with no list fetched: the networks met most often. A fetched
    /// list (see `ContentBlocker`) adds to these.
    public static let builtIn: [String] = [
        "doubleclick.net", "googlesyndication.com", "googleadservices.com", "google-analytics.com",
        "googletagmanager.com", "googletagservices.com", "adservice.google.com", "analytics.google.com",
        "2mdn.net", "admob.com", "adsensecustomsearchads.com",
        "facebook.net", "connect.facebook.net", "pixel.facebook.com", "an.facebook.com",
        "amazon-adsystem.com", "adnxs.com", "adsrvr.org", "adform.net", "advertising.com", "adroll.com",
        "criteo.com", "criteo.net", "rubiconproject.com", "pubmatic.com", "openx.net", "casalemedia.com",
        "taboola.com", "outbrain.com", "revcontent.com", "mgid.com", "zemanta.com", "teads.tv",
        "scorecardresearch.com", "quantserve.com", "quantcount.com", "moatads.com", "adsafeprotected.com",
        "doubleverify.com", "chartbeat.com", "chartbeat.net", "newrelic.com", "nr-data.net",
        "hotjar.com", "hotjar.io", "mouseflow.com", "fullstory.com", "crazyegg.com", "clarity.ms",
        "luckyorange.com", "inspectlet.com", "smartlook.com", "logrocket.io",
        "mixpanel.com", "segment.io", "segment.com", "amplitude.com", "heapanalytics.com", "kissmetrics.com",
        "optimizely.com", "branch.io", "appsflyer.com", "adjust.com", "kochava.com",
        "bat.bing.com", "ads.linkedin.com", "px.ads.linkedin.com", "snap.licdn.com",
        "ads-twitter.com", "analytics.twitter.com", "static.ads-twitter.com", "t.co/i/adsct",
        "analytics.tiktok.com", "ads.tiktok.com", "sc-static.net", "tr.snapchat.com",
        "ads.pinterest.com", "ct.pinterest.com", "ads.yahoo.com", "analytics.yahoo.com", "yieldmo.com",
        "bluekai.com", "krxd.net", "demdex.net", "everesttech.net", "omtrdc.net", "2o7.net",
        "exelator.com", "eyeota.net", "tapad.com", "rlcdn.com", "liveramp.com", "agkn.com",
        "mathtag.com", "bidswitch.net", "sharethrough.com", "smartadserver.com", "contextweb.com",
        "indexww.com", "lijit.com", "sovrn.com", "33across.com", "gumgum.com", "triplelift.com",
        "media.net", "yandex.ru/metrika", "mc.yandex.ru", "an.yandex.ru", "adriver.ru", "tns-counter.ru",
        "zedo.com", "serving-sys.com", "sizmek.com", "flashtalking.com",
        "turn.com", "spotxchange.com", "springserve.com", "stickyadstv.com", "tremorhub.com",
        "popads.net", "popcash.net", "propellerads.com", "exoclick.com", "juicyads.com", "adsterra.com",
        "onesignal.com", "pushwoosh.com", "statcounter.com", "histats.com", "clicky.com", "getclicky.com",
        "matomo.cloud", "parsely.com", "parse.ly",
        "addthis.com", "sharethis.com", "disqusads.com", "viglink.com", "skimresources.com",
        "bounceexchange.com", "bouncex.net", "permutive.com", "permutive.app", "id5-sync.com",
        "crwdcntrl.net", "lotame.com", "tiqcdn.com", "tealiumiq.com",
        "cxense.com", "ml314.com", "bizographics.com", "6sc.co", "leadfeeder.com",
        "intentiq.com", "adtelligent.com", "improvedigital.com", "undertone.com", "kargo.com",
    ]

    /// The domains named in a list of one of the usual shapes: a domain per
    /// line, or a hosts file (`0.0.0.0 example.com`). Comments and anything
    /// that is not a plain domain are skipped.
    public static func domains(in text: String) -> [String] {
        var out: [String] = []
        for line in text.split(whereSeparator: \.isNewline) {
            let content = line.split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
            let words = content.split(whereSeparator: { $0 == " " || $0 == "\t" })
            guard var word = words.last.map({ $0.lowercased() }), words.count <= 2 else { continue }
            if word.hasPrefix("||") { word = String(word.dropFirst(2)) }
            if word.hasSuffix("^") { word = String(word.dropLast()) }
            guard isDomain(word), word != "localhost" else { continue }
            out.append(word)
        }
        return out
    }

    public static func isDomain(_ text: String) -> Bool {
        let labels = text.split(separator: ".", omittingEmptySubsequences: false)
        guard labels.count >= 2, text.count <= 253 else { return false }
        return labels.allSatisfy { label in
            !label.isEmpty && label.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") }
        }
    }

    private static func escaped(_ text: String) -> String {
        var out = ""
        for c in text {
            if "\\.[]{}()*+?^$|".contains(c) { out.append("\\") }
            out.append(c)
        }
        return out
    }

    /// The pattern that matches an address on `entry` or a subdomain of it.
    /// An entry may carry the start of a path (`example.com/pixel`).
    static func pattern(for entry: String) -> String {
        let parts = entry.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false)
        let host = escaped(String(parts[0]))
        let prefix = "^[^:]+://+([^:/]+\\.)?" + host
        if parts.count == 2, !parts[1].isEmpty { return prefix + "(:[0-9]+)?/" + escaped(String(parts[1])) }
        return prefix + "[:/]"
    }

    /// The rule list, as the JSON WebKit compiles. Requests to `domains`
    /// from pages of another site are blocked; pages of the `exempt` sites
    /// are left alone entirely.
    public static func json(domains: [String], exempt: [String]) -> String {
        var seen = Set<String>()
        var rules: [[String: Any]] = []
        for entry in domains where seen.insert(entry).inserted {
            rules.append([
                "trigger": ["url-filter": pattern(for: entry), "load-type": ["third-party"]],
                "action": ["type": "block"],
            ])
        }
        let sites = exempt.filter(isDomain).sorted()
        if !sites.isEmpty {
            // Within a list, a later rule can undo the ones before it.
            rules.append([
                "trigger": ["url-filter": ".*", "if-top-url": sites.map { "^[^:]+://+([^:/]+\\.)?" + escaped($0) + "[:/]" }],
                "action": ["type": "ignore-previous-rules"],
            ])
        }
        guard let data = try? JSONSerialization.data(withJSONObject: rules, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else { return "[]" }
        return text
    }

    /// Names a compiled list after what went into it, so that an unchanged
    /// list is found again and not compiled a second time.
    public static func identifier(for json: String) -> String {
        let digest = SHA256.hash(data: Data(json.utf8))
        return "tilde-block-" + digest.prefix(8).map { String(format: "%02x", $0) }.joined()
    }

    /// The site an address belongs to for the purpose of switching blocking
    /// off: its host without a leading `www.`.
    public static func site(of url: URL?) -> String? {
        guard let host = url?.host?.lowercased(), !host.isEmpty else { return nil }
        return host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
    }
}
