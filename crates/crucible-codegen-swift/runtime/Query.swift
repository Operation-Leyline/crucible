import Foundation

/// An exact decimal for JMESPath arithmetic: never rounds through Double.
internal struct QueryDecimal: Sendable {
    var negative: Bool
    /// Big-endian digits without leading or trailing zeros; empty means zero.
    var digits: [UInt8]
    var exponent: Int

    init(negative: Bool, digits: [UInt8], exponent: Int) {
        var digits = Array(digits.drop(while: { $0 == 0 }))
        var exponent = exponent
        while digits.last == 0 { digits.removeLast(); exponent += 1 }
        self.negative = negative && !digits.isEmpty; self.digits = digits; self.exponent = digits.isEmpty ? 0 : exponent
    }
    init(_ value: JSONValue) throws {
        guard case .number(let raw) = value else { throw WireError("JMESPath expected number") }
        let negative = raw.hasPrefix("-")
        let unsigned = negative ? String(raw.dropFirst()) : raw
        let parts = unsigned.lowercased().split(separator: "e", omittingEmptySubsequences: false)
        let decimal = parts[0].split(separator: ".", omittingEmptySubsequences: false)
        guard let shift = parts.count == 2 ? Int(parts[1]) : 0, shift.magnitude <= 1_000_000 else {
            throw WireError("JMESPath arithmetic exponent exceeds supported precision")
        }
        let fraction = decimal.count == 2 ? decimal[1] : ""
        self.init(negative: negative, digits: (decimal[0] + fraction).compactMap { $0.wholeNumberValue.map(UInt8.init) },
                  exponent: shift - fraction.count)
    }
    var isZero: Bool { digits.isEmpty }

    static func compare(_ left: QueryDecimal, _ right: QueryDecimal) -> Int {
        let ls = left.isZero ? 0 : left.negative ? -1 : 1, rs = right.isZero ? 0 : right.negative ? -1 : 1
        if ls != rs { return ls < rs ? -1 : 1 }
        if ls == 0 { return 0 }
        let lm = left.digits.count + left.exponent, rm = right.digits.count + right.exponent
        if lm != rm { return ls * (lm < rm ? -1 : 1) }
        let count = max(left.digits.count, right.digits.count)
        for index in 0..<count {
            let a = index < left.digits.count ? left.digits[index] : 0
            let b = index < right.digits.count ? right.digits[index] : 0
            if a != b { return ls * (a < b ? -1 : 1) }
        }
        return 0
    }

    private func scaled(to exponent: Int) throws -> [UInt8] {
        guard self.exponent - exponent <= 100_000 else { throw WireError("JMESPath arithmetic exceeds supported precision") }
        return digits + Array(repeating: 0, count: self.exponent - exponent)
    }
    private static func magnitudeLess(_ a: [UInt8], _ b: [UInt8]) -> Bool {
        let a = Array(a.drop(while: { $0 == 0 })), b = Array(b.drop(while: { $0 == 0 }))
        return a.count != b.count ? a.count < b.count : a.lexicographicallyPrecedes(b)
    }
    private static func add(_ a: [UInt8], _ b: [UInt8]) -> [UInt8] {
        var result: [UInt8] = []; var carry: UInt8 = 0
        var i = a.count - 1, j = b.count - 1
        while i >= 0 || j >= 0 || carry > 0 {
            let sum = (i >= 0 ? a[i] : 0) + (j >= 0 ? b[j] : 0) + carry
            result.append(sum % 10); carry = sum / 10; i -= 1; j -= 1
        }
        return result.reversed()
    }
    /// a - b where |a| >= |b|.
    private static func subtract(_ a: [UInt8], _ b: [UInt8]) -> [UInt8] {
        var result: [UInt8] = []; var borrow = 0
        var i = a.count - 1, j = b.count - 1
        while i >= 0 {
            var value = Int(a[i]) - borrow - (j >= 0 ? Int(b[j]) : 0)
            borrow = value < 0 ? 1 : 0; if value < 0 { value += 10 }
            result.append(UInt8(value)); i -= 1; j -= 1
        }
        return result.reversed()
    }
    static func + (left: QueryDecimal, right: QueryDecimal) throws -> QueryDecimal {
        if left.isZero { return right }; if right.isZero { return left }
        let exponent = min(left.exponent, right.exponent)
        let a = try left.scaled(to: exponent), b = try right.scaled(to: exponent)
        if left.negative == right.negative { return QueryDecimal(negative: left.negative, digits: add(a, b), exponent: exponent) }
        if magnitudeLess(a, b) { return QueryDecimal(negative: right.negative, digits: subtract(b, a), exponent: exponent) }
        return QueryDecimal(negative: left.negative, digits: subtract(a, b), exponent: exponent)
    }
    /// Division by a positive count, exact or to 34 fractional digits.
    func divided(by divisor: Int) -> QueryDecimal {
        var quotient: [UInt8] = []; var remainder = 0; var exponent = self.exponent
        var source = digits[...]
        while true {
            if let digit = source.first { source = source.dropFirst(); remainder = remainder * 10 + Int(digit) }
            else { if remainder == 0 || exponent <= -34 { break }; remainder *= 10; exponent -= 1 }
            quotient.append(UInt8(remainder / divisor)); remainder %= divisor
        }
        return QueryDecimal(negative: negative, digits: quotient, exponent: exponent)
    }
    func rounded(up: Bool) -> QueryDecimal {
        guard exponent < 0 else { return self }
        let whole = Array(digits.dropLast(min(digits.count, -exponent)))
        let away = up != negative
        return QueryDecimal(negative: negative, digits: away ? QueryDecimal.add(whole, [1]) : whole, exponent: 0)
    }
    var json: JSONValue {
        if isZero { return .number("0") }
        let sign = negative ? "-" : ""
        let text = digits.map { String($0) }.joined()
        if exponent >= 0 {
            return .number(exponent <= 64 ? sign + text + String(repeating: "0", count: exponent) : sign + text + "e" + String(exponent))
        }
        let places = -exponent
        if places >= digits.count { return .number(sign + "0." + String(repeating: "0", count: places - digits.count) + text) }
        return .number(sign + text.dropLast(places) + "." + text.suffix(places))
    }
}

/// Evaluates the portable compiled JMESPath tree from crucible-smithy.
internal enum JMESPath {
    static func evaluate(_ node: JSONValue, _ input: JSONValue) throws -> JSONValue {
        func eval(_ key: String) throws -> JSONValue { try evaluate(node[key] ?? .null, input) }
        switch node["kind"]?.text {
        case "identity": return input
        case "literal": return node["value"] ?? .null
        case "field": return input[try node["name"]!.string] ?? .null
        case "index":
            guard case .array(let list) = input else { return .null }
            var index = try integer(node["index"]); if index < 0 { index += list.count }
            return list.indices.contains(index) ? list[index] : .null
        case "subexpression": return try evaluate(node["right"]!, eval("left"))
        case "projection":
            guard case .array(let list) = try eval("left") else { return .null }
            return .array(try list.map { try evaluate(node["right"]!, $0) }.filter { $0 != .null })
        case "flatten":
            guard case .array(let list) = try eval("expression") else { return .null }
            return .array(list.flatMap { item -> [JSONValue] in if case .array(let nested) = item { return nested }; return [item] })
        case "objectValues":
            guard case .object(let object) = try eval("expression") else { return .null }
            return .array(sortedKeys(object).map { object[$0]! })
        case "multiList":
            if input == .null { return .null }
            return .array(try node["expressions"]!.array.map { try evaluate($0, input) })
        case "multiHash":
            if input == .null { return .null }
            var result: [String: JSONValue] = [:]
            for entry in try node["entries"]!.array { result[try entry["key"]!.string] = try evaluate(entry["value"]!, input) }
            return .object(result)
        case "and": let left = try eval("left"); return truthy(left) ? try eval("right") : left
        case "or": let left = try eval("left"); return truthy(left) ? left : try eval("right")
        case "not": return .bool(!truthy(try eval("expression")))
        case "condition": return truthy(try eval("predicate")) ? try eval("then") : .null
        case "compare":
            let left = try eval("left"), right = try eval("right")
            let comparator = try node["comparator"]!.string
            if comparator == "equal" { return .bool(try left.equivalent(to: right)) }
            if comparator == "notEqual" { return .bool(!(try left.equivalent(to: right))) }
            guard case .number = left, case .number = right else { return .null }
            let order = QueryDecimal.compare(try QueryDecimal(left), try QueryDecimal(right))
            switch comparator {
            case "lessThan": return .bool(order < 0)
            case "lessThanEqual": return .bool(order <= 0)
            case "greaterThan": return .bool(order > 0)
            case "greaterThanEqual": return .bool(order >= 0)
            default: throw WireError("Unknown JMESPath comparator")
            }
        case "slice":
            guard case .array(let list) = input else { return .null }
            let step = try integer(node["step"]); guard step != 0 else { throw WireError("Zero slice step") }
            func boundary(_ key: String, _ fallback: Int) throws -> Int {
                guard let raw = node[key], raw != .null else { return fallback }
                var value = try integer(raw); if value < 0 { value += list.count }
                return step > 0 ? min(list.count, max(0, value)) : min(list.count - 1, max(-1, value))
            }
            let start = try boundary("start", step > 0 ? 0 : list.count - 1)
            let stop = try boundary("stop", step > 0 ? list.count : -1)
            var result: [JSONValue] = []; var index = start
            while step > 0 ? index < stop : index > stop { result.append(list[index]); index += step }
            return .array(result)
        case "function": return try function(try node["function"]!.string, try node["arguments"]!.array, input)
        default: throw WireError("Unknown compiled JMESPath node")
        }
    }

    static func truthy(_ value: JSONValue) -> Bool {
        switch value {
        case .null: return false
        case .bool(let flag): return flag
        case .string(let text): return !text.isEmpty
        case .array(let list): return !list.isEmpty
        case .object(let object): return !object.isEmpty
        case .number: return true
        }
    }
    private static func integer(_ value: JSONValue?) throws -> Int {
        guard case .number(let text)? = value, let number = Int(text) else { throw WireError("JMESPath integer out of range") }
        return number
    }
    private static func sortedKeys(_ object: [String: JSONValue]) -> [String] {
        object.keys.sorted { $0.utf8.lexicographicallyPrecedes($1.utf8) }
    }
    private static func text(_ value: JSONValue) throws -> String {
        guard case .string(let text) = value else { throw WireError("JMESPath expected string") }; return text
    }
    private static func array(_ value: JSONValue) throws -> [JSONValue] {
        guard case .array(let list) = value else { throw WireError("JMESPath expected array") }; return list
    }
    private static func object(_ value: JSONValue) throws -> [String: JSONValue] {
        guard case .object(let object) = value else { throw WireError("JMESPath expected object") }; return object
    }
    private static func order(_ left: JSONValue, _ right: JSONValue) throws -> Int {
        if case .number = left { return QueryDecimal.compare(try QueryDecimal(left), try QueryDecimal(right)) }
        let a = try text(left), b = try text(right)
        return a == b ? 0 : a.utf8.lexicographicallyPrecedes(b.utf8) ? -1 : 1
    }
    /// Sort keys must be all numbers or all strings.
    private static func ordered(_ values: [JSONValue]) throws -> [Int] {
        if let first = values.first {
            for value in values {
                if case .number = first { _ = try QueryDecimal(value) } else { _ = try text(value) }
            }
        }
        return try values.indices.sorted { a, b in
            let result = try order(values[a], values[b]); return result < 0 || result == 0 && a < b
        }
    }
    private static func reference(_ nodes: [JSONValue], _ index: Int, _ input: JSONValue) throws -> JSONValue {
        guard nodes[index]["kind"]?.text == "expressionReference" else { throw WireError("JMESPath expected expression reference") }
        return try evaluate(nodes[index]["expression"]!, input)
    }

    private static func function(_ name: String, _ nodes: [JSONValue], _ input: JSONValue) throws -> JSONValue {
        let variadic = name == "merge" || name == "not_null"
        let arity = ["contains", "starts_with", "ends_with", "join", "map", "max_by", "min_by", "sort_by"].contains(name) ? 2 : 1
        guard variadic ? !nodes.isEmpty : nodes.count == arity else { throw WireError("JMESPath invalid function arity") }
        func arg(_ index: Int) throws -> JSONValue { try evaluate(nodes[index], input) }
        switch name {
        case "abs": var value = try QueryDecimal(arg(0)); value.negative = false; return value.json
        case "ceil", "floor": return try QueryDecimal(arg(0)).rounded(up: name == "ceil").json
        case "sum", "avg":
            let list = try array(arg(0))
            var total = QueryDecimal(negative: false, digits: [], exponent: 0)
            for item in list { total = try total + QueryDecimal(item) }
            if name == "sum" { return total.json }
            return list.isEmpty ? .null : total.divided(by: list.count).json
        case "contains":
            let source = try arg(0), wanted = try arg(1)
            if case .string(let haystack) = source {
                let needle = try text(wanted)
                return .bool(needle.isEmpty || haystack.contains(needle))
            }
            return .bool(try array(source).contains { try $0.equivalent(to: wanted) })
        case "starts_with": return .bool(try text(arg(0)).hasPrefix(try text(arg(1))))
        case "ends_with": return .bool(try text(arg(0)).hasSuffix(try text(arg(1))))
        case "join":
            let glue = try text(arg(0))
            return .string(try array(arg(1)).map(text).joined(separator: glue))
        case "keys": return .array(sortedKeys(try object(arg(0))).map(JSONValue.string))
        case "values": let value = try object(arg(0)); return .array(sortedKeys(value).map { value[$0]! })
        case "length":
            switch try arg(0) {
            case .array(let list): return .number(String(list.count))
            case .object(let object): return .number(String(object.count))
            case let value: return .number(String(try text(value).unicodeScalars.count))
            }
        case "map": return .array(try array(arg(1)).map { try reference(nodes, 0, $0) })
        case "max", "min", "sort":
            let list = try array(arg(0)); let indices = try ordered(list)
            if name == "sort" { return .array(indices.map { list[$0] }) }
            guard let selected = name == "max" ? indices.last : indices.first else { return .null }
            return list[selected]
        case "max_by", "min_by", "sort_by":
            let list = try array(arg(0))
            let keys = try list.map { try reference(nodes, 1, $0) }
            let indices = try ordered(keys)
            if name == "sort_by" { return .array(indices.map { list[$0] }) }
            guard !list.isEmpty else { return .null }
            var selected = 0
            for index in 1..<list.count {
                let result = try order(keys[index], keys[selected])
                if name == "max_by" ? result > 0 : result < 0 { selected = index }
            }
            return list[selected]
        case "merge":
            var result: [String: JSONValue] = [:]
            for index in nodes.indices { for (key, value) in try object(arg(index)) { result[key] = value } }
            return .object(result)
        case "not_null":
            for index in nodes.indices { let value = try arg(index); if value != .null { return value } }
            return .null
        case "reverse":
            let value = try arg(0)
            if case .array(let list) = value { return .array(list.reversed()) }
            var scalars = String.UnicodeScalarView(); scalars.append(contentsOf: try text(value).unicodeScalars.reversed())
            return .string(String(scalars))
        case "to_array": let value = try arg(0); if case .array = value { return value }; return .array([value])
        case "to_number":
            switch try arg(0) {
            case .number(let text): return .number(text)
            case .string(let text):
                guard case .number? = try? JSONValue(data: Data(text.utf8)), (try? QueryDecimal(.number(text))) != nil else { return .null }
                return .number(text)
            default: return .null
            }
        case "to_string":
            let value = try arg(0); if case .string = value { return value }
            return .string(String(decoding: try value.data(), as: UTF8.self))
        case "type":
            switch try arg(0) {
            case .null: return .string("null")
            case .bool: return .string("boolean")
            case .string: return .string("string")
            case .number: return .string("number")
            case .array: return .string("array")
            case .object: return .string("object")
            }
        default: throw WireError("Unknown compiled JMESPath function: \(name)")
        }
    }
}

extension WireCodec {
    /// The JMESPath view of a member-named value: timestamps become epoch seconds.
    func queryView(_ target: String?, _ value: JSONValue, format: String? = nil) throws -> JSONValue {
        guard let target, value != .null else { return value }
        let shape = try shape(target)
        switch shape["kind"]?.text {
        case "timestamp":
            let timestamp = try Timestamp.fromWire(value, context(target, format: format))
            return try timestamp.toWire(context(target, format: "epoch-seconds"))
        case "structure", "union":
            guard case .object(let object) = value else { return value }
            var result: [String: JSONValue] = [:]
            for (name, item) in object {
                if let member = shape["members"]?[name] {
                    result[name] = try queryView(member["target"]?.text, item, format: member["format"]?.text)
                } else { result[name] = item }
            }
            return .object(result)
        case "list":
            guard case .array(let list) = value, let member = shape["member"] else { return value }
            return .array(try list.map { try queryView(member["target"]?.text, $0, format: member["format"]?.text) })
        case "map":
            guard case .object(let object) = value, let member = shape["value"] else { return value }
            return .object(try object.mapValues { try queryView(member["target"]?.text, $0, format: member["format"]?.text) })
        default: return value
        }
    }
}
