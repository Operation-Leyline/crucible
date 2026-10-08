import Foundation

/// A JSON document retaining the original spelling of every numeric token.
public indirect enum JSONValue: Sendable, Equatable, CustomStringConvertible {
    case null
    case bool(Bool)
    case string(String)
    case number(String)
    case array([JSONValue])
    case object([String: JSONValue])

    /// The document as JSON text.
    public var description: String {
        (try? data()).flatMap { String(data: $0, encoding: .utf8) } ?? "<invalid JSON>"
    }
    /// Parses `data`, keeping every numeric token exactly. Throws ``WireError`` for invalid or trailing input.
    public init(data: Data) throws {
        var reader = JSONReader(bytes: Array(data))
        self = try reader.value(depth: 0)
        reader.whitespace()
        guard reader.offset == reader.bytes.count else { throw WireError("Trailing JSON data") }
    }
    /// Encodes the document as UTF-8 JSON.
    public func data() throws -> Data {
        Data(try rendered().utf8)
    }
    private func rendered(depth: Int = 0) throws -> String {
        guard depth <= 255 else { throw WireError("JSON nesting limit exceeded") }
        switch self {
        case .null: return "null"
        case .bool(let value): return value ? "true" : "false"
        case .string(let value):
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.withoutEscapingSlashes]
            return String(decoding: try encoder.encode(value), as: UTF8.self)
        case .number(let value):
            guard case .number = try JSONValue(data: Data(value.utf8)) else { throw WireError("Invalid numeric token") }
            return value
        case .array(let values): return "[" + (try values.map { try $0.rendered(depth: depth + 1) }).joined(separator: ",") + "]"
        case .object(let values):
            return "{" + (try values.keys.sorted().map { key in
                try JSONValue.string(key).rendered() + ":" + values[key]!.rendered(depth: depth + 1)
            }).joined(separator: ",") + "}"
        }
    }
    internal subscript(_ key: String) -> JSONValue? {
        guard case .object(let value) = self else { return nil }
        return value[key]
    }
    internal var object: [String: JSONValue] {
        get throws { guard case .object(let value) = self else { throw WireError("Expected object") }; return value }
    }
    internal var array: [JSONValue] {
        get throws { guard case .array(let value) = self else { throw WireError("Expected array") }; return value }
    }
    internal var string: String {
        get throws { guard case .string(let value) = self else { throw WireError("Expected string") }; return value }
    }
    internal var number: String {
        get throws { guard case .number(let value) = self else { throw WireError("Expected number") }; return value }
    }
    internal var text: String? { if case .string(let value) = self { return value }; return nil }
    internal var flag: Bool { if case .bool(let value) = self { return value }; return false }
}

/// A value could not be encoded or decoded, or a setting was invalid.
public struct WireError: Error, Sendable, CustomStringConvertible {
    public let description: String
    public init(_ message: String) { description = message }
}

private struct JSONReader {
    let bytes: [UInt8]
    var offset = 0
    mutating func whitespace() {
        while offset < bytes.count && [9, 10, 13, 32].contains(bytes[offset]) { offset += 1 }
    }
    mutating func consume(_ byte: UInt8) -> Bool {
        if offset < bytes.count && bytes[offset] == byte { offset += 1; return true }
        return false
    }
    mutating func expect(_ byte: UInt8) throws {
        guard consume(byte) else { throw WireError("Unexpected JSON token") }
    }
    mutating func string() throws -> String {
        let start = offset
        try expect(34)
        var escaped = false
        while offset < bytes.count {
            let byte = bytes[offset]; offset += 1
            if escaped { escaped = false }
            else if byte == 92 { escaped = true }
            else if byte == 34 { return try JSONDecoder().decode(String.self, from: Data(bytes[start..<offset])) }
        }
        throw WireError("Unterminated JSON string")
    }
    mutating func value(depth: Int) throws -> JSONValue {
        guard depth < 256 else { throw WireError("JSON nesting exceeds 255 levels") }
        whitespace()
        guard offset < bytes.count else { throw WireError("Missing JSON value") }
        switch bytes[offset] {
        case 34: return .string(try string())
        case 123:
            offset += 1; whitespace()
            var result: [String: JSONValue] = [:]
            if consume(125) { return .object(result) }
            while true {
                let key = try string(); whitespace(); try expect(58)
                result[key] = try value(depth: depth + 1); whitespace()
                if consume(125) { return .object(result) }
                try expect(44); whitespace()
            }
        case 91:
            offset += 1; whitespace()
            var result: [JSONValue] = []
            if consume(93) { return .array(result) }
            while true {
                result.append(try value(depth: depth + 1)); whitespace()
                if consume(93) { return .array(result) }
                try expect(44); whitespace()
            }
        case 116: try word("true"); return .bool(true)
        case 102: try word("false"); return .bool(false)
        case 110: try word("null"); return .null
        default:
            let start = offset
            _ = consume(45)
            if !consume(48) { try digits() }
            if consume(46) { try digits() }
            if consume(101) || consume(69) { if !consume(43) { _ = consume(45) }; try digits() }
            return .number(String(decoding: bytes[start..<offset], as: UTF8.self))
        }
    }
    private mutating func digits() throws {
        let start = offset
        while offset < bytes.count && (48...57).contains(bytes[offset]) { offset += 1 }
        guard offset > start else { throw WireError("Invalid JSON number") }
    }
    private mutating func word(_ word: String) throws {
        for byte in word.utf8 { try expect(byte) }
    }
}

/// Converts numeric tokens without passing through floating point.
internal struct WireNumber {
    let negative: Bool
    let digits: String
    let point: Int
    init(_ raw: String) throws {
        guard case .number = try JSONValue(data: Data(raw.utf8)) else { throw WireError("Expected number") }
        negative = raw.hasPrefix("-")
        let unsigned = negative ? String(raw.dropFirst()) : raw
        let parts = unsigned.lowercased().split(separator: "e", omittingEmptySubsequences: false)
        let decimal = parts[0].split(separator: ".", omittingEmptySubsequences: false)
        let coefficient = decimal.map(String.init).joined()
        digits = String(coefficient.drop(while: { $0 == "0" }))
        // Typed numbers have bounded precision. Documents retain any valid exponent.
        guard let exponent = parts.count == 1 ? 0 : Int(parts[1]), (-10000...10000).contains(exponent) else { throw WireError("Numeric exponent out of range") }
        point = decimal[0].count + exponent - (coefficient.count - digits.count)
    }
    func components(fractionDigits: Int) throws -> (String, String) {
        if digits.allSatisfy({ $0 == "0" }) { return ("0", String(repeating: "0", count: fractionDigits)) }
        guard point <= 20 && point >= -fractionDigits else { throw WireError("Number outside supported precision") }
        let whole: String
        let fraction: String
        if point < 0 { whole = "0"; fraction = String(repeating: "0", count: -point) + digits }
        else if point >= digits.count { whole = digits + String(repeating: "0", count: point - digits.count); fraction = "" }
        else { whole = String(digits.prefix(point)); fraction = String(digits.dropFirst(point)) }
        guard fraction.dropFirst(fractionDigits).allSatisfy({ $0 == "0" }) else { throw WireError("Number exceeds supported precision") }
        let trimmed = whole.drop(while: { $0 == "0" })
        let integer = (negative ? "-" : "") + (trimmed.isEmpty ? "0" : String(trimmed))
        let padded = String(fraction.prefix(fractionDigits)) + String(repeating: "0", count: max(0, fractionDigits - fraction.count))
        return (integer, padded)
    }
    func integer<T: FixedWidthInteger>(_ type: T.Type) throws -> T {
        let (whole, _) = try components(fractionDigits: 0)
        guard let result = T(whole) else { throw WireError("Integer overflow") }
        return result
    }
}
