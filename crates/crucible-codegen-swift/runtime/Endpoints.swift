import Foundation

internal struct ResolvedEndpoint: Sendable {
    let url: String
    let headers: [String: [String]]
    let properties: [String: JSONValue]
}

/// Evaluates the shared endpoint rules IR; `.null` means an unset value.
internal enum Endpoints {
    static func hostLabel(_ text: String, subdomains: Bool) -> Bool {
        let labels = subdomains ? text.split(separator: ".", omittingEmptySubsequences: false).map(String.init) : [text]
        return labels.allSatisfy { label in
            let bytes = Array(label.utf8)
            func alphanumeric(_ byte: UInt8) -> Bool { (48...57).contains(byte) || (65...90).contains(byte) || (97...122).contains(byte) }
            guard (1...63).contains(bytes.count), alphanumeric(bytes.first!), alphanumeric(bytes.last!) else { return false }
            return bytes.allSatisfy { alphanumeric($0) || $0 == 45 }
        }
    }

    private static func ipAddress(_ host: String) -> Bool {
        let octets = host.split(separator: ".", omittingEmptySubsequences: false)
        if octets.count == 4 && octets.allSatisfy({ !$0.isEmpty && $0.count <= 3 && $0.allSatisfy(\.isASCII) && Int($0).map { (0...255).contains($0) } == true }) {
            return true
        }
        return host.contains(":") && host.allSatisfy { $0.isHexDigit || $0 == ":" || $0 == "." }
    }

    static func parseURL(_ text: String) -> JSONValue? {
        guard !text.unicodeScalars.contains(where: { $0.value <= 32 || $0.value == 127 || $0 == "?" || $0 == "#" }),
              let url = URLComponents(string: text), let scheme = url.scheme?.lowercased(), ["http", "https"].contains(scheme),
              url.user == nil, url.password == nil, var host = url.host, !host.isEmpty else { return nil }
        if host.hasPrefix("[") && host.hasSuffix("]") { host = String(host.dropFirst().dropLast()) }
        let isIP = ipAddress(host)
        if !isIP && !hostLabel(host.hasSuffix(".") ? String(host.dropLast()) : host, subdomains: true) { return nil }
        var authority = host.lowercased()
        if authority.contains(":") { authority = "[\(authority)]" }
        if let port = url.port {
            guard (0...65535).contains(port) else { return nil }
            if port != (scheme == "https" ? 443 : 80) { authority += ":\(port)" }
        }
        let path = url.percentEncodedPath.isEmpty ? "/" : url.percentEncodedPath
        return .object(["scheme": .string(scheme), "authority": .string(authority), "path": .string(path),
                        "normalizedPath": .string(path.hasSuffix("/") ? path : path + "/"), "isIp": .bool(isIP)])
    }

    static func attribute(_ value: JSONValue, _ path: JSONValue) throws -> JSONValue {
        var steps: [JSONValue] = []
        if case .string(let text) = path {
            for part in text.split(separator: ".", omittingEmptySubsequences: false) {
                let pieces = part.split(separator: "[", maxSplits: 1, omittingEmptySubsequences: false)
                if !pieces[0].isEmpty { steps.append(.object(["kind": .string("member"), "name": .string(String(pieces[0]))])) }
                if pieces.count == 2 {
                    guard pieces[1].hasSuffix("]"), let index = Int(pieces[1].dropLast()) else { throw WireError("Invalid endpoint attribute index") }
                    steps.append(.object(["kind": .string("index"), "index": .number(String(index))]))
                }
            }
        } else { steps = try path.array }
        var current = value
        for step in steps {
            if step["kind"]?.text == "member" {
                guard case .object(let object) = current else { return .null }
                current = object[try step["name"]!.string] ?? .null
            } else {
                guard case .array(let list) = current, case .number(let raw)? = step["index"], var index = Int(raw) else { return .null }
                if index < 0 { index += list.count }
                guard list.indices.contains(index) else { return .null }
                current = list[index]
            }
            if current == .null { return .null }
        }
        return current
    }

    static func function(_ name: String, _ args: [JSONValue]) throws -> JSONValue {
        func boolean(_ value: JSONValue) throws -> Bool { guard case .bool(let flag) = value else { throw WireError("Endpoint function requires a boolean") }; return flag }
        func integer(_ value: JSONValue) throws -> Int { guard case .number(let raw) = value, let number = Int(raw) else { throw WireError("Endpoint function requires an integer") }; return number }
        switch name {
        case "isSet": return .bool(args[0] != .null)
        case "not": return .bool(!(try boolean(args[0])))
        case "booleanEquals":
            guard case .bool(let a) = args[0], case .bool(let b) = args[1] else { return .bool(false) }; return .bool(a == b)
        case "stringEquals":
            guard case .string(let a) = args[0], case .string(let b) = args[1] else { return .bool(false) }; return .bool(a == b)
        case "getAttr": return try attribute(args[0], args[1])
        case "substring":
            let text = try args[0].string, start = try integer(args[1]), end = try integer(args[2])
            let bytes = Array(text.utf8)
            guard bytes.allSatisfy({ $0 < 128 }), start >= 0, end >= start, end <= bytes.count else { return .null }
            let range = try boolean(args[3]) ? (bytes.count - end)..<(bytes.count - start) : start..<end
            return .string(String(decoding: bytes[range], as: UTF8.self))
        case "isValidHostLabel": return .bool(hostLabel(try args[0].string, subdomains: try boolean(args[1])))
        case "parseUrl": return parseURL(try args[0].string) ?? .null
        case "uriEncode": return .string(SigV4Signer.uriEncode(try args[0].string))
        default: throw WireError("Unsupported endpoint function: \(name)")
        }
    }

    static func expression(_ node: JSONValue, _ scope: [String: JSONValue]) throws -> JSONValue {
        switch node["kind"]?.text {
        case "reference": return scope[try node["name"]!.string] ?? .null
        case "boolean", "integer": return node["value"] ?? .null
        case "array": return .array(try node["values"]!.array.map { try expression($0, scope) })
        case "object": return .object(try node["values"]!.object.mapValues { try expression($0, scope) })
        case "call": return try function(try node["function"]!.string, try node["arguments"]!.array.map { try expression($0, scope) })
        case "string":
            var text = ""
            for part in try node["parts"]!.array {
                let value = part["kind"]?.text == "literal" ? part["value"] ?? .null
                    : try attribute(scope[try part["name"]!.string] ?? .null, part["path"] ?? .array([]))
                switch value {
                case .string(let item), .number(let item): text += item
                case .bool(let flag): text += flag ? "true" : "false"
                default: throw WireError("Endpoint template references a missing or non-scalar value")
                }
            }
            return .string(text)
        default: throw WireError("Unsupported endpoint expression")
        }
    }

    static func rules(_ nodes: [JSONValue], _ parameters: [String: JSONValue]) throws -> ResolvedEndpoint? {
        rule: for rule in nodes {
            var scope = parameters
            for condition in try rule["conditions"]!.array {
                let value = try function(try condition["function"]!.string, try condition["arguments"]!.array.map { try expression($0, scope) })
                if value == .null || value == .bool(false) { continue rule }
                if let assign = condition["assign"]?.text { scope[assign] = value }
            }
            let result = rule["result"]!
            switch result["kind"]?.text {
            case "tree": if let nested = try rules(try result["rules"]!.array, scope) { return nested }
            case "error": throw WireError(try expression(result["message"]!, scope).string)
            case "endpoint":
                let url = try expression(result["url"]!, scope).string
                guard parseURL(url) != nil else { throw WireError("Endpoint rule returned an invalid HTTP(S) URL") }
                var headers: [String: [String]] = [:]
                for (name, items) in try result["headers"]!.object {
                    headers[name.lowercased()] = try items.array.map { try expression($0, scope).string }
                }
                return ResolvedEndpoint(url: url, headers: headers,
                    properties: try result["properties"]!.object.mapValues { try expression($0, scope) })
            default: throw WireError("Unsupported endpoint rule result")
            }
        }
        return nil
    }

    static func resolve(_ ruleSet: JSONValue, sources: JSONValue?, input: JSONValue,
                        client: [String: JSONValue], builtins: [String: JSONValue]) throws -> ResolvedEndpoint {
        var parameters: [String: JSONValue] = [:]
        for (name, parameter) in try ruleSet["parameters"]!.object {
            var value = JSONValue.null
            for source in (try? sources?[name]?.array) ?? [] {
                switch source["kind"]?.text {
                case "static", "default": value = source["value"] ?? .null
                case "inputMember": value = input[try source["member"]!.string] ?? .null
                case "inputPath": value = try JMESPath.evaluate(source["path"]!["expression"]!, input)
                case "client": value = client[try source["name"]!.string] ?? .null
                case "builtIn": value = builtins[try source["name"]!.string] ?? .null
                default: throw WireError("Unsupported endpoint parameter source")
                }
                if value != .null { break }
            }
            if value == .null && parameter["required"]?.flag == true { throw WireError("Required endpoint parameter \(name) is missing") }
            if value != .null {
                let valid: Bool
                switch (parameter["kind"]?.text, value) {
                case ("string", .string), ("boolean", .bool): valid = true
                case ("stringArray", .array(let items)): valid = items.allSatisfy { $0.text != nil }
                default: valid = false
                }
                guard valid else { throw WireError("Endpoint parameter \(name) has the wrong type") }
            }
            parameters[name] = value
        }
        guard let result = try rules(try ruleSet["rules"]!.array, parameters) else { throw WireError("No endpoint rule matched") }
        return result
    }

    static func hostPrefix(_ url: String, _ description: JSONValue, input: JSONValue) throws -> String {
        var prefix = ""
        for part in try description["parts"]!.array {
            if part["kind"]?.text == "literal" { prefix += try part["value"]!.string; continue }
            let member = try part["member"]!.string
            guard case .string(let value)? = input[member], hostLabel(value, subdomains: false) else {
                throw WireError("Invalid endpoint host label \(member)")
            }
            prefix += value
        }
        guard var components = URLComponents(string: url), let host = components.host else { throw WireError("Invalid endpoint URL") }
        let expanded = prefix + host
        guard expanded.utf8.count <= 253, hostLabel(expanded, subdomains: true) else { throw WireError("Invalid endpoint hostname after prefix expansion") }
        components.host = expanded
        guard let result = components.string else { throw WireError("Invalid endpoint URL") }
        return result
    }
}
