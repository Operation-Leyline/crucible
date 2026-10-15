import Foundation

/// Distinguishes an omitted member from JSON null and a supplied value.
public indirect enum OptionalValue<Value: Sendable>: Sendable {
    /// The member was omitted.
    case unset
    /// The member was sent as an explicit null.
    case null
    /// The member has a value.
    case value(Value)
    /// Whether the member was omitted (false) or present, null or not (true).
    public var isSet: Bool { if case .unset = self { return false }; return true }
    /// The value, or nil when omitted or null.
    public var value: Value? { if case .value(let value) = self { return value }; return nil }
}
extension OptionalValue: Equatable where Value: Equatable {}

internal protocol WireCodable: Sendable {
    static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Self
    func toWire(_ context: WireContext) throws -> JSONValue
}
internal struct WireContext: Sendable {
    let codec: WireCodec
    let target: String
    let format: String
    var shape: JSONValue { get throws { try codec.shape(target) } }
    func child(_ member: JSONValue) throws -> WireContext {
        guard let target = member["target"]?.text else { throw WireError("Schema target missing") }
        return try codec.context(target, format: member["format"]?.text)
    }
}
internal struct WireCodec: Sendable {
    let schema: JSONValue
    func shape(_ target: String) throws -> JSONValue {
        guard let shape = schema["shapes"]?[target] else { throw WireError("Unknown shape: \(target)") }
        return shape
    }
    func context(_ target: String, format: String? = nil) throws -> WireContext {
        WireContext(codec: self, target: target, format: try format ?? shape(target)["format"]?.text ?? "date-time")
    }
    func encode<T: WireCodable>(_ target: String, _ value: T) throws -> JSONValue {
        try value.toWire(context(target))
    }
    func decode<T: WireCodable>(_ target: String, _ value: JSONValue, as: T.Type) throws -> T {
        try T.fromWire(value, context(target))
    }
    // Members the shape does not declare are dropped; member names are the JSON names.
    func document(_ target: String, _ value: JSONValue, encode: Bool) throws -> JSONValue {
        if value == .null { return value }
        let shape = try shape(target)
        switch shape["kind"]?.text {
        case "structure", "union":
            let object = try value.object
            let members = try shape["members"]!.object
            var result: [String: JSONValue] = [:]
            for (name, member) in members {
                if let item = object[name] { result[name] = try document(member["target"]!.string, item, encode: encode) }
            }
            if shape["kind"]?.text == "union" {
                for (name, item) in object where members[name] == nil { result[name] = item }
            }
            return .object(result)
        case "list":
            return .array(try value.array.map { try document(shape["member"]!["target"]!.string, $0, encode: encode) })
        case "map":
            var result: [String: JSONValue] = [:]
            for (name, item) in try value.object { result[name] = try document(shape["value"]!["target"]!.string, item, encode: encode) }
            return .object(result)
        default: return value
        }
    }
    func member(_ target: String, _ name: String) throws -> JSONValue {
        guard let member = try shape(target)["members"]?[name] else { throw WireError("Unknown member: \(name)") }
        return member
    }
    func read<T: WireCodable>(_ target: String, _ name: String, _ object: [String: JSONValue], as: T.Type) throws -> T {
        let member = try member(target, name)
        let value = object[name] ?? member["default"] ?? .null
        return try T.fromWire(value, context(target).child(member))
    }
    func optional<T: WireCodable>(_ target: String, _ name: String, _ object: [String: JSONValue], as: T.Type) throws -> OptionalValue<T> {
        let member = try member(target, name)
        guard let value = object[name] ?? member["default"].flatMap({ $0 == .null ? nil : $0 }) else { return .unset }
        if value == .null { return .null }
        return .value(try T.fromWire(value, context(target).child(member)))
    }
    func put<T: WireCodable>(_ target: String, _ name: String, _ value: T, into object: inout [String: JSONValue]) throws {
        object[name] = try value.toWire(context(target).child(member(target, name)))
    }
    func put<T: WireCodable>(_ target: String, _ name: String, _ value: OptionalValue<T>, into object: inout [String: JSONValue]) throws {
        switch value {
        case .unset: break
        case .null: object[name] = .null
        case .value(let value): try put(target, name, value, into: &object)
        }
    }
    /// Re-encodes a member value from its document format into an HTTP binding format.
    func rebind(_ member: JSONValue, _ value: JSONValue, format: String) throws -> JSONValue {
        let target = try member["target"]!.string
        let shape = try shape(target)
        switch shape["kind"]?.text {
        case "timestamp":
            return try Timestamp.fromWire(value, context(target, format: member["format"]?.text)).toWire(context(target, format: format))
        case "list": return .array(try value.array.map { try rebind(shape["member"]!, $0, format: format) })
        default: return value
        }
    }
    func header(_ member: JSONValue, _ text: String, format: String? = nil) throws -> JSONValue {
        let shape = try shape(member["target"]!.string)
        if let format, shape["kind"]?.text == "timestamp" {
            let target = try member["target"]!.string
            let wire = format == "epoch-seconds" ? try JSONValue(data: Data(text.utf8)) : .string(text)
            return try Timestamp.fromWire(wire, context(target, format: format)).toWire(context(target, format: member["format"]?.text))
        }
        switch shape["kind"]?.text {
        case "list":
            if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return .array([]) }
            var values: [JSONValue] = []; var part = ""; var quoted = false; var escaped = false
            func append(_ part: String) throws {
                let trimmed = part.trimmingCharacters(in: .whitespacesAndNewlines)
                let decoded = trimmed.hasPrefix("\"") ? try JSONValue(data: Data(trimmed.utf8)).string : trimmed
                values.append(try header(shape["member"]!, decoded, format: format))
            }
            let element = shape["member"]!
            if try self.shape(element["target"]!.string)["kind"]?.text == "timestamp", (format ?? element["format"]?.text) == "http-date",
               !text.drop(while: \.isWhitespace).hasPrefix("\"") {
                // Every IMF-fixdate holds a comma and the protocols write these lists unquoted, so only a comma
                // that is followed by the next date's weekday separates two values.
                let weekdays = ["Mon,", "Tue,", "Wed,", "Thu,", "Fri,", "Sat,", "Sun,"]
                for index in text.indices {
                    if text[index] == "," {
                        let next = text[text.index(after: index)...].drop(while: \.isWhitespace)
                        if weekdays.contains(where: { next.hasPrefix($0) }) { try append(part); part = ""; continue }
                    }
                    part.append(text[index])
                }
                try append(part)
                return .array(values)
            }
            for character in text {
                if character == "," && !quoted { try append(part); part = ""; continue }
                part.append(character)
                if escaped { escaped = false }
                else if character == "\\" && quoted { escaped = true }
                else if character == "\"" { quoted.toggle() }
            }
            guard !quoted else { throw WireError("Unterminated quoted header") }
            try append(part)
            return .array(values)
        case "byte", "short", "integer", "long", "intEnum", "bigInteger", "bigDecimal", "float", "double", "boolean": return try JSONValue(data: Data(text.utf8))
        case "timestamp" where member["format"]?.text == "epoch-seconds": return try JSONValue(data: Data(text.utf8))
        default: return .string(text)
        }
    }
    func bound(_ value: JSONValue) throws -> String {
        switch value {
        case .string(let value), .number(let value): return value
        case .bool(let value): return value ? "true" : "false"
        case .array(let values):
            return try values.map { item in
                let value = try bound(item)
                if value.isEmpty || value.contains(",") || value.contains("\"") || value.contains("\\") || value.trimmingCharacters(in: .whitespacesAndNewlines) != value {
                    return String(decoding: try JSONValue.string(value).data(), as: UTF8.self)
                }
                return value
            }.joined(separator: ", ")
        default: throw WireError("Expected bound scalar")
        }
    }
}

extension String: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> String { try value.string }
    internal func toWire(_ context: WireContext) throws -> JSONValue { .string(self) }
}
extension Bool: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Bool {
        guard case .bool(let result) = value else { throw WireError("Expected boolean") }; return result
    }
    internal func toWire(_ context: WireContext) throws -> JSONValue { .bool(self) }
}
internal protocol WireInteger: FixedWidthInteger, WireCodable {}
extension WireInteger {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Self { try WireNumber(value.number).integer(Self.self) }
    internal func toWire(_ context: WireContext) throws -> JSONValue { .number(String(self)) }
}
extension Int8: WireInteger {}
extension Int16: WireInteger {}
extension Int32: WireInteger {}
extension Int64: WireInteger {}
extension Float: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Float {
        guard let result = try Float(value.number), result.isFinite else { throw WireError("Float overflow") }; return result
    }
    internal func toWire(_ context: WireContext) throws -> JSONValue {
        guard isFinite else { throw WireError("Non-finite float") }; return .number(String(self))
    }
}
extension Double: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Double {
        guard let result = try Double(value.number), result.isFinite else { throw WireError("Double overflow") }; return result
    }
    internal func toWire(_ context: WireContext) throws -> JSONValue {
        guard isFinite else { throw WireError("Non-finite double") }; return .number(String(self))
    }
}
extension Data: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Data {
        guard let bytes = try Data(base64Encoded: value.string) else { throw WireError("Invalid base64 blob") }; return bytes
    }
    internal func toWire(_ context: WireContext) throws -> JSONValue { .string(base64EncodedString()) }
}
extension JSONValue: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> JSONValue { value }
    internal func toWire(_ context: WireContext) throws -> JSONValue { self }
}
extension Array: WireCodable where Element: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Self {
        let child = try context.child(context.shape["member"]!)
        let values = try value.array
        try context.codec.collection(values, shape: context.shape)
        return try values.map { try Element.fromWire($0, child) }
    }
    internal func toWire(_ context: WireContext) throws -> JSONValue {
        let child = try context.child(context.shape["member"]!)
        let values = try map { try $0.toWire(child) }
        try context.codec.collection(values, shape: context.shape)
        return .array(values)
    }
}
extension Dictionary: WireCodable where Key: WireCodable, Value: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Self {
        let shape = try context.shape
        let key = try context.child(shape["key"]!); let item = try context.child(shape["value"]!)
        var result: Self = [:]
        for (name, value) in try value.object {
            if value == .null && shape["sparse"]?.flag != true { throw WireError("Null in dense map") }
            result.updateValue(try Value.fromWire(value, item), forKey: try Key.fromWire(.string(name), key))
        }
        return result
    }
    internal func toWire(_ context: WireContext) throws -> JSONValue {
        let shape = try context.shape
        let key = try context.child(shape["key"]!); let item = try context.child(shape["value"]!)
        var result: [String: JSONValue] = [:]
        for (name, value) in self {
            let encoded = try value.toWire(item)
            if encoded == .null && shape["sparse"]?.flag != true { throw WireError("Null in dense map") }
            result[try name.toWire(key).string] = encoded
        }
        return .object(result)
    }
}
extension Optional: WireCodable where Wrapped: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Self {
        value == .null ? nil : try Wrapped.fromWire(value, context)
    }
    internal func toWire(_ context: WireContext) throws -> JSONValue {
        try map { try $0.toWire(context) } ?? .null
    }
}
extension WireCodec {
    func collection(_ values: [JSONValue], shape: JSONValue) throws {
        if shape["sparse"]?.flag != true && values.contains(.null) { throw WireError("Null in dense list") }
        if shape["set"]?.flag == true {
            for index in values.indices {
                if try values[..<index].contains(where: { try $0.equivalent(to: values[index]) }) { throw WireError("Duplicate set member") }
            }
        }
    }
}
