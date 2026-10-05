import Foundation

/// An exact integer, including values beyond machine integer limits.
public struct BigInteger: Sendable, Hashable, CustomStringConvertible, WireCodable {
    public let description: String
    private let exact: ExactNumber
    public init(_ value: String) throws {
        exact = try ExactNumber(value)
        guard !exact.exponent.hasPrefix("-") else { throw WireError("Expected integral number") }
        description = value
    }
    public static func == (lhs: Self, rhs: Self) -> Bool { lhs.exact == rhs.exact }
    public func hash(into hasher: inout Hasher) { hasher.combine(exact) }
    static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Self { try Self(value.number) }
    func toWire(_ context: WireContext) throws -> JSONValue { .number(description) }
}

/// An exact decimal. Construction and serialization never round through Double.
public struct BigDecimal: Sendable, Hashable, CustomStringConvertible, WireCodable {
    public let description: String
    private let exact: ExactNumber
    public init(_ value: String) throws { exact = try ExactNumber(value); description = value }
    public static func == (lhs: Self, rhs: Self) -> Bool { lhs.exact == rhs.exact }
    public func hash(into hasher: inout Hasher) { hasher.combine(exact) }
    static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Self { try Self(value.number) }
    func toWire(_ context: WireContext) throws -> JSONValue { .number(description) }
}

// Normalize the coefficient and exponent separately: even a huge exponent does
// not allocate a string proportional to the represented integer's magnitude.
internal struct ExactNumber: Sendable, Hashable {
    let coefficient: String
    let exponent: String
    init(_ raw: String) throws {
        guard case .number = try JSONValue(data: Data(raw.utf8)) else { throw WireError("Expected number") }
        let negative = raw.hasPrefix("-")
        let unsigned = negative ? String(raw.dropFirst()) : raw
        let parts = unsigned.lowercased().split(separator: "e", omittingEmptySubsequences: false)
        let decimal = parts[0].split(separator: ".", omittingEmptySubsequences: false)
        var digits = decimal.joined().drop(while: { $0 == "0" })
        if digits.isEmpty { coefficient = "0"; exponent = "0"; return }
        let initial = digits.count
        while digits.last == "0" { digits = digits.dropLast() }
        coefficient = (negative ? "-" : "") + digits
        let adjustment = initial - digits.count - (decimal.count == 2 ? decimal[1].count : 0)
        exponent = Self.add(parts.count == 2 ? String(parts[1]) : "0", adjustment)
    }
    private static func add(_ raw: String, _ amount: Int) -> String {
        var negative = raw.hasPrefix("-")
        var digits = Array(raw.drop(while: { $0 == "-" || $0 == "+" || $0 == "0" }).reversed()).map { $0.wholeNumberValue! }
        if digits.isEmpty { digits = [0]; negative = false }
        var remaining = amount
        // The adjustment is bounded by input length, not the exponent value.
        while remaining != 0 {
            let subtract = (remaining < 0) != negative
            if subtract {
                if digits == [0] { negative.toggle(); digits = [1] }
                else {
                    var index = 0
                    while digits[index] == 0 { digits[index] = 9; index += 1 }
                    digits[index] -= 1
                    while digits.count > 1 && digits.last == 0 { digits.removeLast() }
                    if digits == [0] { negative = false }
                }
            } else {
                var index = 0
                while index < digits.count && digits[index] == 9 { digits[index] = 0; index += 1 }
                if index == digits.count { digits.append(1) } else { digits[index] += 1 }
            }
            remaining += remaining < 0 ? 1 : -1
        }
        return (negative ? "-" : "") + digits.reversed().map(String.init).joined()
    }
}

extension JSONValue {
    internal func equivalent(to other: JSONValue) throws -> Bool {
        switch (self, other) {
        case (.number(let left), .number(let right)): return try ExactNumber(left) == ExactNumber(right)
        case (.array(let left), .array(let right)):
            return try left.count == right.count && zip(left, right).allSatisfy { try $0.equivalent(to: $1) }
        case (.object(let left), .object(let right)):
            return try left.count == right.count && left.allSatisfy { key, value in
                guard let other = right[key] else { return false }; return try value.equivalent(to: other)
            }
        default: return self == other
        }
    }
}
