import Foundation
import XCTest
@testable import Weather

final class SerializationTests: XCTestCase {
    private func equivalent(_ actual: JSONValue, _ expected: JSONValue) throws -> Bool {
        switch (actual, expected) {
        case (.number(let left), .number(let right)):
            let a = try WireNumber(left).components(fractionDigits: 9)
            let b = try WireNumber(right).components(fractionDigits: 9)
            return a.0 == b.0 && a.1 == b.1
        case (.object(let left), .object(let right)):
            guard left.count == right.count else { return false }
            for (key, value) in right {
                guard let item = left[key], try equivalent(item, value) else { return false }
            }
            return true
        case (.array(let left), .array(let right)):
            guard left.count == right.count else { return false }
            for (a, b) in zip(left, right) { if try !equivalent(a, b) { return false } }
            return true
        default: return actual == expected
        }
    }
    private func state<T>(_ value: OptionalValue<T>) -> String {
        switch value { case .unset: return "absent"; case .null: return "null"; case .value: return "value" }
    }
    func testSharedSerialization() throws {
        let codec = try ClientRuntime(ClientConfig(endpoint: "http://127.0.0.1"), endpoint: "").codec
        let path = ProcessInfo.processInfo.environment["SERIALIZATION_CASES"]!
        let rows = try JSONValue(data: Data(contentsOf: URL(fileURLWithPath: path))).array
        for row in rows {
            let id = try row["id"]!.string
            let target = try row["target"]!.string
            let wire = try JSONValue(data: Data(row["wire"]!.string.utf8))
            var states: [String: String] = [:]
            var blobHex: String?
            var city: CitySummary?
            func roundTrip<T: WireCodable>(_ type: T.Type) throws -> JSONValue {
                try codec.encode(target, codec.decode(target, wire, as: type))
            }
            let encoded: JSONValue
            do {
                switch target {
                case "example.weather#CitySummary":
                    let value = try codec.decode(target, wire, as: CitySummary.self)
                    city = value
                    states = ["population": "value", "updatedAt": state(value.updatedAtPresence), "createdAt": state(value.createdAtPresence)]
                    encoded = try codec.encode(target, value)
                case "example.weather#Photo":
                    let value = try codec.decode(target, wire, as: Data.self)
                    blobHex = value.map { String(format: "%02x", $0) }.joined()
                    encoded = try codec.encode(target, value)
                case "example.weather#Sky": encoded = try roundTrip(Sky.self)
                case "example.weather#Location": encoded = try roundTrip(Location.self)
                case "example.weather#Forecast": encoded = try roundTrip(Forecast.self)
                default: throw WireError("Unmapped corpus target: \(target)")
                }
            } catch is WireError {
                XCTAssertEqual(row["reject"], .bool(true), id)
                continue
            }
            XCTAssertNotEqual(row["reject"], .bool(true), id)
            guard let expected = row["expect"] else { continue }
            let wanted = try JSONValue(data: Data(expected["json"]!.string.utf8))
            XCTAssertTrue(try equivalent(JSONValue(data: encoded.data()), wanted), id)
            if let fields = expected["states"] {
                for (member, value) in try fields.object { XCTAssertEqual(states[member], value.text, id + ": " + member) }
            }
            if let hex = expected["blobHex"] { XCTAssertEqual(blobHex, hex.text, id) }
            if let instant = expected["instant"] {
                let value = try XCTUnwrap(city)
                let stamp = try XCTUnwrap(instant["member"]?.text == "updatedAt" ? value.updatedAt : value.createdAt)
                XCTAssertEqual(String(stamp.seconds), instant["seconds"]?.text, id)
                XCTAssertEqual(stamp.nanoseconds, try WireNumber(instant["nanoseconds"]!.number).integer(UInt32.self), id)
            }
        }
    }
}
