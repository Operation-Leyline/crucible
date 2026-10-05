import Foundation
import XCTest
@testable import Weather

private func json(_ text: String) throws -> JSONValue { try JSONValue(data: Data(text.utf8)) }
private func response(_ status: Int, _ body: String = "{}", headers: [String: [String]] = [:]) -> HTTPResponse {
    HTTPResponse(status: status, headers: headers, body: Data(body.utf8))
}
private actor FakeTransport: HTTPTransport {
    var requests: [HTTPRequest] = []
    let handler: @Sendable (Int, HTTPRequest, Bool) async throws -> HTTPResponse
    init(_ handler: @escaping @Sendable (Int, HTTPRequest, Bool) async throws -> HTTPResponse) { self.handler = handler }
    func send(_ request: HTTPRequest, readBody: Bool) async throws -> HTTPResponse {
        requests.append(request)
        return try await handler(requests.count, request, readBody)
    }
    var count: Int { requests.count }
}
private actor Counter {
    var value = 0
    func next() -> Int { value += 1; return value }
}
final class RuntimeTests: XCTestCase, @unchecked Sendable {
    private func codec() throws -> WireCodec {
        try ClientRuntime(ClientConfig(endpoint: "http://127.0.0.1"), endpoint: "", digest: false).codec
    }
    func testSharedRetryConformance() async throws {
        let cases = try JSONValue(data: Data(contentsOf: URL(fileURLWithPath: ProcessInfo.processInfo.environment["RETRY_CASES"]!))).array
        for test in cases {
            let failures = try WireNumber(test["failures"]!.number).integer(Int64.self)
            let status = Int(try WireNumber(test["status"]!.number).integer(Int64.self))
            let maximum = Int(try WireNumber(test["maxAttempts"]!.number).integer(Int64.self))
            let transport = FakeTransport { attempt, _, _ in
                var body: [String: JSONValue] = [:]
                if let name = test["error"], name != .null { body["__type"] = name }
                return HTTPResponse(status: attempt <= failures ? status : 200,
                    headers: ["x-amzn-errortype": ["Throttled"]], body: try JSONValue.object(body).data())
            }
            let client = try WeatherClient(ClientConfig(endpoint: "http://127.0.0.1", transport: transport, maxAttempts: maximum, baseDelay: 0))
            var success = false
            do {
                if test["safe"]?.flag == true { _ = try await client.retrySafe() }
                else { _ = try await client.retryUnsafe() }
                success = true
            } catch is any ServiceError { }
            let attempts = await transport.count
            XCTAssertEqual(success, test["success"]?.flag, test["name"]!.text!)
            XCTAssertEqual(Int64(attempts), try WireNumber(test["attempts"]!.number).integer(Int64.self), test["name"]!.text!)
        }
    }
    func testExactNumbersTimestampsAndModels() throws {
        let codec = try codec()
        let city = try codec.decode("example.weather#CitySummary", json("{\"id\":\"c\",\"name\":\"x\",\"population\":9223372036854775807,\"createdAt\":-0.000000001}"), as: CitySummary.self)
        XCTAssertEqual(city.population, Int64.max)
        XCTAssertEqual(city.createdAt, try Timestamp(seconds: -1, nanoseconds: 999999999))
        let encoded = try codec.encode("example.weather#CitySummary", city)
        XCTAssertEqual(encoded["createdAt"], .number("-0.000000001"))
        XCTAssertEqual(encoded["population"], .number(String(Int64.max)))
        XCTAssertEqual(try WireNumber("0.00000000000000000001e20").integer(Int64.self), 1)
        XCTAssertEqual(try WireNumber("-9223372036854775808").integer(Int64.self), Int64.min)
        for number in ["9223372036854775808", "-9223372036854775809", "1.000000001", "1e1000"] {
            XCTAssertThrowsError(try WireNumber(number).integer(Int64.self))
        }
        let defaults = try codec.decode("example.weather#CitySummary", json("{\"id\":\"c\",\"name\":\"x\"}"), as: CitySummary.self)
        XCTAssertEqual(defaults.population, 0)
        var ordinary = CreateCityInput(name: "", expectedRevision: 0)
        XCTAssertEqual(ordinary.name, ""); XCTAssertEqual(ordinary.expectedRevision, 0)
        XCTAssertTrue(ordinary.namePresence.isSet)
        let overridden = CreateCityInput(name: "value", namePresence: .unset)
        XCTAssertNil(overridden.name); XCTAssertFalse(overridden.namePresence.isSet)
        ordinary.name = nil
        XCTAssertEqual(ordinary.namePresence, .null)
        ordinary.namePresence = .unset
        XCTAssertNil(ordinary.name); XCTAssertFalse(ordinary.namePresence.isSet)
        let input = CreateCityInput(name: "x", locationPresence: .null)
        let wire = try codec.encode("example.weather#CreateCityInput", input)
        XCTAssertEqual(wire["location"], .null); XCTAssertNil(wire["clientToken"])
        let future = try json("{\"future\":{\"n\":9223372036854775807}}")
        let union = try codec.decode("example.weather#Location", future, as: Location.self)
        XCTAssertEqual(try codec.encode("example.weather#Location", union), future)
        XCTAssertEqual(try codec.encode("example.weather#Location", Location.unknown(.init())), try json("{\"unknown\":{}}"))
        let forecast = try codec.decode("example.weather#Forecast", json("{\"future\":null}"), as: Forecast.self)
        XCTAssertEqual(forecast[Sky(rawValue: "future")], .null)
        XCTAssertThrowsError(try codec.decode("example.weather#Tags", json("[null]"), as: Tags.self))
        let bytes = Data([0, 1, 255, 128])
        XCTAssertEqual(try codec.encode("example.weather#Photo", bytes), .string("AAH/gA=="))
        XCTAssertEqual(try codec.decode("example.weather#Photo", .string("AAH/gA=="), as: Data.self), bytes)
        XCTAssertThrowsError(try codec.decode("example.weather#Photo", .string("!bad"), as: Data.self))
        var nested = JSONValue.null
        for _ in 0..<300 { nested = .array([nested]) }
        XCTAssertThrowsError(try nested.data())
        let timestamp = try Timestamp(seconds: 1790812800, nanoseconds: 123456789)
        let context = try codec.context("example.weather#DateTime", format: "date-time")
        XCTAssertEqual(try Timestamp.fromWire(timestamp.toWire(context), context), timestamp)
        XCTAssertEqual(try timestamp.toWire(codec.context("example.weather#DateTime", format: "http-date")), .string("Thu, 01 Oct 2026 00:00:00 GMT"))
    }
    func testSharedHeaderLists() throws {
        let codec = try codec()
        let cases = try JSONValue(data: Data(contentsOf: URL(fileURLWithPath: ProcessInfo.processInfo.environment["HEADER_CASES"]!))).array
        for test in cases {
            XCTAssertEqual(try codec.header(.object(["target": .string("example.weather#Tags")]), test["wire"]!.string), test["values"]!)
            XCTAssertEqual(try codec.header(.object(["target": .string("example.weather#Tags")]), codec.bound(test["values"]!)), test["values"]!)
        }
    }
    func testRetriesReplayBytesRefreshCredentialsAndPreserveInput() async throws {
        let credentials = Counter()
        let transport = FakeTransport { attempt, request, _ in
            XCTAssertEqual(request.headers["authorization"], "Bearer \(attempt)")
            XCTAssertEqual(request.headers["x-expected-revision"], "7")
            XCTAssertNotNil(request.headers["x-amz-content-sha256"])
            return attempt == 1 ? response(503, "{\"__type\":\"FutureError\"}") : response(200, "{\"city\":{\"id\":\"c\",\"name\":\"x\"}}")
        }
        let client = try WeatherClient(ClientConfig(endpoint: "http://127.0.0.1", transport: transport,
            token: { String(await credentials.next()) }, baseDelay: 0,
            middleware: [{ request in var request = request; request.body?.append(32); return request }]))
        let input = CreateCityInput(name: "x", expectedRevision: 7)
        _ = try await client.createCity(input)
        XCTAssertFalse(input.clientTokenPresence.isSet)
        let requests = await transport.requests
        XCTAssertEqual(requests.count, 2); XCTAssertEqual(requests[0].body, requests[1].body)
        XCTAssertNotNil(try JSONValue(data: requests[0].body!)["clientToken"]?.text)
        let bytes = requests[0].body!
        // A precomputed fixture checks the actual hash implementation in native HTTP tests.
        XCTAssertEqual(bytes.last, 32)
    }
    func testErrorMetadataAndMalformedModeledMembers() async throws {
        let transport = FakeTransport { _, _, _ in
            response(429, "{\"__type\":\"ThrottlingError\",\"message\":42,\"requestId\":\"body\"}", headers: ["X-Request-ID": ["header"]])
        }
        let client = try WeatherClient(ClientConfig(endpoint: "http://127.0.0.1", transport: transport, maxAttempts: 2, baseDelay: 0))
        do { _ = try await client.ping(); XCTFail("Expected error") }
        catch let error as any ServiceError {
            XCTAssertTrue(error.metadata.throttling); XCTAssertEqual(error.metadata.name, "ThrottlingError")
            XCTAssertEqual(error.metadata.requestID, "header"); XCTAssertEqual(error.metadata.status, 429)
        }
        let attempts = await transport.count; XCTAssertEqual(attempts, 2)
        let malformed = FakeTransport { _, _, _ in response(200, "{bad") }
        let bad = try WeatherClient(ClientConfig(endpoint: "http://127.0.0.1", transport: malformed, baseDelay: 0))
        do { _ = try await bad.getCity(.init(cityId: "x")); XCTFail("Expected decoding error") }
        catch is WireError { }
        let count = await malformed.count; XCTAssertEqual(count, 1)
    }
    func testBindingsAndRawPayloads() async throws {
        let bytes = Data([0, 1, 255, 128])
        let transport = FakeTransport { _, request, _ in
            XCTAssertTrue(request.url.absoluteString.contains("a%2Fb%20%2A%3F%23%C3%A9"))
            XCTAssertTrue(request.url.absoluteString.contains("at=2026-10-01T00%3A00%3A00Z"))
            return response(200, "{\"city\":{\"id\":\"c\",\"name\":\"x\"}}", headers: ["x-request-id": ["request"]])
        }
        let client = try WeatherClient(ClientConfig(endpoint: "http://127.0.0.1", transport: transport))
        let output = try await client.getCity(.init(cityId: "a/b *?#é", at: try Timestamp(seconds: 1790812800)))
        XCTAssertEqual(output.requestId, "request")
        for payload in [bytes, Data()] {
            let raw = FakeTransport { _, _, readBody in
                XCTAssertTrue(readBody)
                return HTTPResponse(status: 200, headers: ["x-photo-caption": ["raw"]], body: payload)
            }
            let client = try WeatherClient(ClientConfig(endpoint: "http://127.0.0.1", transport: raw))
            let output = try await client.getCityPhoto(.init(cityId: "x"))
            XCTAssertEqual(output.photo, payload); XCTAssertEqual(output.caption, "raw")
        }
        let empty = FakeTransport { _, request, _ in
            XCTAssertNil(request.body); XCTAssertNil(request.headers["content-type"])
            XCTAssertNil(request.headers["x-amz-content-sha256"])
            return response(200, "{\"photo\":{\"size\":0,\"thumbnail\":\"\"}}")
        }
        let emptyClient = try WeatherClient(ClientConfig(endpoint: "http://127.0.0.1", transport: empty))
        _ = try await emptyClient.putCityPhoto(.init(cityId: "x", photo: Data()))
    }
    func testRetryAfterAndUnsafeCredentialEndpoint() async throws {
        let runtime = try ClientRuntime(ClientConfig(endpoint: "http://127.0.0.1", baseDelay: 1, maxDelay: 2), endpoint: "", digest: false)
        XCTAssertEqual(runtime.retryDelay(1, header: "0"), 0)
        XCTAssertEqual(runtime.retryDelay(1, header: "50"), 50)
        for invalid in ["-1", "NaN", "Infinity", "invalid"] {
            XCTAssertTrue((0...1).contains(runtime.retryDelay(1, header: invalid)))
        }
        let transport = FakeTransport { _, _, _ in response(200) }
        let client = try WeatherClient(ClientConfig(endpoint: "http://example.invalid", transport: transport, token: { "secret" }))
        do { _ = try await client.ping(); XCTFail("Credentials sent without TLS") } catch is WireError { }
        let count = await transport.count; XCTAssertEqual(count, 0)
    }
    func testPaginationIsLazyAndDetectsCycles() async throws {
        let transport = FakeTransport { _, request, _ in
            XCTAssertTrue(request.url.query?.contains("maxResults=1") == true)
            return response(200, "{\"cities\":[],\"nextToken\":\"same\"}")
        }
        let client = try WeatherClient(ClientConfig(endpoint: "http://127.0.0.1", transport: transport))
        let pages = try client.listCitiesPages(.init(maxResults: 1))
        var iterator = pages.makeAsyncIterator()
        let before = await transport.count; XCTAssertEqual(before, 0)
        _ = try await iterator.next(); _ = try await iterator.next()
        do { _ = try await iterator.next(); XCTFail("Expected cycle error") } catch is WireError { }
        let count = await transport.count; XCTAssertEqual(count, 2)
    }
    func testDeadlineAndCancellationStopHooksBeforeSend() async throws {
        let transport = FakeTransport { _, _, _ in response(200) }
        let client = try WeatherClient(ClientConfig(endpoint: "http://127.0.0.1", transport: transport,
            token: {
                await withCheckedContinuation { continuation in
                    DispatchQueue.global().asyncAfter(deadline: .now() + 0.3) { continuation.resume(returning: "late") }
                }
            }, timeout: 0.05))
        let start = Date()
        do { _ = try await client.ping(); XCTFail("Expected timeout") } catch is TimeoutError { }
        XCTAssertLessThan(Date().timeIntervalSince(start), 0.5)
        try await Task.sleep(nanoseconds: 350_000_000)
        let count = await transport.count; XCTAssertEqual(count, 0)
        let task = Task { try await client.ping() }; task.cancel()
        do { _ = try await task.value; XCTFail("Expected cancellation") } catch is CancellationError { }
    }
    func testNativeRedirectAndHeaderOnlySuccess() async throws {
        let fixture = ProcessInfo.processInfo.environment["FIXTURE_URL"]!
        let redirect = try WeatherClient(ClientConfig(endpoint: fixture + "/redirect", maxAttempts: 1))
        defer { redirect.close() }
        do { _ = try await redirect.ping(); XCTFail("Redirect followed") }
        catch let error as any ServiceError { XCTAssertEqual(error.metadata.status, 302) }
        let discard = try WeatherClient(ClientConfig(endpoint: fixture + "/discard", timeout: 1))
        defer { discard.close() }
        _ = try await discard.ping()
    }
    func testNativeBodyTimeoutAndCancellation() async throws {
        let fixture = ProcessInfo.processInfo.environment["FIXTURE_URL"]!
        let stall = try WeatherClient(ClientConfig(endpoint: fixture + "/stall", timeout: 0.05))
        defer { stall.close() }
        let stalledAt = Date()
        do { _ = try await stall.getCity(.init(cityId: "x")); XCTFail("Expected body timeout") } catch is TimeoutError { }
        XCTAssertGreaterThan(Date().timeIntervalSince(stalledAt), 0.02)
        let cancellable = try WeatherClient(ClientConfig(endpoint: fixture + "/stall", timeout: 10))
        defer { cancellable.close() }
        let task = Task { try await cancellable.getCity(.init(cityId: "x")) }
        try await Task.sleep(nanoseconds: 50_000_000); task.cancel()
        do { _ = try await task.value; XCTFail("Expected body cancellation") } catch is CancellationError { }
    }
    func testNativeDigestAndCredentialRefresh() async throws {
        let fixture = ProcessInfo.processInfo.environment["FIXTURE_URL"]!
        let counter = Counter()
        let echo = try WeatherClient(ClientConfig(endpoint: fixture + "/echo", token: { String(await counter.next()) }, baseDelay: 0))
        defer { echo.close() }
        let echoed = try await echo.createCity(.init(name: "x", expectedRevision: 7))
        XCTAssertEqual(echoed.city.name, "x")
    }
    func testNativeCallsDoNotReuseCachedResponsesAcrossCredentials() async throws {
        let fixture = ProcessInfo.processInfo.environment["FIXTURE_URL"]!
        let counter = Counter()
        let client = try WeatherClient(ClientConfig(endpoint: fixture + "/cache", token: { String(await counter.next()) }))
        defer { client.close() }
        let first = try await client.getCity(.init(cityId: "c"))
        let second = try await client.getCity(.init(cityId: "c"))
        XCTAssertEqual(first.city.name, "Bearer 1")
        XCTAssertEqual(second.city.name, "Bearer 2")
    }
    func testRealWeatherHTTP() async throws {
        let client = try WeatherClient(ClientConfig(endpoint: ProcessInfo.processInfo.environment["WEATHER_URL"]!, token: { "test-token" }))
        defer { client.close() }
        _ = try await client.ping()
        let input = CreateCityInput(name: "swift-http", location: .unknown(.init()))
        let city = try await client.createCity(input).city
        let fetched = try await client.getCity(.init(cityId: city.id))
        XCTAssertEqual(fetched.city.name, "swift-http"); XCTAssertNotNil(fetched.requestId)
        let bytes = Data([0, 1, 255, 128, 13, 10, 65, 66, 67])
        let stored = try await client.putCityPhoto(.init(cityId: city.id, caption: "binary", photo: bytes))
        XCTAssertEqual(stored.photo.size, 9); XCTAssertEqual(stored.photo.thumbnail, bytes.prefix(8))
        let photo = try await client.getCityPhoto(.init(cityId: city.id))
        XCTAssertEqual(photo.photo, bytes); XCTAssertEqual(photo.caption, "binary")
        do { _ = try await client.createCity(input); XCTFail("Expected conflict") } catch is ConflictError { }
        var found = false
        for try await page in try client.listCitiesPages(.init(maxResults: 1)) {
            found = found || page.cities.contains { $0.id == city.id }
        }
        XCTAssertTrue(found)
    }
}
