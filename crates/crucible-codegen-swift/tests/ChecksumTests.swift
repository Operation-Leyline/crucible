import Foundation
import XCTest
@testable import ChecksumSDK

private struct Vectors: Decodable {
    let inputUtf8: String
    let base64: [String: String]
    let emptyMd5: String
}

private final class Validated: @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String] = []
    func append(_ value: String) { lock.lock(); values.append(value); lock.unlock() }
    var all: [String] { lock.lock(); defer { lock.unlock() }; return values }
}

final class ChecksumTests: XCTestCase, @unchecked Sendable {
    private func vectors() throws -> Vectors {
        let path = try XCTUnwrap(ProcessInfo.processInfo.environment["CHECKSUM_CASES"])
        return try JSONDecoder().decode(Vectors.self, from: Data(contentsOf: URL(fileURLWithPath: path)))
    }

    func testPublishedDigestVectors() throws {
        let vectors = try vectors()
        XCTAssertEqual(Set(vectors.base64.keys), Set(Checksums.supported))
        for (algorithm, expected) in vectors.base64 {
            XCTAssertEqual(try Checksums.digest(algorithm, Data(vectors.inputUtf8.utf8)), expected, algorithm)
        }
        XCTAssertEqual(try Checksums.digest("MD5", Data()), vectors.emptyMd5)
        XCTAssertThrowsError(try Checksums.digest("XXHASH64", Data()))
    }

    func testChecksumsOverHTTP() async throws {
        let vectors = try vectors()
        let fixture = try ScriptedFixture()
        try await fixture.reset()
        let payload = Data(vectors.inputUtf8.utf8)
        let validated = Validated()
        let client = try ChecksumsClient(ClientConfig(endpoint: fixture.url, checksumValidated: { validated.append("\($0):\($1)") }))
        func respond(_ algorithm: String = "CRC32", value: String? = nil, extra: [String: String] = [:]) async throws {
            var headers = extra
            headers["x-amz-checksum-" + algorithm.lowercased()] = value ?? vectors.base64[algorithm]!
            headers["x-amz-checksum-xxhash3"] = "unmodeled"
            try await fixture.script(Scripted(headers: headers, body: vectors.inputUtf8))
        }
        let input = Input(data: payload)

        try await respond(); _ = try await client.legacy(input)
        try await respond(); _ = try await client.required(input)
        try await respond(); _ = try await client.legacy(Input())
        var requests = try await fixture.requests()
        XCTAssertEqual(requests.map { $0.headers["content-md5"] }, [vectors.base64["MD5"], vectors.base64["MD5"], vectors.emptyMd5])
        XCTAssertEqual(validated.all, [], "operations without validation modes do not validate responses")

        try await respond()
        let output = try await client.flexible(input)
        XCTAssertEqual(output.data, payload)
        requests = try await fixture.requests()
        XCTAssertEqual(requests[0].headers["x-algorithm"], "CRC32")
        XCTAssertEqual(requests[0].headers["x-validation"], "ENABLED")
        XCTAssertEqual(requests[0].headers["x-amz-checksum-crc32"], vectors.base64["CRC32"])
        XCTAssertNil(input.algorithm, "defaults are applied to a private copy")
        XCTAssertEqual(validated.all.last, "Flexible:CRC32")

        for algorithm in vectors.base64.keys.sorted() {
            try await respond(algorithm)
            _ = try await client.flexible(Input(data: payload, algorithm: Algorithm(rawValue: algorithm)))
            requests = try await fixture.requests()
            XCTAssertEqual(requests[0].headers["x-amz-checksum-" + algorithm.lowercased()], vectors.base64[algorithm], algorithm)
            XCTAssertEqual(validated.all.last, "Flexible:" + algorithm)
        }

        try await respond(value: "AAAA")
        do { _ = try await client.flexible(input); XCTFail("corruption accepted") }
        catch let error as ChecksumMismatchError { XCTAssertEqual(error.algorithm, "CRC32") }

        try await respond(extra: ["x-amz-checksum-sha256": "AAAA"])
        _ = try await client.flexible(input)
        XCTAssertEqual(validated.all.last, "Flexible:CRC32", "the first modeled algorithm present is validated")

        let count = validated.all.count
        try await fixture.script(Scripted(headers: ["x-amz-checksum-xxhash3": "unmodeled"], body: vectors.inputUtf8))
        _ = try await client.flexible(input)
        XCTAssertEqual(validated.all.count, count, "an unmodeled checksum is not validated")

        try await respond()
        _ = try await client.flexible(Input(data: payload, checksum: "supplied"))
        requests = try await fixture.requests()
        XCTAssertEqual(requests.last?.headers["x-amz-checksum-crc32"], "supplied")

        let optOut = try ChecksumsClient(ClientConfig(endpoint: fixture.url, requestChecksumCalculation: .whenRequired,
                                                      responseChecksumValidation: .whenRequired))
        try await respond(value: "AAAA")
        _ = try await optOut.flexible(input)
        requests = try await fixture.requests()
        XCTAssertNil(requests[0].headers["x-algorithm"])
        XCTAssertNil(requests[0].headers["x-validation"])
        XCTAssertNil(requests[0].headers["x-amz-checksum-crc32"])

        do { _ = try await client.flexible(Input(algorithm: Algorithm(rawValue: "XXHASH64"))); XCTFail("unsupported algorithm accepted") }
        catch is WireError {}
        requests = try await fixture.requests()
        XCTAssertTrue(requests.isEmpty)

        let hook = try ChecksumsClient(ClientConfig(endpoint: fixture.url, middleware: [{ request in
            var request = request; request.body = payload; return request
        }]))
        try await respond()
        _ = try await hook.legacy(Input())
        requests = try await fixture.requests()
        XCTAssertEqual(requests[0].bytes, payload)
        XCTAssertEqual(requests[0].headers["content-md5"], vectors.base64["MD5"], "checksums cover the bytes middleware sends")
    }
}
