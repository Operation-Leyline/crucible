import Foundation
import XCTest
// The qualification script substitutes the generated module for each AWS JSON protocol.
@testable import __MODULE__

final class ProtocolTests: XCTestCase, @unchecked Sendable {
    func testAWSJSONProtocolOverHTTP() async throws {
        let protocolName = try XCTUnwrap(ProcessInfo.processInfo.environment["PROTOCOL"])
        let rpc = protocolName != "restJson1"
        let fixture = try ScriptedFixture()
        try await fixture.reset()
        let client = try JsonServiceClient(ClientConfig(endpoint: fixture.url))
        let instant = try Timestamp(seconds: -1, nanoseconds: 750_000_000)
        let renamed = rpc ? "renamed" : "wire-name"
        try await fixture.script(Scripted(body: """
            {"\(renamed)":"hello","when":-0.25,"iso":"1969-12-31T23:59:59.75Z","floatValue":"NaN","doubleValue":"Infinity",
             "choice":{"text":"ok","__type":"ignored"},"nullable":null,"document":null}
            """))
        let output = try await client.echo(ValuesInput(renamed: "hello", when: instant, iso: instant, date: instant, dates: [instant],
            query: instant, floatValue: .nan, doubleValue: .infinity, nullablePresence: .null, documentPresence: .null))
        var requests = try await fixture.requests()
        let request = try XCTUnwrap(requests.first)
        XCTAssertEqual(request.method, "POST")
        let body = request.json
        XCTAssertEqual(body["when"] as? Double, -0.25)
        XCTAssertEqual(body["iso"] as? String, "1969-12-31T23:59:59.75Z")
        XCTAssertNil(body["nullable"], "a structure null is omitted")
        XCTAssertTrue(body.keys.contains("document") && body["document"] is NSNull, "a document null is sent")
        XCTAssertEqual(body["floatValue"] as? String, "NaN")
        XCTAssertEqual(body["doubleValue"] as? String, "Infinity")
        if rpc {
            XCTAssertEqual(request.route, "/")
            XCTAssertEqual(request.headers["x-amz-target"], "JsonService.Echo")
            XCTAssertEqual(request.headers["content-type"], protocolName == "awsJson1_0" ? "application/x-amz-json-1.0" : "application/x-amz-json-1.1")
            XCTAssertEqual(body["renamed"] as? String, "hello")
            XCTAssertEqual(body["date"] as? Double, -0.25)
        } else {
            XCTAssertEqual(request.route, "/echo")
            XCTAssertNil(request.headers["x-amz-target"])
            XCTAssertEqual(request.headers["x-date"], "Wed, 31 Dec 1969 23:59:59 GMT")
            XCTAssertEqual(request.query["time"], "1969-12-31T23:59:59.75Z")
            XCTAssertEqual(body["wire-name"] as? String, "hello")
            XCTAssertNil(body["date"])
        }
        XCTAssertEqual(output.renamed, "hello")
        XCTAssertEqual(output.when, instant)
        XCTAssertEqual(output.iso, instant)
        XCTAssertFalse(output.nullablePresence.isSet, "a structure null is absent")
        XCTAssertEqual(output.documentPresence, .null)
        XCTAssertTrue(output.floatValue?.isNaN == true)
        XCTAssertEqual(output.doubleValue, .infinity)
        guard case .text(let text)? = output.choice else { return XCTFail("union type marker was not ignored") }
        XCTAssertEqual(text, "ok")

        try await fixture.script(Scripted(status: 409, headers: ["X-Amzn-Errortype": "example.json#Problem:detail",
            "X-Amzn-Requestid": "request-42"], body: "{\"__type\":\"Other\",\"Message\":\"bad\"}"))
        do { _ = try await client.echo(ValuesInput()); XCTFail("modeled failure succeeded") }
        catch let error as ProblemFault {
            XCTAssertEqual(error.metadata.name, "Problem")
            XCTAssertEqual(error.metadata.requestID, "request-42")
            XCTAssertEqual(error.metadata.message, "bad")
            XCTAssertEqual(error.metadata.status, 409)
        }
        try await fixture.script(Scripted(status: 400, body: "{\"code\":\"aws.example#Unmodeled\",\"message\":\"no\"}"))
        do { _ = try await client.echo(ValuesInput()); XCTFail("unmodeled failure succeeded") }
        catch let error as UnknownServiceError { XCTAssertEqual(error.metadata.name, "Unmodeled") }

        try await fixture.reset()
        try await fixture.script(Scripted(body: ""))
        _ = try await client.empty()
        requests = try await fixture.requests()
        if rpc {
            XCTAssertEqual(requests[0].bytes, Data("{}".utf8))
            XCTAssertEqual(requests[0].headers["x-amz-target"], "JsonService.Empty")
        } else {
            XCTAssertEqual(requests[0].route, "/empty")
        }
    }
}
