import Foundation

struct ChildProcessResponse: Codable, Identifiable {
    let pid: Int
    let command: String
    let memKb: Int
    let port: Int?

    var id: Int { pid }
}

struct SubAgentResponse: Codable, Identifiable {
    let name: String
    let status: String
    let tokens: Int

    var id: String { name }
}

struct AgentSessionResponse: Codable, Identifiable {
    let agentCli: String
    let pid: Int
    let sessionId: String
    let cwd: String
    let projectName: String
    let startedAt: Double
    let status: AgentStatus
    let model: String
    let contextPercent: Double
    let totalInputTokens: Int
    let totalOutputTokens: Int
    let totalCacheRead: Int
    let totalCacheCreate: Int
    let turnCount: Int
    let currentTasks: [String]
    let memMb: Double
    let version: String
    let gitBranch: String
    let gitAdded: Int
    let gitModified: Int
    let tokenHistory: [Int]
    let subagents: [SubAgentResponse]
    let memFileCount: Int
    let memLineCount: Int
    let children: [ChildProcessResponse]
    let initialPrompt: String
    let firstAssistantText: String

    var id: String { sessionId }
}

struct RateLimitResponse: Codable, Identifiable {
    let source: String
    let fiveHourPct: Double?
    let fiveHourResetsAt: Double?
    let sevenDayPct: Double?
    let sevenDayResetsAt: Double?
    let updatedAt: Double?

    var id: String { source }
}

struct OrphanPortResponse: Codable, Identifiable {
    let port: Int
    let pid: Int
    let command: String
    let projectName: String

    var id: Int { port }
}

struct AgentSnapshotResponse: Codable {
    let collectedAt: String
    let sessions: [AgentSessionResponse]
    let orphanPorts: [OrphanPortResponse]
    let rateLimits: [RateLimitResponse]
}
