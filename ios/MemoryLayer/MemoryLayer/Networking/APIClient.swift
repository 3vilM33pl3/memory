import Foundation

enum APIError: Error, LocalizedError {
    case unauthorized
    case notFound
    case serverError(Int, String)
    case networkUnavailable
    case decodingFailed(Error)
    case invalidURL

    var errorDescription: String? {
        switch self {
        case .unauthorized: return "Unauthorized. Check your API token."
        case .notFound: return "Resource not found."
        case .serverError(let code, let msg): return "Server error \(code): \(msg)"
        case .networkUnavailable: return "Network unavailable."
        case .decodingFailed(let err): return "Decoding failed: \(err.localizedDescription)"
        case .invalidURL: return "Invalid URL."
        }
    }
}

actor APIClient {
    private let session: URLSession
    private var baseURL: String
    private var token: String?
    private let decoder: JSONDecoder

    init(baseURL: String = "", token: String? = nil) {
        self.baseURL = baseURL
        self.token = token
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = AppConstants.requestTimeout
        self.session = URLSession(configuration: config)
        self.decoder = JSONDecoder()
        self.decoder.keyDecodingStrategy = .convertFromSnakeCase
    }

    func configure(baseURL: String, token: String?) {
        self.baseURL = baseURL
        self.token = token
    }

    // MARK: - Generic request

    private func request<T: Decodable>(
        method: String = "GET",
        path: String,
        body: (any Encodable)? = nil
    ) async throws -> T {
        guard let url = URL(string: baseURL + path) else {
            throw APIError.invalidURL
        }
        var req = URLRequest(url: url)
        req.httpMethod = method
        if let token {
            req.setValue(token, forHTTPHeaderField: "x-api-token")
        }
        if let body {
            req.setValue("application/json", forHTTPHeaderField: "Content-Type")
            let encoder = JSONEncoder()
            encoder.keyEncodingStrategy = .convertToSnakeCase
            req.httpBody = try encoder.encode(AnyEncodable(body))
        }

        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await session.data(for: req)
        } catch {
            throw APIError.networkUnavailable
        }

        guard let http = response as? HTTPURLResponse else {
            throw APIError.networkUnavailable
        }

        switch http.statusCode {
        case 200..<300:
            break
        case 401:
            throw APIError.unauthorized
        case 404:
            throw APIError.notFound
        default:
            let body = String(data: data, encoding: .utf8) ?? ""
            throw APIError.serverError(http.statusCode, body)
        }

        do {
            return try decoder.decode(T.self, from: data)
        } catch {
            throw APIError.decodingFailed(error)
        }
    }

    // MARK: - Health

    func healthCheck() async throws -> [String: AnyCodable] {
        try await request(path: "/healthz")
    }

    // MARK: - Project

    func getOverview(project: String) async throws -> ProjectOverviewResponse {
        try await request(path: "/v1/projects/\(project.urlEncoded)/overview")
    }

    func getMemories(project: String, status: MemoryStatus? = nil, page: Int? = nil, perPage: Int? = nil) async throws -> ProjectMemoriesResponse {
        var params: [String] = []
        if let status { params.append("status=\(status.rawValue)") }
        if let page { params.append("page=\(page)") }
        if let perPage { params.append("per_page=\(perPage)") }
        let query = params.isEmpty ? "" : "?\(params.joined(separator: "&"))"
        return try await request(path: "/v1/projects/\(project.urlEncoded)/memories\(query)")
    }

    func getActivities(project: String) async throws -> [ActivityEvent] {
        try await request(path: "/v1/projects/\(project.urlEncoded)/activities")
    }

    func getReplacementProposals(project: String) async throws -> ReplacementProposalListResponse {
        try await request(path: "/v1/projects/\(project.urlEncoded)/replacement-proposals")
    }

    func approveProposal(project: String, id: String) async throws -> ReplacementProposalResolutionResponse {
        try await request(method: "POST", path: "/v1/projects/\(project.urlEncoded)/replacement-proposals/\(id.urlEncoded)/approve")
    }

    func rejectProposal(project: String, id: String) async throws -> ReplacementProposalResolutionResponse {
        try await request(method: "POST", path: "/v1/projects/\(project.urlEncoded)/replacement-proposals/\(id.urlEncoded)/reject")
    }

    // MARK: - Memory

    func getMemory(id: String) async throws -> MemoryEntryResponse {
        try await request(path: "/v1/memory/\(id.urlEncoded)")
    }

    func getMemoryHistory(id: String) async throws -> MemoryHistoryResponse {
        try await request(path: "/v1/memory/\(id.urlEncoded)/history")
    }

    // MARK: - Query

    func query(_ queryRequest: QueryRequest) async throws -> QueryResponse {
        try await request(method: "POST", path: "/v1/query", body: queryRequest)
    }

    // MARK: - Resume

    func resume(project: String) async throws -> ResumeResponse {
        struct ResumeRequest: Encodable {
            let project: String
            let repoRoot: String? = nil
            let includeLlmSummary: Bool = true
            let limit: Int = 20
        }
        return try await request(method: "POST", path: "/v1/projects/\(project.urlEncoded)/resume", body: ResumeRequest(project: project))
    }

    // MARK: - Agents

    func getAgents() async throws -> AgentSnapshotResponse {
        try await request(path: "/v1/agents")
    }
}

// MARK: - Helpers

private extension String {
    var urlEncoded: String {
        addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? self
    }
}

private struct AnyEncodable: Encodable {
    private let _encode: (Encoder) throws -> Void

    init(_ wrapped: any Encodable) {
        _encode = { encoder in
            try wrapped.encode(to: encoder)
        }
    }

    func encode(to encoder: Encoder) throws {
        try _encode(encoder)
    }
}
