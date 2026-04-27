import Foundation

@Observable
final class QueryViewModel {
    var queryText = ""
    var response: QueryResponse?
    var isLoading = false
    var error: String?

    private let connection: ConnectionManager

    init(connection: ConnectionManager) {
        self.connection = connection
    }

    func runQuery(project: String) async {
        let text = queryText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }

        isLoading = true
        error = nil
        do {
            let request = QueryRequest(
                project: project,
                query: text,
                filters: [:],
                topK: 8,
                minConfidence: nil,
                history: nil
            )
            response = try await connection.api.query(request)
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }
}
