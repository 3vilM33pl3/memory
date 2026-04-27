import Foundation

struct ReplacementProposalRecord: Codable, Identifiable {
    let id: String
    let project: String
    let targetMemoryId: String
    let targetSummary: String
    let candidateSummary: String
    let candidateCanonicalText: String
    let candidateMemoryType: MemoryType
    let score: Double
    let policy: String
    let reasons: [String]
    let createdAt: String
}

struct ReplacementProposalListResponse: Codable {
    let project: String
    let proposals: [ReplacementProposalRecord]
}

struct ReplacementProposalResolutionResponse: Codable {
    let project: String
    let proposalId: String
    let status: String
    let policy: String
    let targetMemoryId: String
    let targetSummary: String
    let candidateSummary: String
    let newMemoryId: String?
}
