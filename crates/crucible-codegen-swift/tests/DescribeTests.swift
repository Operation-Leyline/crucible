// Run against a package generated from crates/crucible-smithy/tests/fixtures/client-docs.json.
import Foundation
import XCTest
@testable import Vault

final class DescribeTests: XCTestCase {
    private let marker = "*** Sensitive Data Redacted ***"
    private let secrets = ["hunter2", "4321", "old-one", "old-two", "map-secret", "union-secret"]

    private func secret() throws -> Secret {
        Secret(name: "db", password: "hunter2", pin: "4321", history: ["old-one", "old-two"],
               byName: ["k": "map-secret"], aliases: ["alias-a", "alias-b"], labels: ["env": "prod"],
               kind: .login, level: .high, credential: .password("union-secret"),
               createdAt: try Timestamp(seconds: 0), payload: Data([1, 2, 3]))
    }

    func testPrintsMembersAwsStyleAndRecursesIntoCollections() throws {
        let text = try secret().description
        XCTAssertTrue(text.hasPrefix("Secret(name=db, "), text)
        XCTAssertTrue(text.contains("aliases=[alias-a, alias-b]"), text)
        XCTAssertTrue(text.contains("labels={env=prod}"), text)
        XCTAssertTrue(text.contains("kind=Kind(login)"), text)
        XCTAssertTrue(text.contains("level=Level(2)"), text)
        XCTAssertTrue(text.contains("createdAt=1970-01-01T00:00:00Z"), text)
        XCTAssertTrue(text.contains("payload=<3 bytes>"), text)
        XCTAssertFalse(text.contains("revision"), "absent members are omitted")
    }

    func testTimestampsOutsideRfc3339YearsPrintAsEpochSeconds() throws {
        XCTAssertEqual(try Timestamp(seconds: 253_402_300_800).description, "253402300800")
        XCTAssertEqual(try Timestamp(seconds: -62_135_596_801, nanoseconds: 500_000_000).description, "-62135596801.5")
        XCTAssertEqual(try Timestamp(seconds: 253_402_300_799).description, "9999-12-31T23:59:59Z")
        XCTAssertEqual(try Timestamp(seconds: 0, nanoseconds: 1_000).description, "1970-01-01T00:00:00.000001Z")
        XCTAssertEqual(try Timestamp(seconds: -62_135_596_800).description, "0001-01-01T00:00:00Z")
        XCTAssertEqual(try Timestamp(seconds: 951_782_400).description, "2000-02-29T00:00:00Z")
        XCTAssertEqual(try Timestamp(seconds: -1).description, "1969-12-31T23:59:59Z")
    }

    func testSensitiveMembersNeverPrintTheirValue() throws {
        let value = try secret()
        for text in [value.description, value.debugDescription, String(reflecting: value), "\(value)"] {
            for leaked in secrets { XCTAssertFalse(text.contains(leaked), "\(leaked) in \(text)") }
            XCTAssertTrue(text.contains("password=\(marker)"), text)
            XCTAssertTrue(text.contains("pin=\(marker)"), text)
            XCTAssertTrue(text.contains("history=\(marker)"), text)
            XCTAssertTrue(text.contains("byName=\(marker)"), text)
            XCTAssertTrue(text.contains("credential=Credential(password=\(marker))"), text)
        }
        var dumped = ""
        dump(value, to: &dumped)
        for leaked in secrets { XCTAssertFalse(dumped.contains(leaked), "\(leaked) in dump") }
    }

    func testErrorMetadataDumpsHideMessageAndHeaders() {
        let metadata = ErrorMetadata(name: "AccessDenied", message: "msg-secret", status: 403,
                                     headers: ["authorization": ["header-secret"]])
        // An error's own description is its message by design, so the unknown error is held to the headers only.
        var dumped = ""
        dump(UnknownServiceError(metadata: metadata), to: &dumped)
        XCTAssertFalse(dumped.contains("header-secret"), dumped)
        var metadataDump = ""
        dump(metadata, to: &metadataDump)
        for text in [metadataDump, String(reflecting: metadata), metadata.debugDescription] {
            XCTAssertFalse(text.contains("header-secret") || text.contains("msg-secret"), text)
        }
    }

    func testUnionsAndAbsentMembers() {
        XCTAssertEqual(Credential.pin("9999").description, "Credential(pin=\(marker))")
        XCTAssertEqual(Credential.username("bob").description, "Credential(username=bob)")
        XCTAssertEqual(Credential.unknownVariant(name: "future", value: .string("token-123")).description,
                       "Credential(unknownVariant=future)")
        XCTAssertEqual(Secret(name: "x").description, "Secret(name=x)")
    }

    func testErrorsPrintMembersAndTransportMetadataButNotHeaders() {
        let error = AccessDenied(message: "denied", reason: "secret-reason",
                                 metadata: ErrorMetadata(name: "AccessDenied", message: "denied", status: 403,
                                                         requestID: "r-1", headers: ["authorization": ["x"]]))
        let text = error.description
        XCTAssertTrue(text.contains("message=denied"), text)
        XCTAssertTrue(text.contains("reason=\(marker)"), text)
        XCTAssertFalse(text.contains("secret-reason"), text)
        XCTAssertTrue(text.contains("metadata=ErrorMetadata(name=AccessDenied, status=403, requestID=r-1)"), text)
        XCTAssertFalse(text.contains("authorization"), text)
    }
}
