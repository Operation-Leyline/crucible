import Foundation

/// Whole epoch seconds and nanoseconds; Date conversion has Date's floating-point precision.
public struct Timestamp: Sendable, Equatable, CustomStringConvertible {
    /// Whole seconds since the Unix epoch.
    public let seconds: Int64
    /// The fraction of a second, below one billion.
    public let nanoseconds: UInt32
    /// RFC 3339 in UTC, with the fraction only when it is not zero.
    public var description: String {
        // RFC 3339 only covers years 1-9999; other instants show as raw epoch text.
        guard (-62_135_596_800...253_402_300_799).contains(seconds) else {
            return nanoseconds == 0 ? "\(seconds)" : "\(seconds).\(Self.fractionDigits(nanoseconds))"
        }
        // Civil-from-days arithmetic instead of a shared formatter: no allocation per call, no shared mutable state.
        let days = seconds >= 0 ? seconds / 86_400 : -((-seconds + 86_399) / 86_400)
        let remainder = Int(seconds - days * 86_400)
        let shifted = days + 719_468
        let era = (shifted >= 0 ? shifted : shifted - 146_096) / 146_097
        let dayOfEra = shifted - era * 146_097
        let yearOfEra = (dayOfEra - dayOfEra / 1_460 + dayOfEra / 36_524 - dayOfEra / 146_096) / 365
        let dayOfYear = dayOfEra - (365 * yearOfEra + yearOfEra / 4 - yearOfEra / 100)
        let monthIndex = (5 * dayOfYear + 2) / 153
        let day = dayOfYear - (153 * monthIndex + 2) / 5 + 1
        let month = monthIndex < 10 ? monthIndex + 3 : monthIndex - 9
        let year = yearOfEra + era * 400 + (month <= 2 ? 1 : 0)
        func pad(_ value: Int64, _ width: Int) -> String {
            let digits = String(value)
            return String(repeating: "0", count: max(0, width - digits.count)) + digits
        }
        var text = "\(pad(year, 4))-\(pad(month, 2))-\(pad(day, 2))T\(pad(Int64(remainder / 3_600), 2)):\(pad(Int64(remainder % 3_600 / 60), 2)):\(pad(Int64(remainder % 60), 2))"
        if nanoseconds != 0 { text += "." + Self.fractionDigits(nanoseconds) }
        return text + "Z"
    }
    private static func fractionDigits(_ nanoseconds: UInt32) -> String {
        var fraction = String(nanoseconds)
        fraction = String(repeating: "0", count: 9 - fraction.count) + fraction
        while fraction.hasSuffix("0") { fraction.removeLast() }
        return fraction
    }
    /// Throws ``WireError`` when `nanoseconds` is a second or more.
    public init(seconds: Int64, nanoseconds: UInt32 = 0) throws {
        guard nanoseconds < 1_000_000_000 else { throw WireError("Nanoseconds must be below one second") }
        self.seconds = seconds; self.nanoseconds = nanoseconds
    }
    /// Throws ``WireError`` when `date` is not finite or outside the timestamp range.
    public init(date: Date) throws {
        let value = date.timeIntervalSince1970
        guard value.isFinite, let seconds = Int64(exactly: floor(value)) else { throw WireError("Date outside timestamp range") }
        try self.init(seconds: seconds, nanoseconds: UInt32((value - floor(value)) * 1_000_000_000))
    }
    /// The instant as a `Date`, with its floating-point precision.
    public var date: Date { Date(timeIntervalSince1970: Double(seconds) + Double(nanoseconds) / 1_000_000_000) }
    private static func httpFormatter() -> DateFormatter {
        let formatter = DateFormatter(); formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(secondsFromGMT: 0); formatter.dateFormat = "EEE, dd MMM yyyy HH:mm:ss 'GMT'"
        formatter.isLenient = false
        return formatter
    }
    private var fraction: String {
        let digits = String(nanoseconds)
        return (String(repeating: "0", count: 9 - digits.count) + digits).replacingOccurrences(of: "0+$", with: "", options: .regularExpression)
    }
}
extension Timestamp: WireCodable {
    internal static func fromWire(_ value: JSONValue, _ context: WireContext) throws -> Timestamp {
        if context.format == "epoch-seconds" {
            let number = try WireNumber(value.number)
            let (whole, fraction) = try number.components(fractionDigits: 9)
            guard let seconds = Int64(whole), let nanos = UInt32(fraction) else { throw WireError("Timestamp out of range") }
            if number.negative && nanos > 0 {
                let (normalized, overflow) = seconds.subtractingReportingOverflow(1)
                guard !overflow else { throw WireError("Timestamp out of range") }
                return try Timestamp(seconds: normalized, nanoseconds: 1_000_000_000 - nanos)
            }
            return try Timestamp(seconds: seconds, nanoseconds: nanos)
        }
        let text = try value.string
        if context.format == "http-date" {
            guard let date = httpFormatter().date(from: text) else { throw WireError("Invalid HTTP timestamp") }
            return try Timestamp(date: date)
        }
        var base = text; var nanos: UInt32 = 0
        if let dot = text.firstIndex(of: ".") {
            let suffix = text[text.index(after: dot)...]
            let fraction = suffix.prefix(while: { $0.isASCII && $0.isNumber })
            guard !fraction.isEmpty, fraction.dropFirst(9).allSatisfy({ $0 == "0" }) else { throw WireError("Timestamp exceeds nanosecond precision") }
            let digits = String(fraction.prefix(9))
            nanos = UInt32(digits + String(repeating: "0", count: 9 - digits.count))!
            base = String(text[..<dot]) + String(suffix.dropFirst(fraction.count))
        }
        guard let date = ISO8601DateFormatter().date(from: base) else { throw WireError("Invalid ISO timestamp") }
        let whole = try Timestamp(date: date)
        return try Timestamp(seconds: whole.seconds, nanoseconds: nanos)
    }
    internal func toWire(_ context: WireContext) throws -> JSONValue {
        switch context.format {
        case "epoch-seconds":
            if nanoseconds == 0 { return .number(String(seconds)) }
            if seconds < 0 {
                let digits = String(1_000_000_000 - nanoseconds)
                let fraction = (String(repeating: "0", count: 9 - digits.count) + digits).replacingOccurrences(of: "0+$", with: "", options: .regularExpression)
                return .number((seconds == -1 ? "-0" : String(seconds + 1)) + "." + fraction)
            }
            return .number(String(seconds) + "." + fraction)
        case "http-date": return .string(Self.httpFormatter().string(from: Date(timeIntervalSince1970: Double(seconds))))
        default:
            let base = ISO8601DateFormatter().string(from: Date(timeIntervalSince1970: Double(seconds)))
            guard base.hasSuffix("Z") else { throw WireError("Timestamp outside ISO date range") }
            return .string(nanoseconds == 0 ? base : String(base.dropLast()) + "." + fraction + "Z")
        }
    }
}
