import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import XCTest

/// A response the loopback fixture returns to the next SDK request.
struct Scripted: Encodable, Sendable {
    var status = 200
    var headers: [String: String] = [:]
    var body = "{}"
    var block = false
    init(status: Int = 200, headers: [String: String] = [:], body: String = "{}", block: Bool = false) {
        self.status = status; self.headers = headers; self.body = body; self.block = block
    }
}

/// A request as the loopback fixture received it.
struct Recorded: Decodable, Sendable {
    let method: String
    let path: String
    let headers: [String: String]
    let body: String
    /// Whether an independent signer accepted the SigV4 signature; nil when unsigned.
    let sigv4: Bool?
    var bytes: Data { Data(base64Encoded: body) ?? Data() }
    var json: [String: Any] { ((try? JSONSerialization.jsonObject(with: bytes)) as? [String: Any]) ?? [:] }
    var route: String { String(path.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false)[0]) }
    var query: [String: String] {
        guard let raw = path.split(separator: "?", maxSplits: 1).dropFirst().first else { return [:] }
        var result: [String: String] = [:]
        for pair in raw.split(separator: "&") {
            let parts = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false).map { String($0).removingPercentEncoding ?? String($0) }
            result[parts[0]] = parts.count > 1 ? parts[1] : ""
        }
        return result
    }
}

struct ScriptedFixture {
    let url: String
    init() throws {
        guard let url = ProcessInfo.processInfo.environment["SCRIPT_URL"] else { throw XCTSkip("SCRIPT_URL is not set") }
        self.url = url
    }
    private func exchange(_ path: String, _ body: Data?) async throws -> Data {
        var request = URLRequest(url: URL(string: url + path)!)
        request.httpMethod = body == nil ? "GET" : "POST"
        request.httpBody = body
        return try await withCheckedThrowingContinuation { continuation in
            URLSession.shared.dataTask(with: request) { data, _, error in
                if let error { continuation.resume(throwing: error) } else { continuation.resume(returning: data ?? Data()) }
            }.resume()
        }
    }
    func script(_ responses: Scripted...) async throws {
        _ = try await exchange("/__script", try JSONEncoder().encode(responses))
    }
    func requests() async throws -> [Recorded] {
        try JSONDecoder().decode([Recorded].self, from: try await exchange("/__requests", nil))
    }
    func reset() async throws { _ = try await requests() }
}
