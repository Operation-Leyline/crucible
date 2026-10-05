import Foundation
import XCTest
@testable import Jobs

final class WaiterTests: XCTestCase, @unchecked Sendable {
    private let fast = WaiterOptions(minDelay: 0, maxDelay: 0)
    private let input = Input(names: ["a", "b"])

    func testWaitersOverHTTP() async throws {
        let fixture = try ScriptedFixture()
        try await fixture.reset()
        let client = try JobsClient(ClientConfig(endpoint: fixture.url, maxAttempts: 1))

        try await fixture.script(Scripted(status: 404, body: "{\"__type\":\"Missing\"}"),
                                 Scripted(body: "{\"wire-status\":\"PENDING\"}"),
                                 Scripted(body: "{\"wire-status\":\"READY\"}"))
        let ready = try await client.waitUntilReady(input, timeout: 5, options: fast)
        XCTAssertEqual(ready?.status, "READY")
        var requests = try await fixture.requests()
        XCTAssertEqual(requests.count, 3)
        XCTAssertEqual(requests[0].route, "/read")
        XCTAssertEqual(requests[0].json["names"] as? [String], ["a", "b"])

        try await fixture.script(Scripted(status: 404, body: "{\"__type\":\"example.wait#Missing\"}"))
        let deleted = try await client.waitUntilDeleted(input, timeout: 5, options: fast)
        XCTAssertNil(deleted, "an error-type success state has no output")

        let items = "{\"items\":[{\"wire-status\":\"READY\"},{\"wire-status\":\"READY\"}],\"updatedAt\":\"2026-01-01T00:00:00Z\"}"
        let waits: [(String, @Sendable (Input) async throws -> Output?)] = [
            ("AllReady", { try await client.waitUntilAllReady($0, timeout: 5, options: self.fast) }),
            ("AnyReady", { try await client.waitUntilAnyReady($0, timeout: 5, options: self.fast) }),
            ("FilteredReady", { try await client.waitUntilFilteredReady($0, timeout: 5, options: self.fast) }),
            ("CountsMatch", { try await client.waitUntilCountsMatch($0, timeout: 5, options: self.fast) }),
            ("Updated", { try await client.waitUntilUpdated($0, timeout: 5, options: self.fast) }),
            ("Functions", { try await client.waitUntilFunctions($0, timeout: 5, options: self.fast) }),
        ]
        for (name, wait) in waits {
            try await fixture.script(Scripted(body: items))
            let output = try await wait(input)
            XCTAssertEqual(output?.items?.count, 2, name)
        }
        requests = try await fixture.requests()
        XCTAssertEqual(requests.count, 7, "each query waiter succeeds on its first attempt")

        try await fixture.script(Scripted(body: "{\"wire-status\":\"FAILED\",\"reasons\":[\"job rejected\"]}"))
        do { _ = try await client.waitUntilReady(input, timeout: 5, options: fast); XCTFail("failure state succeeded") }
        catch let error as WaiterError {
            XCTAssertEqual(error.attempts, 1)
            XCTAssertEqual(error.message, "job rejected")
            XCTAssertTrue(error.description.contains("job rejected"))
        }

        try await fixture.reset()
        try await fixture.script(Scripted(body: "{\"wire-status\":\"PENDING\"}"),
                                 Scripted(body: "{\"wire-status\":\"READY\"}"))
        let finalReady = try await client.waitUntilReady(input, timeout: 1.5, options: WaiterOptions(minDelay: 1, maxDelay: 1))
        XCTAssertEqual(finalReady?.status, "READY")
        requests = try await fixture.requests()
        XCTAssertEqual(requests.count, 2, "final poll retains a call budget")

        // The default one-second delay never fits, so the deadline ends the wait.
        try await fixture.reset()
        let started = Date()
        do { _ = try await client.waitUntilReady(input, timeout: 0.2); XCTFail("pending waiter succeeded") }
        catch let error as WaiterTimeoutError { XCTAssertGreaterThanOrEqual(error.attempts, 1) }
        XCTAssertLessThan(Date().timeIntervalSince(started), 2)

        let cancelled = Task { try await client.waitUntilReady(self.input, timeout: 5) }
        cancelled.cancel()
        do { _ = try await cancelled.value; XCTFail("cancelled waiter succeeded") } catch is CancellationError {}

        try await fixture.script(Scripted(block: true))
        let blockedStart = Date()
        do { _ = try await client.waitUntilReady(input, timeout: 0.2); XCTFail("blocked waiter succeeded") }
        catch is WaiterTimeoutError {}
        XCTAssertLessThan(Date().timeIntervalSince(blockedStart), 2, "the deadline cancels an in-flight attempt")

        do { _ = try await client.waitUntilReady(input, timeout: 1, options: WaiterOptions(minDelay: 2, maxDelay: 1)); XCTFail("unordered delays") }
        catch is WireError {}
        do { _ = try await client.waitUntilReady(input, timeout: 0); XCTFail("nonpositive timeout") } catch is WireError {}
    }

    func testCompiledQueryEvaluator() throws {
        func number(_ text: String) -> JSONValue { .number(text) }
        let field: JSONValue = .object(["kind": .string("field"), "name": .string("a")])
        XCTAssertEqual(try JMESPath.evaluate(field, .object(["a": number("1")])), number("1"))
        let sum: JSONValue = .object(["kind": .string("function"), "function": .string("sum"),
            "arguments": .array([.object(["kind": .string("literal"), "value": .array([number("0.1"), number("0.2")])])])])
        XCTAssertEqual(try JMESPath.evaluate(sum, .null), number("0.3"), "decimal arithmetic is exact")
        let avg: JSONValue = .object(["kind": .string("function"), "function": .string("avg"),
            "arguments": .array([.object(["kind": .string("literal"), "value": .array([number("1"), number("2"), number("2")])])])])
        guard case .number(let mean) = try JMESPath.evaluate(avg, .null) else { return XCTFail("avg") }
        XCTAssertTrue(mean.hasPrefix("1.666666666666666666666666666666666"), mean)
        let compare: JSONValue = .object(["kind": .string("compare"), "comparator": .string("lessThan"),
            "left": .object(["kind": .string("literal"), "value": number("1e400")]),
            "right": .object(["kind": .string("literal"), "value": number("2e400")])])
        XCTAssertEqual(try JMESPath.evaluate(compare, .null), .bool(true))
        let mixed: JSONValue = .object(["kind": .string("compare"), "comparator": .string("lessThan"),
            "left": .object(["kind": .string("literal"), "value": .string("a")]),
            "right": .object(["kind": .string("literal"), "value": number("1")])])
        XCTAssertEqual(try JMESPath.evaluate(mixed, .null), .null)
    }
    func testExtremeExponentFailsWithoutTrapping() throws {
        let extreme = "1e\(Int.min)"
        let value = try JSONValue(data: Data(extreme.utf8))
        XCTAssertEqual(value, .number(extreme), "valid JSON syntax reaches query precision validation")
        let literal: JSONValue = .object(["kind": .string("literal"), "value": value])
        let compare: JSONValue = .object(["kind": .string("compare"), "comparator": .string("lessThan"),
            "left": literal, "right": .object(["kind": .string("literal"), "value": .number("1")])])
        XCTAssertThrowsError(try JMESPath.evaluate(compare, .null)) { error in
            XCTAssertTrue(error is WireError)
        }
        for function in ["abs", "sum", "avg", "sort"] {
            let argument: JSONValue = function == "abs" ? value : .array([value])
            let query: JSONValue = .object(["kind": .string("function"), "function": .string(function),
                "arguments": .array([.object(["kind": .string("literal"), "value": argument])])])
            XCTAssertThrowsError(try JMESPath.evaluate(query, .null)) { error in
                XCTAssertTrue(error is WireError)
            }
        }
        let conversion: JSONValue = .object(["kind": .string("function"), "function": .string("to_number"),
            "arguments": .array([.object(["kind": .string("literal"), "value": .string(extreme)])])])
        XCTAssertEqual(try JMESPath.evaluate(conversion, .null), .null, "try? catches the precision error")
    }
}
