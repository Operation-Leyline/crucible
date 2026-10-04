import Foundation

internal func sleep(seconds: TimeInterval) async throws {
    guard seconds.isFinite, seconds >= 0 else { throw WireError("Invalid delay") }
    // Bound conversion before converting to UInt64, whose upper bound rounds in Double.
    let nanos = UInt64(min(seconds * 1_000_000_000, Double(Int64.max)))
    try await Task.sleep(nanoseconds: nanos)
}

// Structured task groups wait for uncooperative hooks before returning. This
// race resumes the caller promptly and cancels losing work, which still must
// check cancellation before sending a request or applying the next hook.
private final class DeadlineState<Value: Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Value, any Error>?
    private var tasks: [Task<Void, Never>] = []
    private var result: Result<Value, any Error>?
    func start(_ continuation: CheckedContinuation<Value, any Error>) -> Bool {
        lock.lock()
        if let result { lock.unlock(); continuation.resume(with: result); return false }
        self.continuation = continuation; lock.unlock(); return true
    }
    func add(_ task: Task<Void, Never>) {
        lock.lock()
        if result != nil { lock.unlock(); task.cancel(); return }
        tasks.append(task); lock.unlock()
    }
    func finish(_ result: Result<Value, any Error>) {
        lock.lock()
        guard self.result == nil else { lock.unlock(); return }
        self.result = result
        let continuation = continuation; self.continuation = nil
        let tasks = tasks; self.tasks = []
        lock.unlock()
        tasks.forEach { $0.cancel() }
        continuation?.resume(with: result)
    }
}
internal func withDeadline<Value: Sendable>(_ seconds: TimeInterval,
    operation: @escaping @Sendable () async throws -> Value) async throws -> Value {
    let state = DeadlineState<Value>()
    return try await withTaskCancellationHandler {
        try await withCheckedThrowingContinuation { continuation in
            guard state.start(continuation) else { return }
            state.add(Task {
                do { try Task.checkCancellation(); state.finish(.success(try await operation())) }
                catch { state.finish(.failure(error)) }
            })
            state.add(Task {
                do { try await sleep(seconds: seconds); state.finish(.failure(TimeoutError())) }
                catch { }
            })
        }
    } onCancel: { state.finish(.failure(CancellationError())) }
}
