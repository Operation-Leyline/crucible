import Foundation

/// What the transport learned about a failed call, apart from the modeled body.
public struct ErrorMetadata: Sendable, CustomStringConvertible {
    /// The error's shape name as received, or `UnknownError` when none was sent.
    public var name: String
    /// The service's human-readable message, empty when it sent none.
    public var message: String
    /// The HTTP status code, or 0 when there was no HTTP response.
    public var status: Int
    /// The request ID from the response headers, when the service sent one.
    public var requestID: String?
    /// The response headers.
    public var headers: [String: [String]]
    /// Whether the retry policy treats this failure as transient.
    public var retryable: Bool
    /// Whether the service asked the client to slow down.
    public var throttling: Bool
    /// Omits `message` and `headers`: they may carry sensitive data.
    public var description: String {
        "ErrorMetadata(name=\(name), status=\(status)" + (requestID.map { ", requestID=\($0)" } ?? "")
            + (retryable ? ", retryable=true" : "") + (throttling ? ", throttling=true" : "") + ")"
    }
    public init(name: String = "UnknownError", message: String = "", status: Int = 0,
                requestID: String? = nil, headers: [String: [String]] = [:], retryable: Bool = false, throttling: Bool = false) {
        self.name = name; self.message = message; self.status = status; self.requestID = requestID
        self.headers = headers; self.retryable = retryable; self.throttling = throttling
    }
}
/// A response the service sent that is not a success.
///
/// Catch the service exception protocol to handle every modeled and unknown
/// service error, or a generated error type for one modeled error. Transport
/// failures are a ``NetworkError`` or ``TimeoutError`` instead.
public protocol ServiceError: Error, Sendable, CustomStringConvertible {
    /// Status, request ID and retry classification of the failed response.
    var metadata: ErrorMetadata { get }
}
extension ServiceError { public var description: String { metadata.message } }
/// The base of every error a service returns, modeled or not.
public protocol ServiceException: ServiceError {}
/// An error response that this client model does not describe, such as one
/// added by a newer server, or one whose body could not be decoded.
public struct UnknownServiceError: ServiceException {
    public let metadata: ErrorMetadata
    public init(metadata: ErrorMetadata) { self.metadata = metadata }
}
/// The request never produced a response: DNS, connection, TLS or I/O failure.
public struct NetworkError: Error, Sendable {
    /// The underlying transport error.
    public let underlying: any Error
}
/// The call outlasted ``ClientConfig/timeout``.
public struct TimeoutError: Error, Sendable {}

/// One outgoing HTTP request.
public struct HTTPRequest: Sendable {
    /// The absolute URL.
    public var url: URL
    /// The HTTP method, upper case.
    public var method: String
    /// The request headers.
    public var headers: [String: String]
    /// The body, or nil for none.
    public var body: Data?
    public init(url: URL, method: String, headers: [String: String] = [:], body: Data? = nil) {
        self.url = url; self.method = method; self.headers = headers; self.body = body
    }
}
/// One HTTP response.
public struct HTTPResponse: Sendable {
    /// The status code.
    public let status: Int
    /// The response headers; a repeated header keeps every value.
    public let headers: [String: [String]]
    /// The body; empty when none was read.
    public let body: Data
    public init(status: Int, headers: [String: [String]] = [:], body: Data = Data()) {
        self.status = status; self.headers = headers; self.body = body
    }
    internal func header(_ name: String) -> String? {
        headers.first { $0.key.lowercased() == name.lowercased() }?.value.joined(separator: ", ")
    }
}
/// Sends HTTP requests for a client. Implement it to plug in a different HTTP
/// stack or a test double through ``ClientConfig/transport``.
public protocol HTTPTransport: Sendable {
    /// Sends `request` and returns the response, without following redirects.
    /// `readBody` applies to successful responses; failure bodies carry modeled errors.
    func send(_ request: HTTPRequest, readBody: Bool) async throws -> HTTPResponse
}
/// Settings for a generated client, fixed when the client is created.
///
/// Every setting is optional. Without credentials, only operations that need
/// none can succeed. A configuration is a value and may be shared by any
/// number of clients.
public struct ClientConfig: Sendable {
    /// The service base URL, overriding the one generated into the client.
    /// Must be absolute HTTP(S) without credentials, query or fragment. Required
    /// unless the model endpoint rules resolve every request.
    public var endpoint: String?
    /// Sends the HTTP requests. Defaults to a `URLSession` that the client
    /// owns and closes with itself. A transport you supply stays yours to close.
    public var transport: (any HTTPTransport)?
    /// Bearer token source, for services that use HTTP bearer authentication.
    public var token: (@Sendable () async throws -> String?)?
    /// The most times a request is sent, the first attempt included. 1 disables
    /// retries. Only operations that are read-only or idempotent are retried
    /// after a network failure or a 500, 502, 503, 504 or retryable response;
    /// throttling responses are retried for any operation.
    public var maxAttempts: Int
    /// The time one call may take in total, in seconds, covering every attempt
    /// and retry delay. Exceeding it throws ``TimeoutError``.
    public var timeout: TimeInterval
    /// The ceiling of the first retry delay, in seconds. Delays use exponential
    /// backoff with full jitter, doubling this value per attempt up to
    /// `maxDelay`; a `Retry-After` response header takes precedence.
    public var baseDelay: TimeInterval
    /// The ceiling of any single backoff delay, in seconds.
    public var maxDelay: TimeInterval
    /// Headers added to every request.
    public var headers: [String: String]
    /// Request rewriters, run in order on every attempt.
    public var middleware: [@Sendable (HTTPRequest) async throws -> HTTPRequest]
    /// Identity for modeled HTTP Basic authentication; `nil` means unavailable.
    public var basicAuth: (@Sendable () async throws -> BasicCredentials?)?
    /// Identity for modeled API key authentication; `nil` or empty means unavailable.
    public var apiKey: (@Sendable () async throws -> String?)?
    /// AWS credentials for SigV4. Wrap a refreshing provider in `CachedCredentials`.
    public var credentials: (any CredentialsProvider)?
    /// The SigV4 signing region and the `AWS::Region` endpoint built-in.
    public var region: String?
    /// Client-context endpoint parameters: strings, booleans, or string arrays.
    public var endpointParameters: [String: JSONValue]
    /// Overrides for endpoint built-ins such as `SDK::Endpoint`.
    public var endpointBuiltIns: [String: JSONValue]
    /// Ignore modeled host prefixes, for endpoints that cannot accept them.
    public var disableHostPrefix: Bool
    /// Whether to checksum request payloads only when an operation requires it
    /// or whenever the service supports it (the default).
    public var requestChecksumCalculation: ChecksumPolicy
    /// Whether to validate response checksums only when required or whenever
    /// the service sends one (the default).
    public var responseChecksumValidation: ChecksumPolicy
    /// Called with the operation name and algorithm after a response checksum validates.
    public var checksumValidated: (@Sendable (String, String) -> Void)?
    /// Invalid values (a non-positive `timeout`, fewer than one attempt,
    /// negative delays) are rejected when a client is created, not here.
    public init(endpoint: String? = nil, transport: (any HTTPTransport)? = nil,
                token: (@Sendable () async throws -> String?)? = nil,
                maxAttempts: Int = 3, timeout: TimeInterval = 30, baseDelay: TimeInterval = 0.1, maxDelay: TimeInterval = 20,
                headers: [String: String] = [:], middleware: [@Sendable (HTTPRequest) async throws -> HTTPRequest] = [],
                basicAuth: (@Sendable () async throws -> BasicCredentials?)? = nil,
                apiKey: (@Sendable () async throws -> String?)? = nil,
                credentials: (any CredentialsProvider)? = nil, region: String? = nil,
                endpointParameters: [String: JSONValue] = [:], endpointBuiltIns: [String: JSONValue] = [:],
                disableHostPrefix: Bool = false,
                requestChecksumCalculation: ChecksumPolicy = .whenSupported,
                responseChecksumValidation: ChecksumPolicy = .whenSupported,
                checksumValidated: (@Sendable (String, String) -> Void)? = nil) {
        self.endpoint = endpoint; self.transport = transport; self.token = token; self.maxAttempts = maxAttempts
        self.timeout = timeout; self.baseDelay = baseDelay; self.maxDelay = maxDelay; self.headers = headers; self.middleware = middleware
        self.basicAuth = basicAuth; self.apiKey = apiKey; self.credentials = credentials; self.region = region
        self.endpointParameters = endpointParameters; self.endpointBuiltIns = endpointBuiltIns
        self.disableHostPrefix = disableHostPrefix
        self.requestChecksumCalculation = requestChecksumCalculation
        self.responseChecksumValidation = responseChecksumValidation
        self.checksumValidated = checksumValidated
    }
}

/// Every page of a paginated operation, as an `AsyncSequence` of outputs.
///
/// Pull-based: each `next()` performs at most one logical SDK call, with the
/// retry behavior of any other call. Iteration ends when the service returns
/// no pagination token, and fails if a token repeats.
public struct Pages<Element: Sendable>: AsyncSequence, Sendable {
    public typealias AsyncIterator = Iterator
    internal let input: JSONValue
    internal let tokenMember: String
    internal let fetch: @Sendable (JSONValue) async throws -> (Element, String?)
    public func makeAsyncIterator() -> Iterator {
        Iterator(input: input, tokenMember: tokenMember, fetch: fetch)
    }
    public struct Iterator: AsyncIteratorProtocol {
        private var input: JSONValue
        private let tokenMember: String
        private let fetch: @Sendable (JSONValue) async throws -> (Element, String?)
        private var seen: Set<String>
        private var finished = false
        private var cycle = false
        internal init(input: JSONValue, tokenMember: String, fetch: @escaping @Sendable (JSONValue) async throws -> (Element, String?)) {
            self.input = input; self.tokenMember = tokenMember; self.fetch = fetch
            self.seen = input[tokenMember]?.text.map { Set([$0]) } ?? []
        }
        public mutating func next() async throws -> Element? {
            try Task.checkCancellation()
            if cycle { finished = true; throw WireError("Pagination token cycle") }
            if finished { return nil }
            do {
                let (element, token) = try await fetch(input)
                if let token, !token.isEmpty {
                    cycle = !seen.insert(token).inserted
                    var object = try input.object; object[tokenMember] = .string(token); input = .object(object)
                } else { finished = true }
                return element
            } catch { finished = true; throw error }
        }
    }
}
