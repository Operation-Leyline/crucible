import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// Thread-safe URLSession transport. Injected instances remain caller-owned.
public final class URLSessionTransport: HTTPTransport, Sendable {
    private let session: URLSession
    private let delegate: SessionDelegate
    /// `timeout` is the per-request and per-resource limit, in seconds.
    public init(timeout: TimeInterval = 30) {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = timeout
        configuration.timeoutIntervalForResource = timeout
        configuration.httpShouldSetCookies = false
        // Each logical SDK call must reach the service, including after writes
        // and when the credential provider changes the caller's identity.
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        let delegate = SessionDelegate()
        let queue = OperationQueue(); queue.maxConcurrentOperationCount = 1
        self.delegate = delegate
        session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: queue)
    }
    /// Cancels in-flight requests and releases the session.
    public func close() { session.invalidateAndCancel() }
    deinit { session.invalidateAndCancel() }
    public func send(_ request: HTTPRequest, readBody: Bool) async throws -> HTTPResponse {
        var native = URLRequest(url: request.url)
        native.httpMethod = request.method; native.httpBody = request.body
        for (name, value) in request.headers { native.setValue(value, forHTTPHeaderField: name) }
        let call = HTTPCall(readBody: readBody)
        let task = session.dataTask(with: native)
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                delegate.register(task, call: call)
                if call.start(continuation, task: task) {
                    task.resume()
                } else { delegate.unregister(task) }
            }
        } onCancel: { call.cancel() }
    }
}

// Delegate callbacks are serial, while cancellation may arrive from any executor.
private final class HTTPCall: @unchecked Sendable {
    let readBody: Bool
    private let lock = NSLock()
    private var continuation: CheckedContinuation<HTTPResponse, any Error>?
    private var task: URLSessionTask?
    private var completed = false
    private var response: HTTPURLResponse?
    private var body = Data()
    init(readBody: Bool) { self.readBody = readBody }
    func start(_ continuation: CheckedContinuation<HTTPResponse, any Error>, task: URLSessionTask) -> Bool {
        lock.lock()
        if completed {
            lock.unlock(); task.cancel(); continuation.resume(throwing: CancellationError()); return false
        }
        self.continuation = continuation; self.task = task
        lock.unlock(); return true
    }
    func cancel() {
        finish(.failure(CancellationError()), cancelTask: true)
    }
    func receive(_ response: HTTPURLResponse) {
        lock.lock(); self.response = response; lock.unlock()
    }
    func append(_ data: Data) {
        lock.lock(); if !completed { body.append(data) }; lock.unlock()
    }
    func complete(_ error: (any Error)?) {
        lock.lock(); let response = response; let body = body; lock.unlock()
        if let error { finish(.failure(error)); return }
        guard let response else { finish(.failure(WireError("Missing HTTP response"))); return }
        var headers: [String: [String]] = [:]
        for (name, value) in response.allHeaderFields {
            headers[String(describing: name).lowercased()] = [String(describing: value)]
        }
        finish(.success(HTTPResponse(status: response.statusCode, headers: headers, body: body)))
    }
    private func finish(_ result: Result<HTTPResponse, any Error>, cancelTask: Bool = false) {
        lock.lock()
        guard !completed else { lock.unlock(); return }
        completed = true
        let continuation = continuation; self.continuation = nil
        let task = task; self.task = nil; body = Data()
        lock.unlock()
        if cancelTask { task?.cancel() }
        continuation?.resume(with: result)
    }
}
private final class SessionDelegate: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let lock = NSLock()
    private var calls: [Int: HTTPCall] = [:]
    func register(_ task: URLSessionTask, call: HTTPCall) { lock.lock(); calls[task.taskIdentifier] = call; lock.unlock() }
    func unregister(_ task: URLSessionTask) { lock.lock(); calls.removeValue(forKey: task.taskIdentifier); lock.unlock() }
    private func call(_ task: URLSessionTask, remove: Bool = false) -> HTTPCall? {
        lock.lock(); defer { lock.unlock() }
        return remove ? calls.removeValue(forKey: task.taskIdentifier) : calls[task.taskIdentifier]
    }
    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse,
                    completionHandler: @escaping @Sendable (URLSession.ResponseDisposition) -> Void) {
        guard let call = call(dataTask), let response = response as? HTTPURLResponse else { completionHandler(.cancel); return }
        call.receive(response)
        if (200..<300).contains(response.statusCode) && !call.readBody {
            call.complete(nil); completionHandler(.cancel)
        } else { completionHandler(.allow) }
    }
    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) { call(dataTask)?.append(data) }
    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: (any Error)?) { call(task, remove: true)?.complete(error) }
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}
