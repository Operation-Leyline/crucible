import Foundation
import XCTest
@testable import Types

final class ExtendedTests: XCTestCase {
    private func codec() throws -> WireCodec {
        try ClientRuntime(ClientConfig(endpoint: "http://127.0.0.1"), endpoint: "").codec
    }
    func testExactDefaultsAndSparseCollections() throws {
        let codec = try codec()
        let node = try JSONValue(data: Data("{\"mode\":123,\"names\":[\"a\",\"b\"],\"sparseNames\":[null,\"x\"],\"sparseMap\":{\"absent\":null,\"future\":123}}".utf8))
        let output = try codec.decode("example.types#Output", node, as: Output.self)
        XCTAssertEqual(output.integer.description, "12345678901234567890123456789012345678901234567890")
        XCTAssertEqual(output.decimal.description, "12345678901234567890.12345678901234567890123456789")
        XCTAssertEqual(output.mode?.rawValue, 123)
        XCTAssertEqual(output.sparseNames!, [nil, "x"])
        XCTAssertTrue(output.sparseMap!.keys.contains("absent"))
        XCTAssertNil(output.sparseMap!["absent"]!)
        XCTAssertEqual(output.sparseMap!["future"]!!.rawValue, 123)
        let encoded = try codec.encode("example.types#Output", output)
        XCTAssertEqual(encoded["sparseNames"], node["sparseNames"])
        XCTAssertEqual(encoded["sparseMap"], node["sparseMap"])
        XCTAssertEqual(encoded["integer"], .number(output.integer.description))
        XCTAssertEqual(Mode.off.rawValue, -1)
    }
    func testSetsDenseNullAndEnumBounds() throws {
        let codec = try codec()
        XCTAssertThrowsError(try codec.decode("example.types#Names", .array([.string("a"), .string("a")]), as: Names.self))
        XCTAssertThrowsError(try codec.encode("example.types#Names", ["a", "a"]))
        XCTAssertThrowsError(try codec.decode("example.types#Names", .array([.null]), as: Names.self))
        XCTAssertThrowsError(try codec.decode("example.types#Mode", .number("2147483648"), as: Mode.self))
        XCTAssertThrowsError(try codec.collection([.number("10"), .number("1e1")], shape: .object(["set": .bool(true)])))
        _ = try codec.decode("example.types#Integers", .array([.number("18446744073709551616"), .number("18446744073709551617")]), as: Integers.self)
        _ = try codec.decode("example.types#Decimals", .array([.number("1.00000000000000000001"), .number("1.00000000000000000002")]), as: Decimals.self)
        XCTAssertThrowsError(try codec.decode("example.types#Integers", .array([.number("1"), .number("1.0")]), as: Integers.self))
        XCTAssertThrowsError(try codec.decode("example.types#Decimals", .array([.number("1"), .number("1.0")]), as: Decimals.self))
    }
    func testNumbersNeverRound() throws {
        XCTAssertEqual(try BigInteger("1e50"), try BigInteger("100000000000000000000000000000000000000000000000000"))
        XCTAssertEqual(try BigInteger("100e-2"), try BigInteger("1"))
        XCTAssertThrowsError(try BigInteger("1.0000000000000000000001"))
        XCTAssertEqual(try BigDecimal("-0e-999999999999999999999999"), try BigDecimal("0"))
        XCTAssertEqual(try BigDecimal("1e999999999999999999999999"), try BigDecimal("10e999999999999999999999998"))
        XCTAssertNotEqual(try BigDecimal("1.0000000000000000000001"), try BigDecimal("1"))
        XCTAssertEqual(try BigDecimal("1e-999999999999999999999999"), try BigDecimal("10e-1000000000000000000000000"))
    }
}
