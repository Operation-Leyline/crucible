import Foundation
import Crypto

internal final class ClientRuntime: Sendable {
    let config: ClientConfig
    private let endpoint: String
    private let digest: Bool
    private let transport: any HTTPTransport
    private let owned: URLSessionTransport?
    let codec: WireCodec
    init(_ config: ClientConfig, endpoint: String, digest: Bool) throws {
        self.config = config; self.digest = digest
        guard let resource = Bundle.module.url(forResource: "schema", withExtension: "json") else { throw WireError("SDK schema missing") }
        codec = WireCodec(schema: try JSONValue(data: Data(contentsOf: resource)))
        var endpoint = config.endpoint ?? endpoint
        while endpoint.hasSuffix("/") { endpoint.removeLast() }
        // Modeled endpoint rules resolve the URL per request when none is configured.
        if !(endpoint.isEmpty && codec.schema["endpointRules"] != nil) {
            guard let url = URLComponents(string: endpoint), ["http", "https"].contains(url.scheme ?? ""),
                  let host = url.host, !host.isEmpty, url.user == nil, url.password == nil, url.query == nil, url.fragment == nil,
                  url.url != nil else { throw WireError("Absolute HTTP(S) endpoint without credentials, query or fragment required") }
        }
        guard config.maxAttempts > 0, config.timeout.isFinite, config.timeout > 0,
              config.timeout <= Double(Int64.max) / 1_000_000_000,
              config.baseDelay.isFinite, config.baseDelay >= 0, config.maxDelay.isFinite, config.maxDelay >= 0 else {
            throw WireError("Invalid timeout, retry delays or attempt limit")
        }
        for value in Array(config.endpointParameters.values) + Array(config.endpointBuiltIns.values) {
            switch value {
            case .string, .bool: continue
            case .array(let items) where items.allSatisfy({ $0.text != nil }): continue
            default: throw WireError("Endpoint parameters must be strings, booleans, or string arrays")
            }
        }
        self.endpoint = endpoint
        if let injected = config.transport { transport = injected; owned = nil }
        else {
            // FoundationNetworking rounds short I/O timeouts on some platforms.
            // Keep its timer beyond the SDK deadline, which cancels the task.
            let native = URLSessionTransport(timeout: ceil(config.timeout) + 1)
            transport = native; owned = native
        }
    }
    func close() { owned?.close() }
    func operation(_ name: String) throws -> JSONValue {
        guard let operation = codec.schema["operations"]?[name] else { throw WireError("Unknown operation: \(name)") }
        return operation
    }
    struct Prepared: Sendable {
        let name: String
        let operation: JSONValue
        let request: HTTPRequest
        let checksum: ChecksumSettings
        let endpoint: ResolvedEndpoint?
    }
    private var awsProtocol: Bool { ["awsRestJson1", "awsJson1_0", "awsJson1_1"].contains(codec.schema["protocol"]?.text ?? "") }
    func prepare(_ name: String, _ op: JSONValue, input: JSONValue) throws -> Prepared {
        var values = try input.object
        if let token = op["token"]?.text, values[token] == nil || values[token] == .null { values[token] = .string(UUID().uuidString.lowercased()) }
        let checksum = try Checksums.prepare(op["checksum"], values: &values,
            request: config.requestChecksumCalculation, response: config.responseChecksumValidation)
        var base = endpoint
        var resolved: ResolvedEndpoint?
        if codec.schema["endpointRules"] != nil || op["endpoint"] != nil {
            let view = try codec.queryView(op["input"]?.text, .object(values))
            if let rules = codec.schema["endpointRules"] {
                var builtins = config.endpointBuiltIns
                if builtins["AWS::Region"] == nil, let region = config.region { builtins["AWS::Region"] = .string(region) }
                if builtins["SDK::Endpoint"] == nil, !endpoint.isEmpty { builtins["SDK::Endpoint"] = .string(endpoint) }
                let result = try Endpoints.resolve(rules, sources: op["endpointParameters"], input: view,
                                                   client: config.endpointParameters, builtins: builtins)
                resolved = result; base = result.url
            }
            if let prefix = op["endpoint"], prefix != .null, !config.disableHostPrefix {
                base = try Endpoints.hostPrefix(base, prefix, input: view)
            }
            while base.hasSuffix("/") { base.removeLast() }
        }
        let bindings = op["request"]!
        // HTTP bindings may use a different timestamp format than the body.
        var bound = values
        if let input = op["input"]?.text, let formats = bindings["timestampFormats"] {
            for (name, format) in try formats.object {
                if let value = values[name], value != .null {
                    bound[name] = try codec.rebind(codec.member(input, name), value, format: format.string)
                }
            }
        }
        var path = try op["uri"]!.string
        for label in try bindings["labels"]!.array {
            let name = try label.string
            guard let value = bound[name], value != .null else { throw WireError("Missing label: \(name)") }
            let text = try codec.bound(value)
            guard !text.isEmpty else { throw WireError("Empty label: \(name)") }
            let greedy = ((try? op["greedyLabels"]?.array) ?? []).contains(.string(name))
            let parts = greedy ? text.split(separator: "/", omittingEmptySubsequences: false).map(String.init) : [text]
            guard !parts.contains(".") && !parts.contains("..") else { throw WireError("Dot-segment HTTP label: \(name)") }
            path = path.replacingOccurrences(of: greedy ? "{\(name)+}" : "{\(name)}", with: parts.map(Self.escape).joined(separator: "/"))
        }
        var query: [String] = []
        for binding in try bindings["query"]!.array {
            let pair = try binding.array
            guard let value = bound[try pair[0].string], value != .null else { continue }
            let items = (try? value.array) ?? [value]
            for item in items { query.append(try Self.escape(pair[1].string) + "=" + Self.escape(codec.bound(item))) }
        }
        if let name = bindings["queryParams"]?.text, let value = values[name], value != .null {
            var reserved = Set(try bindings["query"]!.array.map { try $0.array[1].string })
            if let literal = path.split(separator: "?", maxSplits: 1).dropFirst().first {
                for pair in literal.split(separator: "&") {
                    let key = String(pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)[0])
                    reserved.insert(key.removingPercentEncoding ?? key)
                }
            }
            let map = try value.object
            for key in map.keys.sorted() where !reserved.contains(key) {
                let item = map[key]!
                if item == .null { continue }
                for scalar in (try? item.array) ?? [item] where scalar != .null {
                    query.append(try Self.escape(key) + "=" + Self.escape(codec.bound(scalar)))
                }
            }
        }
        if !query.isEmpty { path += (path.contains("?") ? "&" : "?") + query.joined(separator: "&") }
        var headers = Self.headers(config.headers)
        for (name, items) in resolved?.headers ?? [:] { headers[name] = items.joined(separator: ",") }
        for binding in try bindings["headers"]!.array {
            let pair = try binding.array
            if let value = bound[try pair[0].string], value != .null { headers[try pair[1].string.lowercased()] = try codec.bound(Self.media(value, bindings: bindings, member: pair[0].string, encode: true)) }
        }
        let explicitHeaders = Set(try bindings["headers"]!.array.map { try $0.array[1].string.lowercased() })
        for binding in (try? bindings["prefixHeaders"]?.array) ?? [] {
            let pair = try binding.array
            guard let value = values[try pair[0].string], value != .null else { continue }
            let map = try value.object
            for key in map.keys.sorted() {
                let header = try (pair[1].string + key).lowercased()
                if !explicitHeaders.contains(header) { headers[header] = try codec.bound(Self.media(map[key]!, bindings: bindings, member: pair[0].string, encode: true)) }
            }
        }
        var body: Data?
        let members = try bindings["body"]!.array
        if !members.isEmpty {
            var object: [String: JSONValue] = [:]
            for member in members { let name = try member.string; if let value = values[name] { object[name] = value } }
            body = try codec.document(op["input"]!.string, .object(object), encode: true).data(); headers["content-type"] = "application/json"
        }
        if let payload = try Self.payload(bindings), let value = values[payload.0], value != .null {
            switch bindings["payloadKind"]?.text ?? "blob" {
            case "string": body = try Data(value.string.utf8)
            case "document": body = try codec.document(codec.member(op["input"]!.string, payload.0)["target"]!.string, value, encode: true).data()
            default:
                guard let raw = try Data(base64Encoded: value.string) else { throw WireError("Invalid payload blob") }
                body = raw
            }
            // An empty payload sends no body, so its media type does not apply.
            if body?.isEmpty == true { body = nil } else { headers["content-type"] = payload.1 }
        }
        if let version = ["awsJson1_0": "1.0", "awsJson1_1": "1.1"][codec.schema["protocol"]?.text ?? ""] {
            if body == nil { body = Data("{}".utf8) }
            headers["content-type"] = "application/x-amz-json-" + version
            let service = try codec.schema["service"]!.string
            headers["x-amz-target"] = String(service.split(separator: "#").last ?? Substring(service)) + "." + name
        }
        guard let url = URL(string: base + path) else { throw WireError("Invalid request URL") }
        return Prepared(name: name, operation: op, request: HTTPRequest(url: url, method: try op["method"]!.string, headers: headers, body: body),
                        checksum: checksum, endpoint: resolved)
    }
    func call<Input: WireCodable, Output: WireCodable>(_ name: String, input: Input, as: Output.Type) async throws -> Output {
        let op = try operation(name)
        let values = try op["input"]?.text.map { try codec.encode($0, input) } ?? .object([:])
        let result = try await send(prepare(name, op, input: values))
        let context = try op["output"]?.text.map { try codec.context($0) }
            ?? WireContext(codec: codec, target: "smithy.api#Unit", format: "date-time")
        return try Output.fromWire(result, context)
    }
    func pages<Input: WireCodable, Output: WireCodable>(_ name: String, input: Input, as: Output.Type) throws -> Pages<Output> {
        let op = try operation(name)
        let values = try op["input"]?.text.map { try codec.encode($0, input) } ?? .object([:])
        let pagination = op["pagination"]!
        return Pages(input: values, tokenMember: try pagination["input"]!.string) { [self] input in
            let result = try await send(prepare(name, op, input: input))
            let output = try codec.decode(op["output"]!.string, result, as: Output.self)
            var token: JSONValue? = result
            for part in try pagination["output"]!.array { token = token?[try part.string] }
            return (output, token?.text)
        }
    }
    func send(_ prepared: Prepared) async throws -> JSONValue {
        try await withDeadline(config.timeout) { [self] in
            let op = prepared.operation
            let safe = op["safe"]?.flag == true
            for attempt in 1...config.maxAttempts {
                try Task.checkCancellation()
                var request = prepared.request
                let authentication = try await authenticate(&request, operation: op, endpoint: prepared.endpoint)
                for middleware in config.middleware { try Task.checkCancellation(); request = try await middleware(request) }
                request.headers = Self.headers(request.headers)
                guard let components = URLComponents(url: request.url, resolvingAgainstBaseURL: false),
                      ["http", "https"].contains(components.scheme ?? ""), components.host != nil,
                      components.user == nil, components.password == nil else { throw WireError("Invalid middleware URL") }
                if (authentication.applied || request.headers["authorization"] != nil) && !Self.secure(request.url) {
                    throw WireError("Credentials require HTTPS or loopback HTTP; refusing plaintext")
                }
                if digest, request.body != nil || ["POST", "PUT", "PATCH"].contains(request.method) {
                    request.headers["x-amz-content-sha256"] = SHA256.hash(data: request.body ?? Data()).map { String(format: "%02x", $0) }.joined()
                }
                // Checksums and signatures cover the final bytes after middleware.
                try Checksums.apply(prepared.checksum, to: &request)
                try authentication.sign?(&request)
                try Task.checkCancellation()
                let response: HTTPResponse
                do {
                    let bindings = op["response"]!
                    response = try await transport.send(request, readBody: !bindings["body"]!.array.isEmpty || Self.payload(bindings) != nil
                        || !prepared.checksum.response.isEmpty)
                } catch {
                    if Task.isCancelled || error is CancellationError { throw CancellationError() }
                    if owned != nil, let error = error as? URLError, error.code == .timedOut { throw TimeoutError() }
                    guard safe && attempt < config.maxAttempts else { throw NetworkError(underlying: error) }
                    try await sleep(seconds: retryDelay(attempt, header: nil)); continue
                }
                try Task.checkCancellation()
                if (200..<300).contains(response.status) {
                    if let algorithm = try Checksums.validate(prepared.checksum, response) { config.checksumValidated?(prepared.name, algorithm) }
                    return try success(op, response)
                }
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
        if let payload = try Self.payload(bindings) { body = [payload.0: try payloadValue(op["output"]!.string, bindings: bindings, data: response.body)] }
        else if try bindings["body"]!.array.isEmpty { body = [:] }
        else { body = response.body.isEmpty ? [:] : try codec.document(op["output"]!.string, JSONValue(data: response.body), encode: false).object }
        if let target = op["output"]?.text { try applyHeaders(target, bindings: bindings, response: response, into: &body) }
        return .object(body)
    }
    private func applyHeaders(_ target: String, bindings: JSONValue, response: HTTPResponse, into body: inout [String: JSONValue]) throws {
        for binding in try bindings["headers"]!.array {
            let pair = try binding.array; let name = try pair[0].string
            if let value = try response.header(pair[1].string) {
                let format = bindings["timestampFormats"]?[name]?.text
                body[name] = try Self.media(codec.header(codec.member(target, name), value, format: format), bindings: bindings, member: name, encode: false)
            }
        }
        let explicit = Set(try bindings["headers"]!.array.map { try $0.array[1].string.lowercased() })
        for binding in (try? bindings["prefixHeaders"]?.array) ?? [] {
            let pair = try binding.array
            let member = try pair[0].string
            let prefix = try pair[1].string.lowercased()
            var values: [String: JSONValue] = [:]
            for header in response.headers.keys.sorted() {
                let lower = header.lowercased()
                if lower.hasPrefix(prefix) && !explicit.contains(lower), let text = response.header(header) {
                    values[String(lower.dropFirst(prefix.count))] = try Self.media(.string(text), bindings: bindings, member: member, encode: false)
                }
            }
            body[member] = .object(values)
        }
        if let member = bindings["responseCode"]?.text { body[member] = .number(String(response.status)) }
    }
    private func failure(_ op: JSONValue, _ response: HTTPResponse) -> any ServiceError {
        var body = (try? JSONValue(data: response.body).object) ?? [:]
        var name = response.header("x-crucible-error") ?? body["__type"]?.text
        let aws = awsProtocol
        if aws {
            // AWS JSON error codes may carry a namespace and a ':'-separated suffix.
            if let code = body["code"]?.text { name = code }
            if let header = response.header("x-amzn-errortype") { name = header }
            if let raw = name {
                let code = raw.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? raw
                name = code.split(separator: "#", omittingEmptySubsequences: false).last.map(String.init) ?? code
            }
        }
        let declared = ((try? op["errors"]?.array) ?? []).compactMap(\.text)
        let matches = declared.filter { id in
            guard let shape = try? codec.shape(id) else { return false }
            if let name { return (shape["error"]?["wireCode"]?.text ?? shape["name"]?.text) == name }
            return (try? WireNumber(shape["error"]!["status"]!.number).integer(Int64.self)) == Int64(response.status)
        }
        let target = matches.count == 1 ? matches[0] : nil
        let shape = target.flatMap { try? codec.shape($0) }
        let traits = shape?["error"]
        let metadata = ErrorMetadata(name: name ?? shape?["name"]?.text ?? "UnknownError",
            message: body["message"]?.text ?? (aws ? body["Message"]?.text : nil) ?? "HTTP \(response.status)", status: response.status,
            requestID: (aws ? response.header("x-amzn-requestid") ?? response.header("x-amz-request-id") : nil)
                ?? response.header("x-request-id") ?? body["requestId"]?.text, headers: response.headers,
            retryable: traits?["retryable"]?.flag == true, throttling: traits?["throttling"]?.flag == true)
        if let target, let traits {
            do {
                if let payload = try Self.payload(traits) { body = [payload.0: try payloadValue(target, bindings: traits, data: response.body)] }
                else { body = try codec.document(target, .object(body), encode: false).object }
                try applyHeaders(target, bindings: traits, response: response, into: &body)
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
    private func payloadValue(_ target: String, bindings: JSONValue, data: Data) throws -> JSONValue {
        switch bindings["payloadKind"]?.text ?? "blob" {
        case "string":
            guard let text = String(data: data, encoding: .utf8) else { throw WireError("Invalid UTF-8 payload") }
            return .string(text)
        case "document":
            if data.isEmpty { return .null }
            let payload = try Self.payload(bindings)!
            return try codec.document(codec.member(target, payload.0)["target"]!.string, JSONValue(data: data), encode: false)
        default: return .string(data.base64EncodedString())
        }
    }
    private static func media(_ value: JSONValue, bindings: JSONValue, member: String, encode: Bool) throws -> JSONValue {
        guard bindings["headerEncodings"]?[member]?.text == "base64" else { return value }
        if case .array(let items) = value { return .array(try items.map { try media($0, bindings: bindings, member: member, encode: encode) }) }
        if encode { return try .string(Data(value.string.utf8).base64EncodedString()) }
        let text = try value.string
        guard let bytes = Data(base64Encoded: text), bytes.base64EncodedString() == text, let decoded = String(data: bytes, encoding: .utf8) else { throw WireError("Invalid media-type header base64 or UTF-8") }
        return .string(decoded)
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
