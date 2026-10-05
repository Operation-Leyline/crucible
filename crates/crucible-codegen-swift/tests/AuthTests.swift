import Foundation
import XCTest
@testable import Auth

private actor Counter {
    private(set) var value = 0
    func increment() { value += 1 }
}

private actor Gate {
    private var waiting: [CheckedContinuation<Void, Never>] = []
    private var open = false
    private(set) var entered = false
    func wait() async {
        entered = true
        if open { return }
        await withCheckedContinuation { waiting.append($0) }
    }
    func release() { open = true; waiting.forEach { $0.resume() }; waiting = [] }
}

final class AuthTests: XCTestCase, @unchecked Sendable {
    func testPublishedSigningVector() throws {
        // AWS SigV4 documentation: GET Object, examplebucket, 2013-05-24.
        var request = HTTPRequest(url: URL(string: "https://examplebucket.s3.amazonaws.com/test.txt")!, method: "GET",
                                  headers: ["Range": "bytes=0-9"])
        let identity = try AWSCredentials(accessKeyID: "AKIAIOSFODNN7EXAMPLE", secretAccessKey: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY")
        try SigV4Signer.sign(&request, credentials: identity, options: SigningOptions(region: "us-east-1", service: "s3",
            date: Date(timeIntervalSince1970: 1_369_353_600)))
        XCTAssertEqual(request.headers["authorization"], "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, "
            + "SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41")
        XCTAssertEqual(request.headers["x-amz-content-sha256"], "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        var dumped = ""
        dump(identity, to: &dumped)
        for text in [String(describing: identity), String(reflecting: identity), "\(identity)", dumped] {
            XCTAssertFalse(text.contains(identity.secretAccessKey), text)
        }
        let basic = try BasicCredentials(username: "user", password: "hunter2")
        XCTAssertFalse(String(describing: basic).contains("hunter2"))
        XCTAssertThrowsError(try BasicCredentials(username: "a:b", password: "x"))
        XCTAssertThrowsError(try SigV4Signer.sign(&request, credentials: AWSCredentials(accessKeyID: "a", secretAccessKey: "b",
            expiration: Date(timeIntervalSinceNow: -1)), options: SigningOptions(region: "r", service: "s")))
        XCTAssertThrowsError(try SigV4Signer.sign(&request, credentials: identity, options: SigningOptions(region: "a/b", service: "s")))
    }

    func testPreconfiguredCredentialsBypassProviders() async throws {
        let fixture = try ScriptedFixture()
        try await fixture.reset()
        let basic = try AuthClient(ClientConfig(endpoint: fixture.url, headers: ["Authorization": "Basic supplied"],
            basicAuth: { XCTFail("preconfigured identity invoked provider"); return nil }))
        _ = try await basic.read()
        let key = try AuthClient(ClientConfig(endpoint: fixture.url, headers: ["X-Api-Key": "ApiKey supplied"],
            apiKey: { XCTFail("preconfigured key invoked provider"); return nil }))
        _ = try await key.read()
        let requests = try await fixture.requests()
        XCTAssertEqual(requests.map { $0.headers["authorization"] }, ["Basic supplied", nil])
        XCTAssertEqual(requests.last?.headers["x-api-key"], "ApiKey supplied")
        let unrelated = try AuthClient(ClientConfig(endpoint: fixture.url, headers: ["Authorization": "Digest unrelated"]))
        do { _ = try await unrelated.read(); XCTFail("unrelated authorization accepted") } catch is WireError {}
        let dispatched = try await fixture.requests()
        XCTAssertTrue(dispatched.isEmpty)
    }

    func testModeledAuthenticationOverHTTP() async throws {
        let fixture = try ScriptedFixture()
        try await fixture.reset()
        let calls = Counter()
        let basic = try AuthClient(ClientConfig(endpoint: fixture.url, basicAuth: {
            await calls.increment(); return try BasicCredentials(username: "user", password: "password")
        }))
        _ = try await basic.`public`()
        var count = await calls.value
        XCTAssertEqual(count, 0)
        _ = try await basic.read()
        count = await calls.value
        XCTAssertEqual(count, 1)
        var requests = try await fixture.requests()
        XCTAssertNil(requests[0].headers["authorization"])
        XCTAssertEqual(requests[1].headers["authorization"], "Basic dXNlcjpwYXNzd29yZA==")

        let key = try AuthClient(ClientConfig(endpoint: fixture.url, apiKey: { "secret" }))
        _ = try await key.read()
        requests = try await fixture.requests()
        XCTAssertEqual(requests[0].headers["x-api-key"], "ApiKey secret")

        let signing = try AuthClient(ClientConfig(endpoint: fixture.url,
            middleware: [{ request in var request = request; request.headers["X-Hook"] = "final"; return request }],
            credentials: ClosureCredentialsProvider { try AWSCredentials(accessKeyID: "id", secretAccessKey: "secret", sessionToken: "session") },
            region: "us-east-1"))
        _ = try await signing.read()
        requests = try await fixture.requests()
        let authorization = try XCTUnwrap(requests[0].headers["authorization"])
        XCTAssertTrue(authorization.hasPrefix("AWS4-HMAC-SHA256 Credential=id/"), authorization)
        XCTAssertTrue(authorization.contains("/us-east-1/example/aws4_request"), authorization)
        XCTAssertTrue(authorization.contains(";x-hook"), authorization)
        XCTAssertEqual(requests[0].headers["x-amz-security-token"], "session")
        XCTAssertEqual(requests[0].sigv4, true, "the fixture's independent signer rejected the signature")

        let missing = try AuthClient(ClientConfig(endpoint: fixture.url, region: "us-east-1"))
        do { _ = try await missing.read(); XCTFail("required identity missing") } catch is WireError {}
        _ = try await missing.optional()
        requests = try await fixture.requests()
        XCTAssertEqual(requests.count, 1)
        XCTAssertNil(requests[0].headers["authorization"])

        let plaintext = try AuthClient(ClientConfig(endpoint: "http://example.test", apiKey: { "secret" }))
        do { _ = try await plaintext.read(); XCTFail("credentials sent over plaintext") }
        catch let error as WireError { XCTAssertTrue(error.description.contains("plaintext"), error.description) }
    }

    func testCredentialProvidersAndRefresh() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("credentials")
        try Data("[other]\naws_access_key_id = wrong\naws_secret_access_key = wrong\n[test]\naws_access_key_id = id\naws_secret_access_key = s%cret\naws_session_token = token\n".utf8).write(to: source)
        let chain = CredentialChain(ClosureCredentialsProvider { nil }, ProfileCredentialsProvider(profile: "test", path: source.path))
        let value = try await chain.credentials()
        XCTAssertEqual(value?.secretAccessKey, "s%cret")
        XCTAssertEqual(value?.sessionToken, "token")
        let absent = try await ProfileCredentialsProvider(profile: "test", path: directory.appendingPathComponent("missing").path).credentials()
        XCTAssertNil(absent)

        let calls = Counter()
        let cached = try CachedCredentials(ClosureCredentialsProvider {
            await calls.increment()
            try await Task.sleep(nanoseconds: 20_000_000)
            return try AWSCredentials(accessKeyID: "id", secretAccessKey: "secret", expiration: Date(timeIntervalSinceNow: 20))
        }, refreshBefore: 60)
        try await withThrowingTaskGroup(of: AWSCredentials?.self) { group in
            for _ in 0..<30 { group.addTask { try await cached.credentials() } }
            for try await identity in group { XCTAssertEqual(identity?.accessKeyID, "id") }
        }
        var count = await calls.value
        XCTAssertEqual(count, 1, "one refresh in flight serves every caller")
        _ = try await cached.credentials()
        count = await calls.value
        XCTAssertEqual(count, 1, "short-lived credentials are reused for half their lifetime")

        let gate = Gate()
        let blocked = try CachedCredentials(ClosureCredentialsProvider { await gate.wait(); return nil })
        let first = Task { try await blocked.credentials() }
        while !(await gate.entered) { try await Task.sleep(nanoseconds: 1_000_000) }
        let second = Task { try await blocked.credentials() }
        try await Task.sleep(nanoseconds: 10_000_000)
        second.cancel()
        do { _ = try await second.value; XCTFail("cancelled waiter returned") } catch is CancellationError {}
        await gate.release()
        let released = try await first.value
        XCTAssertNil(released)

        let expired = try CachedCredentials(ClosureCredentialsProvider {
            try AWSCredentials(accessKeyID: "id", secretAccessKey: "secret", expiration: Date(timeIntervalSinceNow: -1))
        }, refreshBefore: 0)
        do { _ = try await expired.credentials(); XCTFail("expired identity accepted") } catch is WireError {}
    }
}
