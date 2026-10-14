import Foundation
import XCTest
@testable import Behavior

private actor Recorder: HTTPTransport {
    private(set) var requests: [HTTPRequest] = []
    func send(_ request: HTTPRequest, readBody: Bool) async throws -> HTTPResponse {
        requests.append(request)
        return HTTPResponse(status: 200, body: Data("{\"wire-status\":\"READY\"}".utf8))
    }
}

final class EndpointTests: XCTestCase, @unchecked Sendable {
    private func config(_ transport: Recorder, region: String = "local", disableHostPrefix: Bool = false) throws -> ClientConfig {
        ClientConfig(transport: transport,
                     credentials: StaticCredentialsProvider(try AWSCredentials(accessKeyID: "key", secretAccessKey: "secret")),
                     region: region, disableHostPrefix: disableHostPrefix)
    }

    func testGeneratedEndpointRulesAndHostPrefix() async throws {
        let transport = Recorder()
        let client = try ServiceClient(config(transport))
        let output = try await client.read(Input(account: "customer"))
        XCTAssertEqual(output.status, "READY")
        var requests = await transport.requests
        let request = try XCTUnwrap(requests.first)
        XCTAssertEqual(request.url.absoluteString, "https://customer.local.example.test/resource")
        XCTAssertEqual(request.headers["x-region"], "local")
        let authorization = try XCTUnwrap(request.headers["authorization"])
        XCTAssertTrue(authorization.contains("/local/example/aws4_request"), authorization)
        XCTAssertEqual(request.headers["x-amz-content-sha256"], "UNSIGNED-PAYLOAD")

        for label in ["a.b", "-bad", "x/y", "x@evil", "", String(repeating: "x", count: 64)] {
            do { _ = try await client.read(Input(account: label)); XCTFail("invalid host label \(label)") } catch is WireError {}
        }
        requests = await transport.requests
        XCTAssertEqual(requests.count, 1, "an invalid host is never dispatched")

        let unprefixed = try ServiceClient(config(transport, disableHostPrefix: true))
        _ = try await unprefixed.read(Input(account: "ignored"))
        requests = await transport.requests
        XCTAssertEqual(requests.last?.url.host, "local.example.test")

        // The rule spells its header `X-Region` (test-swift.py): it outranks
        // a client default of the same name in any case, and one is sent.
        var defaulted = try config(transport)
        defaulted.headers = ["x-region": "client"]
        _ = try await ServiceClient(defaulted).read(Input(account: "customer"))
        requests = await transport.requests
        XCTAssertEqual(requests.last?.headers["x-region"], "local")
        XCTAssertNil(requests.last?.headers["X-Region"])

        let public_ = try await unprefixed.`public`()
        _ = public_
        requests = await transport.requests
        XCTAssertNil(requests.last?.headers["authorization"], "an optional-auth operation with no schemes is unsigned")

        let missing = try ServiceClient(config(transport, region: "missing"))
        do { _ = try await missing.read(Input(account: "customer")); XCTFail("unknown region resolved") }
        catch let error as WireError { XCTAssertTrue(error.description.contains("Unknown region missing"), error.description) }

        // A built-in override supplies the region; the rule's auth properties supply the signing region.
        let override = try ServiceClient(ClientConfig(transport: transport,
            credentials: StaticCredentialsProvider(try AWSCredentials(accessKeyID: "key", secretAccessKey: "secret")),
            endpointBuiltIns: ["AWS::Region": .string("local")]))
        _ = try await override.read(Input(account: "other"))
        requests = await transport.requests
        XCTAssertEqual(requests.last?.url.host, "other.local.example.test")
        XCTAssertTrue(requests.last?.headers["authorization"]?.contains("/local/example/aws4_request") == true)
        XCTAssertThrowsError(try ServiceClient(ClientConfig(transport: transport, endpointParameters: ["Bad": .number("1")])))
    }

    func testEndpointStandardFunctions() throws {
        for value in ["file:///path", "https://user:pass@example.test", "https://example.test?q=x", "https://example.test#x",
                      "https://example.test:99999", "https://evil\n.example", "https://bad_host.example"] {
            XCTAssertNil(Endpoints.parseURL(value), value)
        }
        XCTAssertEqual(Endpoints.parseURL("https://example.test:443/a")?["authority"], .string("example.test"))
        XCTAssertEqual(Endpoints.parseURL("https://example.test:443/a")?["normalizedPath"], .string("/a/"))
        XCTAssertEqual(Endpoints.parseURL("https://[::1]:8443")?["authority"], .string("[::1]:8443"))
        XCTAssertEqual(Endpoints.parseURL("http://127.0.0.1")?["isIp"], .bool(true))
        XCTAssertEqual(try Endpoints.function("substring", [.string("abcdef"), .number("1"), .number("3"), .bool(true)]), .string("de"))
        XCTAssertEqual(try Endpoints.function("substring", [.string("abcdef"), .number("1"), .number("3"), .bool(false)]), .string("bc"))
        XCTAssertEqual(try Endpoints.function("substring", [.string("é"), .number("0"), .number("1"), .bool(false)]), .null)
        XCTAssertEqual(try Endpoints.function("isSet", [.bool(false)]), .bool(true))
        XCTAssertEqual(try Endpoints.function("isSet", [.null]), .bool(false))
        XCTAssertEqual(try Endpoints.function("uriEncode", [.string("a/b +é")]), .string("a%2Fb%20%2B%C3%A9"))
        XCTAssertEqual(try Endpoints.function("isValidHostLabel", [.string("a.b"), .bool(true)]), .bool(true))
        XCTAssertEqual(try Endpoints.function("isValidHostLabel", [.string("a.b"), .bool(false)]), .bool(false))
        XCTAssertEqual(try Endpoints.function("getAttr", [.object(["a": .array([.string("x"), .string("y")])]), .string("a[1]")]), .string("y"))
        XCTAssertEqual(try Endpoints.function("getAttr", [.object(["a": .array([])]), .string("a[3]")]), .null)
        XCTAssertThrowsError(try Endpoints.function("not", [.string("x")]))
    }
}
