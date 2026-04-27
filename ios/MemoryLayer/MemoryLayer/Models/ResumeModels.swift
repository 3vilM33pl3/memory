import Foundation

struct ResumeCheckpoint: Codable {
    let project: String
    let repoRoot: String
    let markedAt: String
    let note: String?
    let gitBranch: String?
    let gitHead: String?
}

struct ResumeAction: Codable, Identifiable {
    let title: String
    let rationale: String
    let commandHint: String?

    var id: String { title }
}

struct CommitRecord: Codable, Identifiable {
    let hash: String
    let shortHash: String
    let subject: String
    let body: String
    let authorName: String?
    let authorEmail: String?
    let committedAt: String
    let parentHashes: [String]
    let changedPaths: [String]
    let importedAt: String

    var id: String { hash }
}

struct ResumeResponse: Codable {
    let project: String
    let generatedAt: String
    let checkpoint: ResumeCheckpoint?
    let briefing: String
    let currentThread: String?
    let changeSummary: [String]
    let attentionItems: [String]
    let primaryNextStep: ResumeAction?
    let secondaryNextSteps: [ResumeAction]
    let contextItems: [ProjectMemoryListItem]
    let timeline: [ActivityEvent]
    let commits: [CommitRecord]
    let changedMemories: [ProjectMemoryListItem]
    let durableContext: [ProjectMemoryListItem]
    let warnings: [String]
    let actions: [ResumeAction]
    let overview: ProjectOverviewResponse
}
