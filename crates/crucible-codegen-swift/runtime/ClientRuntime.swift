import Foundation
import Crypto

internal final class ClientRuntime: Sendable {
    private let config: ClientConfig
    private let endpoint: String
    private let digest: Bool
    private let transport: any HTTPTransport
    private let owned: URLSessionTransport?
    let codec: WireCodec
    init(_ config: ClientConfig, endpoint: String, digest: Bool) throws {
        self.config = config; self.digest = digest
        var endpoint = config.endpoint ?? endpoint
        while endpoint.hasSuffix("/") { endpoint.removeLast() }
        guard let url = URLComponents(string: endpoint), ["http", "https"].contains(url.scheme ?? ""),
              let host = url.host, !host.isEmpty, url.user == nil, url.password == nil, url.query == nil, url.fragment == nil,
              url.url != nil else { throw WireError("Absolute HTTP(S) endpoint without credentials, query or fragment required") }
        guard config.maxAttempts > 0, config.timeout.isFinite, config.timeout > 0,
              config.timeout <= Double(Int64.max) / 1_000_000_000,
              config.baseDelay.isFinite, config.baseDelay >= 0, config.maxDelay.isFinite, config.maxDelay >= 0 else {
            throw WireError("Invalid timeout, retry delays or attempt limit")
        }
        self.endpoint = endpoint
        guard let resource = Bundle.module.url(forResource: "schema", withExtension: "json") else { throw WireError("SDK schema missing") }
        codec = WireCodec(schema: try JSONValue(data: Data(contentsOf: resource)))
        if let injected = config.transport { transport = injected; owned = nil }
        else {
            // FoundationNetworking rounds short I/O timeouts on some platforms.
            // Keep its timer beyond the SDK deadline, which cancels the task.
            let native = URLSessionTransport(timeout: ceil(config.timeout) + 1)
            transport = native; owned = native
        }
    }
    func close() { owned?.close() }
    private func operation(_ name: String) throws -> JSONValue {
        guard let operation = codec.schema["operations"]?[name] else { throw WireError("Unknown operation: \(name)") }
        return operation
    }
    private struct Prepared: Sendable { let operation: JSONValue; let request: HTTPRequest }
    private func prepare(_ op: JSONValue, input: JSONValue) throws -> Prepared {
        var values = try input.object
        if let token = op["token"]?.text, values[token] == nil || values[token] == .null { values[token] = .string(UUID().uuidString.lowercased()) }
        let bindings = op["request"]!
        var path = try op["uri"]!.string
        for label in try bindings["labels"]!.array {
            let name = try label.string
            guard let value = values[name], value != .null else { throw WireError("Missing label: \(name)") }
            let text = try codec.bound(value)
            guard !text.isEmpty else { throw WireError("Empty label: \(name)") }
            path = path.replacingOccurrences(of: "{\(name)}", with: Self.escape(text))
        }
        var query: [String] = []
        for binding in try bindings["query"]!.array {
            let pair = try binding.array
            guard let value = values[try pair[0].string], value != .null else { continue }
            let items = (try? value.array) ?? [value]
            for item in items { query.append(try Self.escape(pair[1].string) + "=" + Self.escape(codec.bound(item))) }
        }
        if !query.isEmpty { path += (path.contains("?") ? "&" : "?") + query.joined(separator: "&") }
        var headers = Self.headers(config.headers)
        for binding in try bindings["headers"]!.array {
            let pair = try binding.array
            if let value = values[try pair[0].string], value != .null { headers[try pair[1].string.lowercased()] = try codec.bound(value) }
        }
        var body: Data?
        let members = try bindings["body"]!.array
        if !members.isEmpty {
            var object: [String: JSONValue] = [:]
            for member in members { let name = try member.string; if let value = values[name] { object[name] = value } }
            body = try JSONValue.object(object).data(); headers["content-type"] = "application/json"
        }
        if let payload = try Self.payload(bindings), let value = values[payload.0], value != .null {
            guard let raw = try Data(base64Encoded: value.string) else { throw WireError("Invalid payload blob") }
            if !raw.isEmpty { body = raw; headers["content-type"] = payload.1 }
        }
        guard let url = URL(string: endpoint + path) else { throw WireError("Invalid request URL") }
        return Prepared(operation: op, request: HTTPRequest(url: url, method: try op["method"]!.string, headers: headers, body: body))
    }
    func call<Input: WireCodable, Output: WireCodable>(_ name: String, input: Input, as: Output.Type) async throws -> Output {
        let op = try operation(name)
        let values = try op["input"]?.text.map { try codec.encode($0, input) } ?? .object([:])
        let result = try await send(prepare(op, input: values))
        let context = try op["output"]?.text.map { try codec.context($0) }
            ?? WireContext(codec: codec, target: "smithy.api#Unit", format: "date-time")
        return try Output.fromWire(result, context)
    }
    func pages<Input: WireCodable, Output: WireCodable>(_ name: String, input: Input, as: Output.Type) throws -> Pages<Output> {
        let op = try operation(name)
        let values = try op["input"]?.text.map { try codec.encode($0, input) } ?? .object([:])
        let pagination = op["pagination"]!
        return Pages(input: values, tokenMember: try pagination["input"]!.string) { [self] input in
            let result = try await send(prepare(op, input: input))
            let output = try codec.decode(op["output"]!.string, result, as: Output.self)
            var token: JSONValue? = result
            for part in try pagination["output"]!.array { token = token?[try part.string] }
            return (output, token?.text)
        }
    }
    private func send(_ prepared: Prepared) async throws -> JSONValue {
        try await withDeadline(config.timeout) { [self] in
            let op = prepared.operation
            let safe = op["safe"]?.flag == true
            for attempt in 1...config.maxAttempts {
                try Task.checkCancellation()
                var request = prepared.request
                if codec.schema["bearer"]?.flag == true, let token = try await config.token?() { request.headers["authorization"] = "Bearer \(token)" }
                for middleware in config.middleware { try Task.checkCancellation(); request = try await middleware(request) }
                request.headers = Self.headers(request.headers)
                guard let components = URLComponents(url: request.url, resolvingAgainstBaseURL: false),
                      ["http", "https"].contains(components.scheme ?? ""), components.host != nil,
                      components.user == nil, components.password == nil else { throw WireError("Invalid middleware URL") }
                if request.headers["authorization"] != nil && !Self.secure(request.url) { throw WireError("Credentials require HTTPS or loopback HTTP") }
                if digest, let body = request.body, !body.isEmpty {
                    request.headers["x-amz-content-sha256"] = SHA256.hash(data: body).map { String(format: "%02x", $0) }.joined()
                }
                try Task.checkCancellation()
                let response: HTTPResponse
                do {
                    let bindings = op["response"]!
                    response = try await transport.send(request, readBody: !bindings["body"]!.array.isEmpty || Self.payload(bindings) != nil)
                } catch {
                    if Task.isCancelled || error is CancellationError { throw CancellationError() }
                    if owned != nil, let error = error as? URLError, error.code == .timedOut { throw TimeoutError() }
                    guard safe && attempt < config.maxAttempts else { throw NetworkError(underlying: error) }
                    try await sleep(seconds: retryDelay(attempt, header: nil)); continue
                }
                try Task.checkCancellation()
                if (200..<300).contains(response.status) { return try success(op, response) }
                let error = failure(op, response)
                let metadata = error.metadata
                let retry = metadata.throttling || safe && (metadata.retryable || [500, 502, 503, 504].contains(response.status))
                guard retry && attempt < config.maxAttempts else { throw error }
                try await sleep(seconds: retryDelay(attempt, header: response.header("retry-after")))
            }
            throw WireError("No request attempt")
        }
    }
    private func success(_ op: JSONValue, _ response: HTTPResponse) throws -> JSONValue {
        let bindings = op["response"]!
        var body: [String: JSONValue]
        if let payload = try Self.payload(bindings) { body = [payload.0: .string(response.body.base64EncodedString())] }
        else if try bindings["body"]!.array.isEmpty { body = [:] }
        else { body = response.body.isEmpty ? [:] : try JSONValue(data: response.body).object }
        if let target = op["output"]?.text { try applyHeaders(target, bindings: bindings["headers"]!, response: response, into: &body) }
        return .object(body)
    }
    private func applyHeaders(_ target: String, bindings: JSONValue, response: HTTPResponse, into body: inout [String: JSONValue]) throws {
        for binding in try bindings.array {
            let pair = try binding.array; let name = try pair[0].string
            if let value = try response.header(pair[1].string) { body[name] = try codec.header(codec.member(target, name), value) }
        }
    }
    private func failure(_ op: JSONValue, _ response: HTTPResponse) -> any ServiceError {
        var body = (try? JSONValue(data: response.body).object) ?? [:]
        let name = body["__type"]?.text
        let declared = ((try? op["errors"]?.array) ?? []).compactMap(\.text)
        let matches = declared.filter { id in
            guard let shape = try? codec.shape(id) else { return false }
            if let name { return shape["name"]?.text == name }
            return (try? WireNumber(shape["error"]!["status"]!.number).integer(Int64.self)) == Int64(response.status)
        }
        let target = matches.count == 1 ? matches[0] : nil
        let shape = target.flatMap { try? codec.shape($0) }
        let traits = shape?["error"]
        let metadata = ErrorMetadata(name: name ?? shape?["name"]?.text ?? "UnknownError",
            message: body["message"]?.text ?? "HTTP \(response.status)", status: response.status,
            requestID: response.header("x-request-id") ?? body["requestId"]?.text, headers: response.headers,
            retryable: traits?["retryable"]?.flag == true, throttling: traits?["throttling"]?.flag == true)
        if let target, let headers = traits?["headers"] {
            do {
                try applyHeaders(target, bindings: headers, response: response, into: &body)
                return try ModeledErrors.decode(target, value: .object(body), codec: codec, metadata: metadata)
            } catch { /* Preserve HTTP metadata when a modeled member is malformed. */ }
        }
        return UnknownServiceError(metadata: metadata)
    }
    internal func retryDelay(_ attempt: Int, header: String?) -> TimeInterval {
        if let header {
            if let seconds = Double(header), seconds.isFinite && seconds >= 0 { return seconds }
            let formatter = DateFormatter(); formatter.locale = Locale(identifier: "en_US_POSIX")
            formatter.timeZone = TimeZone(secondsFromGMT: 0); formatter.dateFormat = "EEE, dd MMM yyyy HH:mm:ss 'GMT'"
            if let date = formatter.date(from: header) {
                let delay = date.timeIntervalSinceNow
                if delay >= 0 { return delay }
            }
        }
        let cap = min(config.maxDelay, config.baseDelay * pow(2, Double(min(attempt - 1, 30))))
        return Double.random(in: 0...cap)
    }
    private static func payload(_ bindings: JSONValue) throws -> (String, String)? {
        guard let value = bindings["payload"], value != .null else { return nil }
        let pair = try value.array; return (try pair[0].string, try pair[1].string)
    }
    private static func headers(_ values: [String: String]) -> [String: String] {
        var result: [String: String] = [:]
        for key in values.keys.sorted() { result[key.lowercased()] = values[key] }
        return result
    }
    private static func secure(_ url: URL) -> Bool {
        url.scheme == "https" || url.scheme == "http" && ["localhost", "127.0.0.1", "::1", "[::1]"].contains(url.host?.lowercased() ?? "")
    }
    private static func escape(_ text: String) -> String {
        text.utf8.map { byte in
            if (65...90).contains(byte) || (97...122).contains(byte) || (48...57).contains(byte) || [45, 46, 95, 126].contains(byte) { return String(UnicodeScalar(byte)) }
            return String(format: "%%%02X", byte)
        }.joined()
    }
}
