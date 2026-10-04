import Foundation

public struct ErrorMetadata: Sendable {
    public var name: String
    public var message: String
    public var status: Int
    public var requestID: String?
    public var headers: [String: [String]]
    public var retryable: Bool
    public var throttling: Bool
    public init(name: String = "UnknownError", message: String = "", status: Int = 0,
                requestID: String? = nil, headers: [String: [String]] = [:], retryable: Bool = false, throttling: Bool = false) {
        self.name = name; self.message = message; self.status = status; self.requestID = requestID
        self.headers = headers; self.retryable = retryable; self.throttling = throttling
    }
}
public protocol ServiceError: Error, Sendable, CustomStringConvertible {
    var metadata: ErrorMetadata { get }
}
extension ServiceError { public var description: String { metadata.message } }
public struct UnknownServiceError: ServiceError {
    public let metadata: ErrorMetadata
    public init(metadata: ErrorMetadata) { self.metadata = metadata }
}
public struct NetworkError: Error, Sendable { public let underlying: any Error }
public struct TimeoutError: Error, Sendable {}

public struct HTTPRequest: Sendable {
    public var url: URL
    public var method: String
    public var headers: [String: String]
    public var body: Data?
    public init(url: URL, method: String, headers: [String: String] = [:], body: Data? = nil) {
        self.url = url; self.method = method; self.headers = headers; self.body = body
    }
}
public struct HTTPResponse: Sendable {
    public let status: Int
    public let headers: [String: [String]]
    public let body: Data
    public init(status: Int, headers: [String: [String]] = [:], body: Data = Data()) {
        self.status = status; self.headers = headers; self.body = body
    }
    internal func header(_ name: String) -> String? {
        headers.first { $0.key.lowercased() == name.lowercased() }?.value.joined(separator: ", ")
    }
}
public protocol HTTPTransport: Sendable {
    /// readBody applies to successful responses; failure bodies carry modeled errors.
    func send(_ request: HTTPRequest, readBody: Bool) async throws -> HTTPResponse
}
public struct ClientConfig: Sendable {
    public var endpoint: String?
    public var transport: (any HTTPTransport)?
    public var token: (@Sendable () async throws -> String?)?
    public var maxAttempts: Int
    public var timeout: TimeInterval
    public var baseDelay: TimeInterval
    public var maxDelay: TimeInterval
    public var headers: [String: String]
    public var middleware: [@Sendable (HTTPRequest) async throws -> HTTPRequest]
    public init(endpoint: String? = nil, transport: (any HTTPTransport)? = nil,
                token: (@Sendable () async throws -> String?)? = nil,
                maxAttempts: Int = 3, timeout: TimeInterval = 30, baseDelay: TimeInterval = 0.1, maxDelay: TimeInterval = 20,
                headers: [String: String] = [:], middleware: [@Sendable (HTTPRequest) async throws -> HTTPRequest] = []) {
        self.endpoint = endpoint; self.transport = transport; self.token = token; self.maxAttempts = maxAttempts
        self.timeout = timeout; self.baseDelay = baseDelay; self.maxDelay = maxDelay; self.headers = headers; self.middleware = middleware
    }
}

/// Pull-based pagination: each next() performs at most one logical SDK call.
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
