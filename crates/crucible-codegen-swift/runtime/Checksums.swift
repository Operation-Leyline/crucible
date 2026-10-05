import Foundation
import Crypto

/// When the client computes request checksums or validates response checksums.
public enum ChecksumPolicy: String, Sendable {
    case whenSupported = "WHEN_SUPPORTED"
    case whenRequired = "WHEN_REQUIRED"
}

/// A modeled response checksum differs from the received payload.
public struct ChecksumMismatchError: Error, Sendable, CustomStringConvertible {
    public let algorithm: String
    public var description: String { "response \(algorithm) checksum mismatch" }
}

internal struct ChecksumSettings: Sendable {
    var algorithm: String?
    var header: String?
    var response: [String] = []
}

/// Buffered HTTP checksums over the exact bytes sent or received.
internal enum Checksums {
    static let supported = ["CRC32", "CRC32C", "CRC64NVME", "MD5", "SHA1", "SHA256", "SHA512"]
    private static let crc32Table = table32(0xEDB8_8320)
    private static let crc32cTable = table32(0x82F6_3B78)
    private static let crc64Table: [UInt64] = (0..<256).map { byte in
        var value = UInt64(byte)
        for _ in 0..<8 { value = value & 1 == 1 ? (value >> 1) ^ 0x9A6C_9329_AC4B_C9B5 : value >> 1 }
        return value
    }
    private static func table32(_ polynomial: UInt32) -> [UInt32] {
        (0..<256).map { byte in
            var value = UInt32(byte)
            for _ in 0..<8 { value = value & 1 == 1 ? (value >> 1) ^ polynomial : value >> 1 }
            return value
        }
    }
    private static func crc32(_ data: Data, _ table: [UInt32]) -> Data {
        var value: UInt32 = 0xFFFF_FFFF
        for byte in data { value = table[Int((value ^ UInt32(byte)) & 0xFF)] ^ (value >> 8) }
        return withUnsafeBytes(of: (value ^ 0xFFFF_FFFF).bigEndian) { Data($0) }
    }
    private static func crc64(_ data: Data) -> Data {
        var value: UInt64 = .max
        for byte in data { value = crc64Table[Int((value ^ UInt64(byte)) & 0xFF)] ^ (value >> 8) }
        return withUnsafeBytes(of: (value ^ .max).bigEndian) { Data($0) }
    }

    /// The base64 digest that AWS checksum headers carry.
    static func digest(_ algorithm: String, _ data: Data) throws -> String {
        let raw: Data
        switch algorithm {
        case "CRC32": raw = crc32(data, crc32Table)
        case "CRC32C": raw = crc32(data, crc32cTable)
        case "CRC64NVME": raw = crc64(data)
        case "MD5": raw = Data(Insecure.MD5.hash(data: data))
        case "SHA1": raw = Data(Insecure.SHA1.hash(data: data))
        case "SHA256": raw = Data(SHA256.hash(data: data))
        case "SHA512": raw = Data(SHA512.hash(data: data))
        default: throw WireError("Unsupported checksum algorithm: \(algorithm)")
        }
        return raw.base64EncodedString()
    }

    /// Fills unset algorithm and validation members according to policy.
    static func prepare(_ description: JSONValue?, values: inout [String: JSONValue],
                        request: ChecksumPolicy, response: ChecksumPolicy) throws -> ChecksumSettings {
        guard let description, description != .null else { return ChecksumSettings() }
        if description["kind"]?.text == "md5" { return ChecksumSettings(algorithm: "MD5", header: "content-md5") }
        var settings = ChecksumSettings()
        if let member = description["requestAlgorithmMember"]?.text {
            var algorithm = values[member]?.text
            if algorithm == nil && (description["requestRequired"]?.flag == true || request == .whenSupported) {
                algorithm = "CRC32"; values[member] = .string("CRC32")
            }
            if let algorithm {
                guard supported.contains(algorithm) else { throw WireError("Unsupported checksum algorithm: \(algorithm)") }
                settings.algorithm = algorithm; settings.header = "x-amz-checksum-" + algorithm.lowercased()
            }
        } else if description["requestRequired"]?.flag == true {
            settings.algorithm = "MD5"; settings.header = "content-md5"
        }
        if let member = description["validationModeMember"]?.text {
            var mode = values[member]?.text
            if mode == nil && response == .whenSupported { mode = "ENABLED"; values[member] = .string("ENABLED") }
            if mode == "ENABLED" {
                settings.response = ((try? description["responseAlgorithms"]?.array) ?? []).compactMap(\.text).filter(supported.contains)
            }
        }
        return settings
    }

    /// A caller-supplied checksum header is sent unchanged.
    static func apply(_ settings: ChecksumSettings, to request: inout HTTPRequest) throws {
        guard let header = settings.header, let algorithm = settings.algorithm, request.headers[header] == nil else { return }
        request.headers[header] = try digest(algorithm, request.body ?? Data())
    }

    /// Validates the first modeled algorithm the response carries.
    static func validate(_ settings: ChecksumSettings, _ response: HTTPResponse) throws -> String? {
        for algorithm in settings.response {
            guard let expected = response.header("x-amz-checksum-" + algorithm.lowercased()) else { continue }
            let actual = try digest(algorithm, response.body)
            let left = Array(expected.utf8), right = Array(actual.utf8)
            var difference = UInt8(left.count == right.count ? 0 : 1)
            for index in 0..<min(left.count, right.count) { difference |= left[index] ^ right[index] }
            guard difference == 0 else { throw ChecksumMismatchError(algorithm: algorithm) }
            return algorithm
        }
        return nil
    }
}
