import Foundation
import XCTest
@testable import Dynamic

private struct DynamicTransport: HTTPTransport {
    let response: HTTPResponse
    func send(_ request: HTTPRequest, readBody: Bool) async throws -> HTTPResponse {
        XCTAssertEqual(request.method, "POST")
        XCTAssertEqual(request.url.absoluteString, "http://127.0.0.1/files/folder/a%20b?empty=&tag=hello%20world&tag=%2F")
        XCTAssertEqual(request.headers["x-meta-trace"], "abc")
        XCTAssertEqual(request.headers["content-type"], "application/json")
        XCTAssertEqual(try JSONValue(data: request.body!), .object(["wire-value": .string("hello")]))
        XCTAssertTrue(readBody)
        return response
    }
}

final class DynamicTests: XCTestCase, @unchecked Sendable {
    private var input: Input {
        Input(key: "folder/a b", query: ["tag": ["hello world", "/"], "empty": [""]], metadataValue: ["trace": "abc"], value: "hello")
    }
    func testDynamicBindings() async throws {
        let transport = DynamicTransport(response: HTTPResponse(status: 206, headers: ["X-Meta-Trace": ["abc"]], body: Data("{\"wire-value\":\"result\",\"value\":\"ignored\"}".utf8)))
        let client = try DynamicClient(ClientConfig(endpoint: "http://127.0.0.1", transport: transport))
        let output = try await client.get(input)
        XCTAssertEqual(output.status, 206)
        XCTAssertEqual(output.metadataValue, ["trace": "abc"])
        XCTAssertEqual(output.value, "result")
    }
    func testModeledErrorBindings() async throws {
        let transport = DynamicTransport(response: HTTPResponse(status: 422, headers: ["x-meta-reason": ["invalid"], "x-crucible-error": ["Problem"]], body: Data("{}".utf8)))
        let client = try DynamicClient(ClientConfig(endpoint: "http://127.0.0.1", transport: transport))
        do { _ = try await client.get(input); XCTFail("Expected modeled error") }
        catch let error as Problem {
            XCTAssertEqual(error.status, 422)
            XCTAssertEqual(error.details, ["reason": "invalid"])
        }
    }
    func testGreedyDotSegmentsRejected() async throws {
        let client = try DynamicClient(ClientConfig(endpoint: "http://127.0.0.1", transport: DynamicTransport(response: HTTPResponse(status: 200))))
        for key in ["a/../b", "./a", "a/."] {
            do { _ = try await client.get(Input(key: key)); XCTFail("Dot segment accepted") }
            catch is WireError { }
        }
    }
}
