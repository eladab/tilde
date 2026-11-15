import XCTest
@testable import WeftCore

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
