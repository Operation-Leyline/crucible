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
