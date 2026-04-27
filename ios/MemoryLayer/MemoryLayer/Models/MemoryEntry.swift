import Foundation

struct ProjectMemoryListItem: Codable, Identifiable {
    let id: String
    let summary: String
    let preview: String
    let memoryType: MemoryType
    let confidence: Double
    let importance: Double
    let status: MemoryStatus
    let updatedAt: String
    let tags: [String]
    let tagCount: Int
    let sourceCount: Int
    let canonicalId: String
    let versionNo: Int
    let isTombstone: Bool
}

struct ProjectMemoriesResponse: Codable {
    let project: String
    let total: Int
    let items: [ProjectMemoryListItem]
}

struct MemorySourceRecord: Codable, Identifiable {
    let id: String
    let taskId: String?
    let filePath: String?
    let gitCommit: String?
    let sourceKind: SourceKind
    let excerpt: String?
}

struct RelatedMemorySummary: Codable, Identifiable {
    let memoryId: String
    let relationType: String
    let summary: String
    let memoryType: MemoryType
    let confidence: Double

    var id: String { memoryId }
}

struct MemoryEmbeddingSpace: Codable, Identifiable {
    let provider: String
    let model: String
    let baseUrl: String
    let chunkCount: Int
    let lastUpdated: String?

    var id: String { "\(provider)-\(model)" }
}

struct MemoryEntryResponse: Codable, Identifiable {
    let id: String
    let project: String
    let canonicalText: String
    let summary: String
    let memoryType: MemoryType
    let importance: Double
    let confidence: Double
    let status: MemoryStatus
    let tags: [String]
    let sources: [MemorySourceRecord]
    let relatedMemories: [RelatedMemorySummary]
    let embeddingSpaces: [MemoryEmbeddingSpace]
    let createdAt: String
    let updatedAt: String
    let canonicalId: String
    let versionNo: Int
    let isTombstone: Bool
}

struct MemoryHistoryResponse: Codable {
    let canonicalId: String
    let project: String
    let versions: [MemoryEntryResponse]
}
