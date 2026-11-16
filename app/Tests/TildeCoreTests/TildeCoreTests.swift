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
