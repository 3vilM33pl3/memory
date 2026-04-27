import Foundation

struct QueryRequest: Codable {
    let project: String
    let query: String
    let filters: [String: String]
    let topK: Int
    let minConfidence: Double?
    let history: Bool?
}

struct QueryDiagnostics: Codable {
    let lexicalCandidates: Int
    let semanticCandidates: Int
    let mergedCandidates: Int
    let returnedResults: Int
    let relationAugmentedCandidates: Int
    let lexicalDurationMs: Int
    let semanticDurationMs: Int
    let rerankDurationMs: Int
    let totalDurationMs: Int
    let semanticStatus: String
}

struct QueryAnswerGeneration: Codable {
    let method: QueryAnswerMethod
    let citedResultNumbers: [Int]
    let evidenceCount: Int
    let durationMs: Int
    let fallbackReason: String?
}

struct QueryAnswerCitation: Codable, Identifiable {
    let resultNumber: Int
    let memoryId: String
    let memoryType: MemoryType
    let summary: String
    let snippet: String

    var id: Int { resultNumber }
}

struct QueryResultDebug: Codable {
    let chunkFts: Double
    let entryFts: Double
    let semanticSimilarity: Double
    let exactPhraseMatches: Double
    let termOverlap: Double
    let tagMatchCount: Double
    let pathMatchCount: Double
    let relationBoost: Double
    let importance: Double
    let memoryConfidence: Double
    let recencyBoost: Double
}

struct QueryResultSource: Codable {
    let id: String?
    let taskId: String?
    let filePath: String?
    let gitCommit: String?
    let sourceKind: SourceKind?
    let excerpt: String?
}

struct QueryResult: Codable, Identifiable {
    let memoryId: String
    let summary: String
    let snippet: String
    let memoryType: MemoryType
    let score: Double
    let matchKind: QueryMatchKind
    let scoreExplanation: [String]
    let debug: QueryResultDebug
    let tags: [String]
    let sources: [QueryResultSource]

    var id: String { memoryId }
}

struct QueryResponse: Codable {
    let answer: String
    let confidence: Double
    let insufficientEvidence: Bool
    let answerGeneration: QueryAnswerGeneration
    let answerCitations: [QueryAnswerCitation]
    let results: [QueryResult]
    let diagnostics: QueryDiagnostics
}
