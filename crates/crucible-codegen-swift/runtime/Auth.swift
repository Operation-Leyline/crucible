import Foundation
import Crypto

/// HTTP Basic credentials. Formatting and reflection never reveal the password.
public struct BasicCredentials: Sendable, CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    /// The user name; it may not contain a colon.
    public let username: String
    /// The password, sent only in the `Authorization` header over HTTPS or loopback HTTP.
    public let password: String
    /// Throws ``WireError`` when `username` contains a colon.
    public init(username: String, password: String) throws {
        guard !username.contains(":") else { throw WireError("Basic authentication username cannot contain a colon") }
        self.username = username; self.password = password
    }
    public var description: String { "BasicCredentials(redacted)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: [:]) }
}

/// AWS credentials. Formatting and reflection never reveal the key material.
public struct AWSCredentials: Sendable, CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    /// The access key ID.
    public let accessKeyID: String
    /// The secret access key.
    public let secretAccessKey: String
    /// The session token of temporary credentials.
    public let sessionToken: String?
    /// When temporary credentials expire; signing with expired ones fails.
    public let expiration: Date?
    /// Throws ``WireError`` when the key ID or secret is empty.
    public init(accessKeyID: String, secretAccessKey: String, sessionToken: String? = nil, expiration: Date? = nil) throws {
        guard !accessKeyID.isEmpty, !secretAccessKey.isEmpty else { throw WireError("AWS access key and secret key must be nonempty") }
        self.accessKeyID = accessKeyID; self.secretAccessKey = secretAccessKey
        self.sessionToken = sessionToken; self.expiration = expiration
    }
    public var description: String { "AWSCredentials(redacted)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: [:]) }
}

/// Resolves AWS credentials. `nil` means unavailable here; thrown errors stop a chain.
public protocol CredentialsProvider: Sendable {
    /// The credentials for the next call, or nil when this provider has none.
    func credentials() async throws -> AWSCredentials?
}

/// Always supplies the same credentials.
public struct StaticCredentialsProvider: CredentialsProvider {
    private let value: AWSCredentials
    public init(_ credentials: AWSCredentials) { value = credentials }
    public func credentials() async throws -> AWSCredentials? { value }
}

/// Supplies credentials from a closure, called for every request.
public struct ClosureCredentialsProvider: CredentialsProvider {
    private let closure: @Sendable () async throws -> AWSCredentials?
    public init(_ closure: @escaping @Sendable () async throws -> AWSCredentials?) { self.closure = closure }
    public func credentials() async throws -> AWSCredentials? { try await closure() }
}

/// Tries providers in order and stops on the first identity or error.
public struct CredentialChain: CredentialsProvider {
    private let providers: [any CredentialsProvider]
    public init(_ providers: [any CredentialsProvider]) { self.providers = providers }
    public init(_ providers: any CredentialsProvider...) { self.providers = providers }
    public func credentials() async throws -> AWSCredentials? {
        for provider in providers {
            try Task.checkCancellation()
            if let value = try await provider.credentials() { return value }
        }
        return nil
    }
}

private func staticCredentials(_ access: String?, _ secret: String?, _ token: String?) throws -> AWSCredentials? {
    let access = access ?? "", secret = secret ?? ""
    if access.isEmpty && secret.isEmpty { return nil }
    guard !access.isEmpty, !secret.isEmpty else { throw WireError("AWS credentials are incomplete") }
    return try AWSCredentials(accessKeyID: access, secretAccessKey: secret, sessionToken: token.flatMap { $0.isEmpty ? nil : $0 })
}

/// Reads AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY and AWS_SESSION_TOKEN.
/// Reads `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN`.
public struct EnvironmentCredentialsProvider: CredentialsProvider {
    public init() {}
    public func credentials() async throws -> AWSCredentials? {
        let environment = ProcessInfo.processInfo.environment
        return try staticCredentials(environment["AWS_ACCESS_KEY_ID"], environment["AWS_SECRET_ACCESS_KEY"], environment["AWS_SESSION_TOKEN"])
    }
}

/// Reads static shared-file credentials. It never executes `credential_process`.
/// Reads static keys from a shared credentials file; never runs `credential_process`.
public struct ProfileCredentialsProvider: CredentialsProvider {
    private let profile: String?
    private let path: String?
    public init(profile: String? = nil, path: String? = nil) { self.profile = profile; self.path = path }
    public func credentials() async throws -> AWSCredentials? {
        let environment = ProcessInfo.processInfo.environment
        let name = profile ?? environment["AWS_PROFILE"].flatMap { $0.isEmpty ? nil : $0 } ?? "default"
        let source = path ?? environment["AWS_SHARED_CREDENTIALS_FILE"].flatMap { $0.isEmpty ? nil : $0 }
            ?? (NSHomeDirectory() as NSString).appendingPathComponent(".aws/credentials")
        guard FileManager.default.fileExists(atPath: source) else { return nil }
        let data = try Data(contentsOf: URL(fileURLWithPath: source))
        guard let text = String(data: data, encoding: .utf8) else { throw WireError("Credentials file is not UTF-8") }
        var section = ""; var values: [String: String] = [:]
        for raw in text.split(omittingEmptySubsequences: false, whereSeparator: { $0 == "\n" || $0 == "\r\n" || $0 == "\r" }) {
            try Task.checkCancellation()
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") || line.hasPrefix(";") { continue }
            if line.hasPrefix("[") && line.hasSuffix("]") {
                section = line.dropFirst().dropLast().trimmingCharacters(in: .whitespaces); continue
            }
            guard section == name else { continue }
            guard let equals = line.firstIndex(of: "=") else { throw WireError("Invalid credentials profile entry") }
            let key = line[..<equals].trimmingCharacters(in: .whitespaces)
            guard values[key] == nil else { throw WireError("Duplicate credentials profile entry") }
            values[key] = line[line.index(after: equals)...].trimmingCharacters(in: .whitespaces)
        }
        return try staticCredentials(values["aws_access_key_id"], values["aws_secret_access_key"], values["aws_session_token"])
    }
}

/// Shares one provider across tasks. At most one refresh is in flight; waiting
/// callers that are cancelled return promptly without cancelling the refresh.
public actor CachedCredentials: CredentialsProvider {
    private let provider: any CredentialsProvider
    private let refreshBefore: TimeInterval
    private let clock: @Sendable () -> Date
    private var value: AWSCredentials?
    private var refreshAt: Date?
    private var refreshing = false
    private var waiters: [UInt64: CheckedContinuation<AWSCredentials?, any Error>] = [:]
    private var nextWaiter: UInt64 = 0

    public init(_ provider: any CredentialsProvider, refreshBefore: TimeInterval = 60,
                clock: @escaping @Sendable () -> Date = { Date() }) throws {
        guard refreshBefore.isFinite, refreshBefore >= 0 else { throw WireError("refreshBefore must be finite and nonnegative") }
        self.provider = provider; self.refreshBefore = refreshBefore; self.clock = clock
    }

    public func credentials() async throws -> AWSCredentials? {
        try Task.checkCancellation()
        if let value, refreshAt.map({ clock() < $0 }) ?? true { return value }
        nextWaiter += 1
        let id = nextWaiter
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<AWSCredentials?, any Error>) in
                if Task.isCancelled { continuation.resume(throwing: CancellationError()); return }
                waiters[id] = continuation
                if !refreshing {
                    refreshing = true
                    let provider = self.provider
                    Task {
                        let result: Result<AWSCredentials?, any Error>
                        do { result = .success(try await provider.credentials()) } catch { result = .failure(error) }
                        self.finish(result)
                    }
                }
            }
        } onCancel: {
            Task { await self.cancel(id) }
        }
    }

    private func cancel(_ id: UInt64) {
        waiters.removeValue(forKey: id)?.resume(throwing: CancellationError())
    }

    private func finish(_ result: Result<AWSCredentials?, any Error>) {
        var outcome = result
        if case .success(let fresh) = result {
            let now = clock()
            if let expiration = fresh?.expiration, expiration <= now {
                outcome = .failure(WireError("Credential provider returned expired credentials"))
            } else {
                value = fresh; refreshAt = nil
                if let expiration = fresh?.expiration {
                    // Short-lived credentials remain reusable for half their lifetime
                    // instead of making every caller refresh again.
                    refreshAt = expiration.addingTimeInterval(-min(refreshBefore, expiration.timeIntervalSince(now) / 2))
                }
            }
        }
        refreshing = false
        let pending = waiters; waiters = [:]
        for continuation in pending.values { continuation.resume(with: outcome) }
    }
}

/// Scope and behavior of one Signature Version 4 signature.
public struct SigningOptions: Sendable {
    /// The signing region.
    public var region: String
    /// The signing name of the service.
    public var service: String
    /// The signing time; the current time when nil.
    public var date: Date?
    /// Signs with `UNSIGNED-PAYLOAD` instead of the body digest.
    public var unsignedPayload: Bool
    /// Defaults to true for S3 and false for other services when nil.
    public var disableDoubleEncoding: Bool?
    /// Whether to leave the canonical path unnormalized.
    public var disableNormalizePath: Bool?
    public init(region: String, service: String, date: Date? = nil, unsignedPayload: Bool = false,
                disableDoubleEncoding: Bool? = nil, disableNormalizePath: Bool? = nil) {
        self.region = region; self.service = service; self.date = date; self.unsignedPayload = unsignedPayload
        self.disableDoubleEncoding = disableDoubleEncoding; self.disableNormalizePath = disableNormalizePath
    }
}

/// AWS Signature Version 4 over the final buffered request.
public enum SigV4Signer {
    private static let excluded: Set<String> = ["authorization", "connection", "expect", "transfer-encoding", "user-agent",
                                                "x-amzn-trace-id", "accept-encoding", "content-length", "host"]

    /// Adds the `Authorization`, `X-Amz-Date` and, for session credentials,
    /// `X-Amz-Security-Token` headers to `request`. Throws ``WireError`` when
    /// the credentials have expired.
    public static func sign(_ request: inout HTTPRequest, credentials: AWSCredentials, options: SigningOptions) throws {
        let now = options.date ?? Date()
        if let expiration = credentials.expiration, expiration <= now { throw WireError("AWS credentials have expired") }
        guard !options.service.isEmpty, !options.region.isEmpty,
              !(options.service + options.region).contains(where: { "/\r\n".contains($0) }) else {
            throw WireError("SigV4 service and region must be nonempty scope components")
        }
        guard let components = URLComponents(url: request.url, resolvingAgainstBaseURL: false), let host = components.host else {
            throw WireError("SigV4 requires an absolute URL")
        }
        var headers: [String: String] = [:]
        for (name, value) in request.headers { headers[name.lowercased()] = value }
        headers.removeValue(forKey: "authorization")
        let (date, stamp) = timestamp(now)
        headers["x-amz-date"] = stamp
        if let token = credentials.sessionToken { headers["x-amz-security-token"] = token } else { headers.removeValue(forKey: "x-amz-security-token") }
        let digest = options.unsignedPayload ? "UNSIGNED-PAYLOAD" : hex(SHA256.hash(data: request.body ?? Data()))
        headers["x-amz-content-sha256"] = digest
        var path = components.percentEncodedPath.isEmpty ? "/" : components.percentEncodedPath
        if !(options.disableNormalizePath ?? (options.service == "s3")) { path = normalize(path) }
        if !(options.disableDoubleEncoding ?? (options.service == "s3")) {
            path = path.split(separator: "/", omittingEmptySubsequences: false).map { uriEncode(String($0)) }.joined(separator: "/")
        }
        var query: [(String, String)] = []
        for pair in (components.percentEncodedQuery ?? "").split(separator: "&") where !pair.isEmpty {
            let parts = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            let key = String(parts[0]), value = parts.count > 1 ? String(parts[1]) : ""
            query.append((uriEncode(key.removingPercentEncoding ?? key), uriEncode(value.removingPercentEncoding ?? value)))
        }
        query.sort { $0.0 == $1.0 ? bytes($0.1, before: $1.1) : bytes($0.0, before: $1.0) }
        var canonical: [String: String] = [:]
        for (name, value) in headers where !excluded.contains(name) {
            canonical[name] = value.split(whereSeparator: { $0 == " " || $0 == "\t" || $0 == "\n" || $0 == "\r" }).joined(separator: " ")
        }
        let authority = host.contains(":") && !host.hasPrefix("[") ? "[\(host)]" : host
        canonical["host"] = authority + (components.port.map { ":\($0)" } ?? "")
        let names = canonical.keys.sorted { bytes($0, before: $1) }
        let signed = names.joined(separator: ";")
        let canonicalRequest = [request.method, path, query.map { "\($0.0)=\($0.1)" }.joined(separator: "&"),
                                names.map { "\($0):\(canonical[$0]!)\n" }.joined(), signed, digest].joined(separator: "\n")
        let scope = "\(date)/\(options.region)/\(options.service)/aws4_request"
        let toSign = "AWS4-HMAC-SHA256\n\(stamp)\n\(scope)\n\(hex(SHA256.hash(data: Data(canonicalRequest.utf8))))"
        var key = Data("AWS4\(credentials.secretAccessKey)".utf8)
        for part in [date, options.region, options.service, "aws4_request"] { key = hmac(key, part) }
        headers["authorization"] = "AWS4-HMAC-SHA256 Credential=\(credentials.accessKeyID)/\(scope), SignedHeaders=\(signed), Signature=\(hex(hmac(key, toSign)))"
        request.headers = headers
    }

    private static func hmac(_ key: Data, _ text: String) -> Data {
        Data(HMAC<SHA256>.authenticationCode(for: Data(text.utf8), using: SymmetricKey(data: key)))
    }
    internal static func hex<D: Sequence>(_ bytes: D) -> String where D.Element == UInt8 {
        bytes.map { String(format: "%02x", $0) }.joined()
    }
    private static func bytes(_ left: String, before right: String) -> Bool {
        left.utf8.lexicographicallyPrecedes(right.utf8)
    }
    private static func timestamp(_ date: Date) -> (String, String) {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(secondsFromGMT: 0)!
        let parts = calendar.dateComponents([.year, .month, .day, .hour, .minute, .second], from: date)
        let day = String(format: "%04d%02d%02d", parts.year!, parts.month!, parts.day!)
        return (day, day + String(format: "T%02d%02d%02dZ", parts.hour!, parts.minute!, parts.second!))
    }
    /// Removes dot segments and repeated slashes, preserving a trailing slash.
    private static func normalize(_ path: String) -> String {
        var segments: [Substring] = []
        for segment in path.split(separator: "/") {
            if segment == "." { continue }
            if segment == ".." { if !segments.isEmpty { segments.removeLast() }; continue }
            segments.append(segment)
        }
        var result = "/" + segments.joined(separator: "/")
        if path.hasSuffix("/") && !result.hasSuffix("/") { result += "/" }
        return result
    }
    internal static func uriEncode(_ text: String) -> String {
        text.utf8.map { byte in
            if (65...90).contains(byte) || (97...122).contains(byte) || (48...57).contains(byte) || [45, 46, 95, 126].contains(byte) {
                return String(UnicodeScalar(byte))
            }
            return String(format: "%%%02X", byte)
        }.joined()
    }
}

internal struct AuthOutcome {
    var applied = false
    var sign: (@Sendable (inout HTTPRequest) throws -> Void)?
}

extension ClientRuntime {
    /// Selects the first modeled scheme with a preset or available identity.
    func authenticate(_ request: inout HTTPRequest, operation op: JSONValue, endpoint: ResolvedEndpoint?) async throws -> AuthOutcome {
        let bearerID = "smithy.api#httpBearerAuth"
        let ids: [String]
        if let modeled = op["auth"] { ids = try modeled.array.map { try $0.string } }
        else { ids = op["bearer"]?.flag == true ? [bearerID] : [] }
        for id in ids {
            var scheme: [String: JSONValue]
            if id == bearerID { scheme = ["kind": .string("bearer")] }
            else {
                guard let described = codec.schema["authSchemes"]?[id] else { throw WireError("Unknown authentication scheme: \(id)") }
                scheme = try described.object
            }
            let kind = try scheme["kind"]?.string ?? ""
            if case .array(let choices)? = endpoint?.properties["authSchemes"] {
                guard let choice = choices.first(where: { $0["name"]?.text?.lowercased() == kind.lowercased() }) else { continue }
                for key in ["signingName", "signingRegion", "disableDoubleEncoding", "disableNormalizePath"] {
                    if let value = choice[key], value != .null { scheme[key] = value }
                }
            }
            if Self.preset(request, kind: kind, scheme: scheme) { return AuthOutcome(applied: true) }
            switch kind {
            case "bearer":
                if let token = try await config.token?(), !token.isEmpty {
                    request.headers["authorization"] = "Bearer \(token)"; return AuthOutcome(applied: true)
                }
            case "basic":
                if let identity = try await config.basicAuth?() {
                    request.headers["authorization"] = "Basic " + Data("\(identity.username):\(identity.password)".utf8).base64EncodedString()
                    return AuthOutcome(applied: true)
                }
            case "apiKey":
                if let key = try await config.apiKey?(), !key.isEmpty {
                    let name = try scheme["name"]?.string ?? ""
                    if scheme["location"]?.text == "query" {
                        guard var components = URLComponents(url: request.url, resolvingAgainstBaseURL: false) else { throw WireError("Invalid request URL") }
                        let pair = SigV4Signer.uriEncode(name) + "=" + SigV4Signer.uriEncode(key)
                        components.percentEncodedQuery = components.percentEncodedQuery.map { $0.isEmpty ? pair : $0 + "&" + pair } ?? pair
                        guard let url = components.url else { throw WireError("Invalid request URL") }
                        request.url = url
                    } else {
                        let prefix = scheme["scheme"]?.text.map { $0 + " " } ?? ""
                        request.headers[name.lowercased()] = prefix + key
                    }
                    return AuthOutcome(applied: true)
                }
            case "sigV4":
                if let identity = try await config.credentials?.credentials() {
                    guard let region = scheme["signingRegion"]?.text ?? config.region else { throw WireError("A region is required for SigV4") }
                    let options = SigningOptions(region: region, service: try scheme["signingName"]?.string ?? "",
                        unsignedPayload: op["unsignedPayload"]?.flag == true,
                        disableDoubleEncoding: scheme["disableDoubleEncoding"].flatMap(Self.boolean),
                        disableNormalizePath: scheme["disableNormalizePath"].flatMap(Self.boolean))
                    return AuthOutcome(applied: true) { request in try SigV4Signer.sign(&request, credentials: identity, options: options) }
                }
            default: throw WireError("Unsupported authentication scheme: \(kind)")
            }
        }
        if !ids.isEmpty && ids != [bearerID] && op["optionalAuth"]?.flag != true {
            throw WireError("No credentials available for the operation's authentication schemes")
        }
        return AuthOutcome()
    }

    private static func boolean(_ value: JSONValue) -> Bool? { if case .bool(let flag) = value { return flag }; return nil }

    /// A caller-supplied credential for the scheme bypasses its provider.
    private static func preset(_ request: HTTPRequest, kind: String, scheme: [String: JSONValue]) -> Bool {
        func prefixed(_ value: String?, _ prefix: String) -> Bool {
            guard let value, value.count > prefix.count + 1 else { return false }
            return value.lowercased().hasPrefix(prefix.lowercased() + " ")
        }
        let headers = Dictionary(request.headers.map { ($0.key.lowercased(), $0.value) }, uniquingKeysWith: { _, last in last })
        switch kind {
        case "bearer": return prefixed(headers["authorization"], "Bearer")
        case "basic": return prefixed(headers["authorization"], "Basic")
        case "sigV4": return prefixed(headers["authorization"], "AWS4-HMAC-SHA256")
        case "apiKey":
            let name = scheme["name"]?.text ?? ""
            if scheme["location"]?.text == "query" {
                let items = URLComponents(url: request.url, resolvingAgainstBaseURL: false)?.queryItems ?? []
                return items.contains { $0.name == name && !($0.value ?? "").isEmpty }
            }
            if let prefix = scheme["scheme"]?.text { return prefixed(headers[name.lowercased()], prefix) }
            return !(headers[name.lowercased()] ?? "").isEmpty
        default: return false
        }
    }
}
