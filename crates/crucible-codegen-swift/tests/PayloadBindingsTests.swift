import Foundation
import XCTest
@testable import Payload

private actor Recorder: HTTPTransport {
    private(set) var requests: [HTTPRequest] = []
    func send(_ request: HTTPRequest, readBody: Bool) async throws -> HTTPResponse {
        requests.append(request)
        return HTTPResponse(status: 200)
    }
}
/// A payload's Content-Type is the modelled member if set, then the payload's
/// media type, then a client-wide default: exactly one value either way.
/// Swift has no streaming request payloads, so only the buffered upload runs.
final class PayloadBindingsTests: XCTestCase, @unchecked Sendable {
    func testPayloadContentTypePrefersModeledHeaderThenMediaTypeThenConfig() async throws {
        for contentType in [nil, "image/png"] as [String?] {
            for configured in [nil, "image/jpeg"] as [String?] {
                let label = "\(contentType ?? "-")/\(configured ?? "-")"
                let recorder = Recorder()
                let headers = configured.map { ["Content-Type": $0] } ?? [:]
                let client = try PayloadBindingsClient(ClientConfig(endpoint: "https://example.invalid", transport: recorder, headers: headers))
                _ = try await client.upload(UploadInput(data: Data("PNG".utf8), contentType: contentType))
                let requests = await recorder.requests
                XCTAssertEqual(requests.count, 1, label)
                XCTAssertEqual(requests.first?.body, Data("PNG".utf8), label)
                let values = requests.first?.headers.filter { $0.key.lowercased() == "content-type" }.map { $0.value } ?? []
                XCTAssertEqual(values, [contentType ?? "application/octet-stream"], label)
            }
        }
    }
}
