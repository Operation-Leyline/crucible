import Foundation

extension ClientRuntime {
    /// Attaches the bearer token to an operation modeled with bearer
    /// authentication. A caller-supplied `Authorization: Bearer ...` header
    /// bypasses the token source. Returns whether credentials are on the request.
    func authenticate(_ request: inout HTTPRequest, operation op: JSONValue) async throws -> Bool {
        guard op["bearer"]?.flag == true else { return false }
        let existing = Dictionary(request.headers.map { ($0.key.lowercased(), $0.value) }, uniquingKeysWith: { _, last in last })["authorization"]
        if let existing, existing.count > "Bearer ".count, existing.lowercased().hasPrefix("bearer ") { return true }
        if let token = try await config.token?(), !token.isEmpty {
            request.headers["authorization"] = "Bearer \(token)"
            return true
        }
        return false
    }
}
