import Foundation
import Docs

struct Vector: Decodable {
    let name: String
    let docUrl: String?
    let typed: Bool
}

@main struct DocsConsumer {
    static func main() async throws {
        let cases = try JSONDecoder().decode([Vector].self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2])))
        for (index, vector) in cases.enumerated() {
            let client = try TroubleshootingClient(ClientConfig(endpoint: CommandLine.arguments[1], maxAttempts: 1, headers: ["x-case": String(index)]))
            var retained: (any ServiceError)?
            do {
                _ = try await client.inspect()
                preconditionFailure("Expected failure for \(vector.name)")
            } catch let error as any ServiceError {
                retained = error
                precondition(error.docURL == vector.docUrl, "\(vector.name): \(error.docURL ?? "nil") != \(vector.docUrl ?? "nil")")
                precondition(!(error is UnknownServiceError) == vector.typed, "Wrong typed error for \(vector.name)")
                if ["document-empty", "document-whitespace", "document-null"].contains(vector.name) {
                    let payload = (error as! DocumentPayloadError).dataPresence
                    precondition(payload.isSet == (vector.name == "document-null"), "\(vector.name): document presence")
                    if vector.name == "document-null" {
                        guard case .null = payload else { preconditionFailure("Explicit document null") }
                    }
                }
            }
            client.close()
            precondition(retained?.docURL == vector.docUrl, "URL lifetime")
        }
        let original = Denied(message: "denied", detail: .access)
        var copy = original
        copy.detail = .empty
        precondition(original.docURL == "https://docs.example.test/errors/Denied/reason-access%2Fneeded")
        precondition(copy.docURL == "https://docs.example.test/errors/Denied/reason-")
    }
}
