import Foundation

/// Overrides the modeled delays, in seconds, between waiter attempts.
public struct WaiterOptions: Sendable {
    /// The shortest delay between attempts; the modeled value when nil.
    public var minDelay: TimeInterval?
    /// The longest delay between attempts; the modeled value when nil.
    public var maxDelay: TimeInterval?
    public init(minDelay: TimeInterval? = nil, maxDelay: TimeInterval? = nil) {
        self.minDelay = minDelay; self.maxDelay = maxDelay
    }
}

/// A waiter reached a failure state or could not evaluate an acceptor.
public struct WaiterError: Error, Sendable, CustomStringConvertible {
    /// The waiter's name.
    public let name: String
    /// How many times the operation was called.
    public let attempts: Int
    /// The operation error observed by the final attempt, if any.
    public let underlying: (any Error)?
    /// The modeled acceptor message, joined when it selects a list.
    public let message: String?
    public var description: String {
        "waiter \(name) failed after \(attempts) attempts" + (message.map { ": \($0)" } ?? underlying.map { ": \($0)" } ?? "")
    }
}

/// The waiter deadline passed before a success or failure state.
public struct WaiterTimeoutError: Error, Sendable, CustomStringConvertible {
    /// The waiter's name.
    public let name: String
    /// How many times the operation was called.
    public let attempts: Int
    /// The operation error observed by the final attempt, if any.
    public let underlying: (any Error)?
    public var description: String { "waiter \(name) timed out after \(attempts) attempts" }
}

private final class WaiterProgress: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    private var last: (any Error)?
    func begin() -> Int { lock.lock(); defer { lock.unlock() }; count += 1; return count }
    func record(_ error: (any Error)?) { lock.lock(); last = error; lock.unlock() }
    var snapshot: (Int, (any Error)?) { lock.lock(); defer { lock.unlock() }; return (count, last) }
}

extension ClientRuntime {
    func wait<Input: WireCodable, Output: WireCodable>(_ name: String, waiter: String, input: Input, timeout: TimeInterval,
                                                        options: WaiterOptions, as: Output.Type) async throws -> Output? {
        let op = try operation(name)
        guard let description = op["waiters"]?[waiter] else { throw WireError("Unknown waiter: \(waiter)") }
        let minimum = try options.minDelay ?? Double(description["minDelay"]?.number ?? "") ?? -1
        let maximum = try options.maxDelay ?? Double(description["maxDelay"]?.number ?? "") ?? -1
        guard timeout.isFinite, timeout > 0, minimum.isFinite, maximum.isFinite, minimum >= 0, maximum >= minimum else {
            throw WireError("Waiter timeout must be positive and delays finite, nonnegative and ordered")
        }
        let values = try op["input"]?.text.map { try codec.encode($0, input) } ?? .object([:])
        let queryInput = try codec.queryView(op["input"]?.text, values)
        let acceptors = try description["acceptors"]!.array
        let outputContext = try op["output"]?.text.map { try codec.context($0) }
            ?? WireContext(codec: codec, target: "smithy.api#Unit", format: "date-time")
        let progress = WaiterProgress()
        let start = ProcessInfo.processInfo.systemUptime
        do {
            return try await withDeadline(timeout) { [self] in
                while true {
                    try Task.checkCancellation()
                    let attempt = progress.begin()
                    var output: Output?
                    var view = JSONValue.null
                    var failure: (any Error)?
                    do {
                        let result = try await send(prepare(name, op, input: values))
                        output = try Output.fromWire(result, outputContext)
                        view = try codec.queryView(op["output"]?.text, result)
                    } catch {
                        if error is CancellationError || Task.isCancelled { throw CancellationError() }
                        failure = error
                    }
                    progress.record(failure)
                    let state: String, message: String?
                    do { (state, message) = try Self.waiterState(acceptors, input: queryInput, output: view, failure: failure) }
                    catch { throw WaiterError(name: waiter, attempts: attempt, underlying: error, message: nil) }
                    if state == "success" { return output }
                    if state == "failure" { throw WaiterError(name: waiter, attempts: attempt, underlying: failure, message: message) }
                    let remaining = timeout - (ProcessInfo.processInfo.systemUptime - start)
                    let ceiling = min(maximum, minimum * pow(2, Double(min(attempt - 1, 62))))
                    var delay = Double.random(in: minimum...max(minimum, ceiling))
                    if remaining <= minimum { delay = remaining }
                    else if remaining - delay <= minimum { delay = remaining - minimum }
                    try await sleep(seconds: max(0, delay))
                }
            }
        } catch is TimeoutError {
            let (attempts, last) = progress.snapshot
            throw WaiterTimeoutError(name: waiter, attempts: attempts, underlying: last)
        }
    }

    /// The first matching acceptor's state, or retry/failure by default.
    static func waiterState(_ acceptors: [JSONValue], input: JSONValue, output: JSONValue,
                            failure: (any Error)?) throws -> (String, String?) {
        for acceptor in acceptors {
            let matcher = acceptor["matcher"]!
            var matches = false
            switch matcher["kind"]?.text {
            case "success": matches = (failure == nil) == matcher["expected"]?.flag
            case "errorType":
                if let error = failure as? any ServiceError {
                    // Error codes may or may not carry the shape's namespace.
                    func local(_ text: String) -> Substring { text.split(separator: "#", omittingEmptySubsequences: false).last ?? Substring(text) }
                    let name = try matcher["name"]!.string
                    matches = error.metadata.name == name || local(error.metadata.name) == local(name)
                }
            case let kind:
                guard failure == nil else { break }
                let source: JSONValue = kind == "inputOutput" ? .object(["input": input, "output": output]) : output
                let result = try JMESPath.evaluate(matcher["path"]!["expression"]!, source)
                let expected = try matcher["expected"]!.string
                switch matcher["comparator"]?.text {
                case "stringEquals": matches = result == .string(expected)
                case "booleanEquals": matches = result == .bool(expected == "true")
                case "allStringEquals":
                    if case .array(let items) = result { matches = !items.isEmpty && items.allSatisfy { $0 == .string(expected) } }
                case "anyStringEquals":
                    if case .array(let items) = result { matches = items.contains(.string(expected)) }
                default: throw WireError("Unsupported waiter comparator")
                }
            }
            guard matches else { continue }
            var message: String?
            if let path = acceptor["message"], path != .null {
                switch try JMESPath.evaluate(path["expression"]!, output) {
                case .string(let text): message = text
                case .array(let items) where items.allSatisfy({ $0.text != nil }): message = items.compactMap(\.text).joined(separator: ", ")
                default: break
                }
            }
            return (try acceptor["state"]!.string, message)
        }
        return (failure == nil ? "retry" : "failure", nil)
    }
}
