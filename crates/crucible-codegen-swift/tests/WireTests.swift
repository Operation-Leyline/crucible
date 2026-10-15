import Foundation
import XCTest
@testable import Wire

private struct WireTransport: HTTPTransport {
    let failure: Bool
    func send(_ request: HTTPRequest, readBody: Bool) async throws -> HTTPResponse {
        XCTAssertTrue(readBody)
        if request.url.path == "/document" {
            XCTAssertEqual(request.headers["content-type"], "application/json")
            XCTAssertEqual(try JSONValue(data: request.body!), .object(["name": .string("record")]))
            return HTTPResponse(status: 200, body: request.body!)
        }
        XCTAssertEqual(request.headers["content-type"], "text/plain")
        XCTAssertEqual(request.body, Data("héllo".utf8))
        XCTAssertEqual(request.headers["x-meta"], "eyJhIjoxfQ==, aGVsbG8=")
        if failure { return HTTPResponse(status: 422, headers: ["x-crucible-error": ["TextError"]], body: Data("invalid text".utf8)) }
        return HTTPResponse(status: 200, headers: ["x-meta": ["eyJhIjoxfQ==, aGVsbG8="]], body: request.body!)
    }
}
final class WireTests: XCTestCase, @unchecked Sendable {
    private let input = TextInput(text: "héllo", meta: ["{\"a\":1}", "hello"])
    func testPayloadsAndMediaHeaders() async throws {
        let client = try WireClient(ClientConfig(endpoint: "http://127.0.0.1", transport: WireTransport(failure: false)))
        let output = try await client.text(input)
        XCTAssertEqual(output.text, input.text); XCTAssertEqual(output.meta, input.meta)
        let record = try await client.document(DocumentInput(document: Record(name: "record")))
        XCTAssertEqual(record.document?.name, "record")
    }
    func testModeledRawErrorPayload() async throws {
        let client = try WireClient(ClientConfig(endpoint: "http://127.0.0.1", transport: WireTransport(failure: true)))
        do { _ = try await client.text(input); XCTFail("Expected TextError") }
        catch let error as TextError { XCTAssertEqual(error.details, "invalid text"); XCTAssertEqual(error.metadata.status, 422) }
    }
}
