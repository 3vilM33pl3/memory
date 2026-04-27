import Foundation

struct NamedCount: Codable, Identifiable {
    let name: String
    let count: Int
    var id: String { name }
}

struct MemoryTypeCount: Codable, Identifiable {
    let memoryType: MemoryType
    let count: Int
    var id: String { memoryType.rawValue }
}

struct SourceKindCount: Codable, Identifiable {
    let sourceKind: SourceKind
    let count: Int
    var id: String { sourceKind.rawValue }
}

struct AutomationStatus: Codable {
    let enabled: Bool
    let mode: String
    let repoRoot: String
    let dirtyFileCount: Int?
    let pendingNoteCount: Int?
    let lastActivityAt: String?
    let lastPersistedAt: String?
    let lastDecision: String?
}

struct WatcherPresence: Codable, Identifiable {
    let watcherId: String
    let project: String
    let repoRoot: String
    let hostname: String
    let hostServiceId: String
    let pid: Int
    let mode: String
    let managedByService: Bool
    let health: String
    let startedAt: String
    let lastHeartbeatAt: String
    let agentCli: String?
    let agentSessionId: String?
    let agentPid: Int?
    let agentStartedAt: String?
    let lastRestartAttemptAt: String?
    let restartAttemptCount: Int

    var id: String { watcherId }
}

struct WatcherPresenceSummary: Codable {
    let activeCount: Int
    let unhealthyCount: Int
    let staleAfterSeconds: Int
    let lastHeartbeatAt: String?
    let watchers: [WatcherPresence]
}

struct ProjectOverviewResponse: Codable {
    let project: String
    let serviceStatus: String
    let databaseStatus: String
    let memoryEntriesTotal: Int
    let activeMemories: Int
    let archivedMemories: Int
    let highConfidenceMemories: Int
    let mediumConfidenceMemories: Int
    let lowConfidenceMemories: Int
    let recentMemories7d: Int
    let recentCaptures7d: Int
    let rawCapturesTotal: Int
    let uncuratedRawCaptures: Int
    let tasksTotal: Int
    let sessionsTotal: Int
    let curationRunsTotal: Int
    let lastMemoryAt: String?
    let lastCurationAt: String?
    let lastCaptureAt: String?
    let oldestUncuratedCaptureAgeHours: Double?
    let embeddingChunksTotal: Int
    let freshEmbeddingChunks: Int
    let staleEmbeddingChunks: Int
    let missingEmbeddingChunks: Int
    let embeddingSpacesTotal: Int
    let activeEmbeddingProvider: String?
    let activeEmbeddingModel: String?
    let pendingReplacementProposals: Int
    let topTags: [NamedCount]
    let topFiles: [NamedCount]
    let memoryTypeBreakdown: [MemoryTypeCount]
    let sourceKindBreakdown: [SourceKindCount]
    let automation: AutomationStatus?
    let watchers: WatcherPresenceSummary?
}
