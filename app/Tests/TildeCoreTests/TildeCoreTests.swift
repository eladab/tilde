import XCTest
@testable import TildeCore

final class InputResolverTests: XCTestCase {
    func r(_ s: String) -> String? { InputResolver.resolve(s)?.absoluteString }

    func testSchemeIsKept() {
        XCTAssertEqual(r("https://example.com/a?b=1"), "https://example.com/a?b=1")
        XCTAssertEqual(r("http://localhost:3000"), "http://localhost:3000")
        XCTAssertEqual(r("about:blank"), "about:blank")
        XCTAssertEqual(r("file:///tmp/x.html"), "file:///tmp/x.html")
        XCTAssertEqual(InputResolver.resolve("data:text/html,<b>a b</b>")?.scheme, "data")
        XCTAssertEqual(r("about: blank"), "https://duckduckgo.com/?q=about%3A%20blank")
    }

    func testHostsGetHttps() {
        XCTAssertEqual(r("example.com"), "https://example.com")
        XCTAssertEqual(r("  example.com/path?q=1 "), "https://example.com/path?q=1")
        XCTAssertEqual(r("localhost"), "https://localhost")
        XCTAssertEqual(r("localhost:8080/x"), "https://localhost:8080/x")
        XCTAssertEqual(r("192.168.1.1"), "https://192.168.1.1")
        XCTAssertEqual(r("sub.domain.co.uk"), "https://sub.domain.co.uk")
    }

    func testEverythingElseSearches() {
        XCTAssertEqual(r("swift concurrency"), "https://duckduckgo.com/?q=swift%20concurrency")
        XCTAssertEqual(r("webkit"), "https://duckduckgo.com/?q=webkit")
        XCTAssertEqual(r("what is a.b c"), "https://duckduckgo.com/?q=what%20is%20a.b%20c")
        XCTAssertEqual(r("foo:bar"), "https://duckduckgo.com/?q=foo%3Abar")
        XCTAssertEqual(r("a&b=c"), "https://duckduckgo.com/?q=a%26b%3Dc")
        XCTAssertEqual(r("example."), "https://duckduckgo.com/?q=example.")
    }

    func testEmpty() {
        XCTAssertNil(r(""))
        XCTAssertNil(r("   "))
    }
}

final class WireTests: XCTestCase {
    func testDecodeRequest() throws {
        let req = try Wire.decodeRequest(#"{"id":7,"method":"viewport.set","col":28,"row":1,"cols":100,"rows":40,"extra":"ignored"}"#)
        XCTAssertEqual(req.method, "viewport.set")
        XCTAssertEqual(req.id as? Int, 7)
        XCTAssertEqual(req.int("col"), 28)
        XCTAssertEqual(req.int("rows"), 40)
        XCTAssertNil(req.int("missing"))
    }

    func testDecodeTypes() throws {
        let req = try Wire.decodeRequest(#"{"id":1,"method":"tab.create","url":"https://a.b","activate":true}"#)
        XCTAssertEqual(req.string("url"), "https://a.b")
        XCTAssertEqual(req.bool("activate"), true)
    }

    func testDecodeErrors() {
        XCTAssertThrowsError(try Wire.decodeRequest("nope")) { XCTAssertEqual($0 as? Wire.DecodeError, .notAnObject) }
        XCTAssertThrowsError(try Wire.decodeRequest("[1]")) { XCTAssertEqual($0 as? Wire.DecodeError, .notAnObject) }
        XCTAssertThrowsError(try Wire.decodeRequest(#"{"id":1}"#)) { XCTAssertEqual($0 as? Wire.DecodeError, .missingMethod) }
    }

    func testEncodeReplyAndEvent() {
        XCTAssertEqual(Wire.reply(id: 3, ["tab_id": 9]), #"{"id":3,"ok":true,"tab_id":9}"#)
        XCTAssertEqual(Wire.failure(id: 3, "no such tab"), #"{"error":"no such tab","id":3,"ok":false}"#)
        XCTAssertEqual(Wire.event("focus.changed", ["mode": "page"]), #"{"event":"focus.changed","mode":"page"}"#)
        XCTAssertEqual(Wire.event("tab.updated", ["tab_id": 1, "url": "https://a.b/c"]), #"{"event":"tab.updated","tab_id":1,"url":"https://a.b/c"}"#)
    }
}

final class ProfileTests: XCTestCase {
    func testDefaultProfileHasNoIdentifier() {
        XCTAssertNil(Profile.storeIdentifier(for: nil))
        XCTAssertNil(Profile.storeIdentifier(for: ""))
        XCTAssertNil(Profile.storeIdentifier(for: "default"))
    }

    func testIdentifiersAreStableAndDistinct() {
        let work = Profile.storeIdentifier(for: "work")
        XCTAssertNotNil(work)
        XCTAssertEqual(work, Profile.storeIdentifier(for: "work"))
        XCTAssertNotEqual(work, Profile.storeIdentifier(for: "personal"))
        // Pinned: changing the derivation would orphan everyone's logins.
        XCTAssertEqual(work?.uuidString.count, 36)
        XCTAssertEqual(work, Profile.storeIdentifier(for: "work"))
        let text = work!.uuidString
        XCTAssertEqual(text[text.index(text.startIndex, offsetBy: 14)], "8")
    }
}

final class MigrationTests: XCTestCase {
    private func makeLibrary() throws -> URL {
        let library = FileManager.default.temporaryDirectory.appendingPathComponent("tilde-migration-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: library, withIntermediateDirectories: true)
        return library
    }

    private func write(_ text: String, _ path: String, in library: URL) throws {
        let url = library.appendingPathComponent(path)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try text.write(to: url, atomically: true, encoding: .utf8)
    }

    private func read(_ path: String, in library: URL) -> String? {
        try? String(contentsOf: library.appendingPathComponent(path), encoding: .utf8)
    }

    func testMovesEverythingKeptUnderTheOldName() throws {
        let library = try makeLibrary()
        defer { try? FileManager.default.removeItem(at: library) }
        try write("session", "Application Support/weft/session.json", in: library)
        try write("site data", "WebKit/dev.weft.Weft/WebsiteData/x", in: library)
        try write("cookies", "HTTPStorages/dev.weft.Weft.binarycookies", in: library)
        try write("other app", "WebKit/com.example.Other/x", in: library)

        let report = Migration.run(library: library)
        XCTAssertEqual(report.count, 3)
        XCTAssertEqual(read("Application Support/tilde/session.json", in: library), "session")
        XCTAssertEqual(read("WebKit/dev.tilde.Tilde/WebsiteData/x", in: library), "site data")
        XCTAssertEqual(read("HTTPStorages/dev.tilde.Tilde.binarycookies", in: library), "cookies")
        XCTAssertNil(read("Application Support/weft/session.json", in: library))
        XCTAssertEqual(read("WebKit/com.example.Other/x", in: library), "other app")

        // A second launch finds nothing left to do.
        XCTAssertEqual(Migration.run(library: library), [])
    }

    func testNeverOverwritesDataAlreadyUnderTheNewName() throws {
        let library = try makeLibrary()
        defer { try? FileManager.default.removeItem(at: library) }
        try write("old", "Application Support/weft/session.json", in: library)
        try write("new", "Application Support/tilde/session.json", in: library)

        XCTAssertEqual(Migration.run(library: library), [])
        XCTAssertEqual(read("Application Support/tilde/session.json", in: library), "new")
        XCTAssertEqual(read("Application Support/weft/session.json", in: library), "old", "the old copy is left where it was")
    }

    func testSessionIsAdoptedEvenWhenTheNewFolderAlreadyExists() throws {
        let library = try makeLibrary()
        defer { try? FileManager.default.removeItem(at: library) }
        try write("old tabs", "Application Support/weft/session.json", in: library)
        try write("", "Application Support/tilde/tilde-123.sock", in: library)

        XCTAssertEqual(Migration.run(library: library).count, 1)
        XCTAssertEqual(read("Application Support/tilde/session.json", in: library), "old tabs")
    }

    func testNothingToMoveIsFine() throws {
        let library = try makeLibrary()
        defer { try? FileManager.default.removeItem(at: library) }
        XCTAssertEqual(Migration.run(library: library), [])
    }
}

final class NetLogTests: XCTestCase {
    private func start(_ number: Int, _ url: String, kind: String = "fetch") -> [String: Any] {
        ["i": number, "p": ["kind": kind, "method": "GET", "url": url, "state": "pending", "started": 1000.0, "detail": "full"]]
    }

    func testAScriptsReportsBecomeOneRequest() {
        let log = NetLog()
        let first = log.apply(document: "a", messages: [start(1, "https://a.b/api")])
        XCTAssertEqual(first.count, 1)
        let id = first[0]
        XCTAssertEqual(log.summary(id)?["state"] as? String, "pending")
        let changed = log.apply(document: "a", messages: [
            ["i": 1, "p": ["status": 404, "response_headers": [["content-type", "application/json"], ["bad"], [1, 2]]]],
            ["i": 1, "p": ["state": "done", "duration": 12.5, "response_body": "{}"]],
        ])
        XCTAssertEqual(changed, [id])
        XCTAssertEqual(log.count, 1)
        let full = log.full(id)!
        XCTAssertEqual(full["status"] as? Int, 404)
        XCTAssertEqual(full["status_text"] as? String, "Not Found", "HTTP/2 sends no reason, so the usual one is supplied")
        XCTAssertEqual(full["response_headers"] as? [[String]], [["content-type", "application/json"]])
        XCTAssertEqual(full["response_body"] as? String, "{}")
        XCTAssertNil(log.summary(id)?["response_body"], "the list is not sent bodies")
    }

    func testTheSameNumberInAnotherDocumentIsAnotherRequest() {
        let log = NetLog()
        let a = log.apply(document: "a", messages: [start(1, "https://a.b/1")])
        let b = log.apply(document: "b", messages: [start(1, "https://a.b/2")])
        XCTAssertNotEqual(a, b)
        XCTAssertEqual(log.count, 2)
    }

    func testWhatAPageSaysIsCheckedAndCutToSize() {
        let log = NetLog()
        let id = log.apply(document: "a", messages: [["i": 1, "p": [
            "kind": "fetch", "url": "https://a.b/", "method": String(repeating: "X", count: 100),
            "status": "200", "size": -5, "duration": Double.infinity, "made_up": "x",
            "response_body": String(repeating: "y", count: NetLog.maxBody + 10),
        ]]])[0]
        let full = log.full(id)!
        XCTAssertEqual((full["method"] as? String)?.count, 16)
        XCTAssertNil(full["status"])
        XCTAssertNil(full["size"])
        XCTAssertNil(full["duration"])
        XCTAssertNil(full["made_up"])
        XCTAssertEqual((full["response_body"] as? String)?.count, NetLog.maxBody)
        // A report without an address starts nothing.
        XCTAssertEqual(log.apply(document: "a", messages: [["i": 2, "p": ["status": 200]]]), [])
    }

    func testWebKitsTimingIsKeptOverTheScripts() {
        let log = NetLog()
        let id = log.apply(document: "a", messages: [start(1, "https://a.b/api")])[0]
        _ = log.apply(document: "a", messages: [["i": 1, "t": true, "p": ["duration": 80.0, "size": 512]]])
        _ = log.apply(document: "a", messages: [["i": 1, "p": ["state": "done", "duration": 95.0]]])
        XCTAssertEqual(log.summary(id)?["duration"] as? Double, 80.0)
        XCTAssertEqual(log.summary(id)?["size"] as? Int, 512)
        XCTAssertEqual(log.summary(id)?["state"] as? String, "done")
    }

    func testADocumentsTimingJoinsTheRequestTheAppRecorded() {
        let log = NetLog()
        let id = log.add(["kind": "doc", "method": "GET", "url": "https://a.b/", "status": 200, "state": "pending", "detail": "document"])
        let changed = log.apply(document: "a", messages: [["i": 1, "t": true, "p": [
            "kind": "doc", "method": "GET", "url": "https://a.b/", "duration": 140.0, "size": 9000, "state": "done", "detail": "timing",
        ]]])
        XCTAssertEqual(changed, [id])
        XCTAssertEqual(log.count, 1)
        let full = log.full(id)!
        XCTAssertEqual(full["detail"] as? String, "document", "what the app saw is not replaced")
        XCTAssertEqual(full["size"] as? Int, 9000)
        // A page that was already loaded has only its timing to go by.
        let other = log.apply(document: "a", messages: [["i": 2, "t": true, "p": ["kind": "doc", "url": "https://c.d/", "duration": 5.0]]])
        XCTAssertEqual(log.count, 2)
        XCTAssertNotEqual(other, [id])
    }

    func testThePageBeingLeftIsNotHeardAfterAReset() {
        let log = NetLog()
        _ = log.apply(document: "old", messages: [start(1, "https://a.b/1")])
        log.reset()
        XCTAssertEqual(log.count, 0)
        XCTAssertEqual(log.apply(document: "old", messages: [start(2, "https://a.b/late")]), [])
        XCTAssertEqual(log.apply(document: "new", messages: [start(1, "https://a.b/2")]).count, 1)
        // Clearing by hand keeps listening to the same page.
        log.clear()
        XCTAssertEqual(log.apply(document: "new", messages: [start(2, "https://a.b/3")]).count, 1)
    }

    func testSocketMessagesAreKeptWithTheirSocket() {
        let log = NetLog()
        let id = log.apply(document: "a", messages: [start(1, "wss://a.b/live", kind: "ws")])[0]
        _ = log.apply(document: "a", messages: [["i": 1, "f": ["out": true, "data": "hi"]], ["i": 1, "f": ["out": false, "data": "ho"]]])
        let frames = log.full(id)?["frames"] as? [[String: Any]]
        XCTAssertEqual(frames?.count, 2)
        XCTAssertEqual(frames?[0]["out"] as? Bool, true)
        XCTAssertEqual(frames?[1]["data"] as? String, "ho")
    }

    func testTheOldestRequestsMakeWay() {
        let log = NetLog()
        let first = log.add(["url": "https://a.b/0"])
        for n in 1...NetLog.maxRecords { log.add(["url": "https://a.b/\(n)"]) }
        XCTAssertEqual(log.count, NetLog.maxRecords)
        XCTAssertNil(log.summary(first))
    }
}

final class JSONSafeTests: XCTestCase {
    func testOnlyWhatJSONCanCarryIsKept() {
        let value: [String: Any] = ["text": "a", "n": 1.5, "flag": true, "date": Date(), "nan": Double.nan,
                                    "list": ["b", Date(), ["deep": Data()]]]
        let safe = Wire.jsonSafe(value) as? [String: Any]
        XCTAssertEqual(safe?.keys.sorted(), ["flag", "list", "n", "text"])
        XCTAssertEqual((safe?["list"] as? [Any])?.count, 2)
        XCTAssertTrue(JSONSerialization.isValidJSONObject(safe as Any))
        // What is not JSON is answered with an empty object, not a crash.
        XCTAssertEqual(Wire.encode(["date": Date()]), "{}")
    }
}

final class SiteMatchTests: XCTestCase {
    func testAHostBelongsToTheSiteItEndsIn() {
        let names = ["example.com", "example.co.uk", "accounts.example.com"]
        XCTAssertEqual(SiteMatch.site(for: "www.example.com", among: names), "example.com")
        XCTAssertEqual(SiteMatch.site(for: "login.accounts.example.com", among: names), "accounts.example.com")
        XCTAssertEqual(SiteMatch.site(for: "shop.example.co.uk", among: names), "example.co.uk")
        XCTAssertEqual(SiteMatch.site(for: "notexample.com", among: names), "notexample.com")
        XCTAssertEqual(SiteMatch.site(for: "www.fresh.dev", among: names), "fresh.dev")
    }

    func testCookieDomainsMayBeginWithADot() {
        XCTAssertTrue(SiteMatch.belongs(".example.com", to: "example.com"))
        XCTAssertTrue(SiteMatch.belongs("a.b.Example.com", to: "example.com"))
        XCTAssertFalse(SiteMatch.belongs("badexample.com", to: "example.com"))
        XCTAssertFalse(SiteMatch.belongs("example.com", to: ""))
    }
}
