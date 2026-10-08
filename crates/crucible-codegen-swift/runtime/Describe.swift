import Foundation

/// A generated model prints as `TypeName(member=value, ...)`.
///
/// Members the service left unset are omitted, nested models and collections
/// recurse, blobs show only their length, and `@sensitive` members print
/// `*** Sensitive Data Redacted ***` instead of their value. Conformers also
/// answer `debugDescription` and reflection (`dump`, `String(reflecting:)`)
/// with that same text, so no inspection path reveals a sensitive value.
public protocol ModelDescription: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {}

extension ModelDescription {
    public var debugDescription: String { description }
    public var customMirror: Mirror { Mirror(self, children: ["description": description]) }
}

/// Formatting shared by the description of every generated model.
internal enum Describe {
    static let redacted = "*** Sensitive Data Redacted ***"

    /// Replaces a present sensitive value; an absent one stays absent so it is omitted.
    static func redact(_ value: Any?) -> Any? {
        guard let value, render(value) != nil else { return nil }
        return redacted
    }

    static func format(_ type: String, _ members: [(String, Any?)]) -> String {
        var parts: [String] = []
        for (name, value) in members {
            if let value, let text = render(value) { parts.append("\(name)=\(text)") }
        }
        return "\(type)(\(parts.joined(separator: ", ")))"
    }

    /// Nil for an absent optional, so callers can omit it.
    static func render(_ value: Any) -> String? {
        if let data = value as? Data { return "<\(data.count) bytes>" }
        let mirror = Mirror(reflecting: value)
        switch mirror.displayStyle {
        case .optional:
            guard let child = mirror.children.first else { return nil }
            return render(child.value)
        case .collection:
            return "[" + mirror.children.map { render($0.value) ?? "null" }.joined(separator: ", ") + "]"
        case .set:
            // Hash order varies between runs; sort so output is stable.
            return "[" + mirror.children.map { render($0.value) ?? "null" }.sorted().joined(separator: ", ") + "]"
        case .dictionary:
            let entries = mirror.children.map { child -> (String, String) in
                let pair = Array(Mirror(reflecting: child.value).children)
                return (render(pair[0].value) ?? "null", render(pair[1].value) ?? "null")
            }.sorted { $0.0 < $1.0 }
            return "{" + entries.map { "\($0.0)=\($0.1)" }.joined(separator: ", ") + "}"
        default:
            return "\(value)"
        }
    }
}
