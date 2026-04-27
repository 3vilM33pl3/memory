import Foundation

struct ActivityEvent: Codable, Identifiable {
    let project: String
    let kind: ActivityKind
    let memoryId: String?
    let summary: String
    let details: ActivityDetails?
    let recordedAt: String

    var id: String { "\(kind.rawValue)-\(recordedAt)" }
}

enum ActivityDetails: Codable {
    case checkpoint(CheckpointDetails)
    case plan(PlanDetails)
    case scan(ScanDetails)
    case commitSync(CommitSyncDetails)
    case bundleTransfer(BundleTransferDetails)
    case query(QueryDetails)
    case watcherHealth(WatcherHealthDetails)
    case memoryReplacement(MemoryReplacementDetails)
    case captureTask(CaptureTaskDetails)
    case curate(CurateDetails)
    case reindex(ReindexDetails)
    case reembed(ReembedDetails)
    case archive(ArchiveDetails)
    case deleteMemory(DeleteMemoryDetails)
    case unknown

    struct CheckpointDetails: Codable {
        let repoRoot: String
        let markedAt: String
        let note: String?
        let gitBranch: String?
        let gitHead: String?
    }

    struct PlanDetails: Codable {
        let action: String
        let title: String
        let threadKey: String
        let totalItems: Int
        let completedItems: Int
        let remainingItems: [String]
        let sourcePath: String?
        let verifiedComplete: Bool
    }

    struct ScanDetails: Codable {
        let dryRun: Bool
        let candidateCount: Int
        let filesConsidered: Int
        let commitsConsidered: Int
        let indexReused: Bool
        let reportPath: String
        let captureId: String?
        let curateRunId: String?
    }

    struct CommitSyncDetails: Codable {
        let importedCount: Int
        let updatedCount: Int
        let totalReceived: Int
        let newestCommit: String?
        let oldestCommit: String?
    }

    struct BundleTransferDetails: Codable {
        let bundleId: String
        let itemCount: Int
        let sourceProject: String?
    }

    struct QueryDetails: Codable {
        let query: String
        let topK: Int
        let resultCount: Int
        let confidence: Double
        let insufficientEvidence: Bool
        let totalDurationMs: Int
        let answer: String?
        let error: String?
    }

    struct WatcherHealthDetails: Codable {
        let watcherId: String
        let hostname: String
        let health: String
        let managedByService: Bool
        let restartAttemptCount: Int
        let agentCli: String?
        let agentSessionId: String?
        let agentPid: Int?
        let previousHealth: String?
        let recoveredAfterRestartAttempts: Int?
        let message: String?
    }

    struct MemoryReplacementDetails: Codable {
        let oldMemoryId: String
        let oldSummary: String
        let newMemoryId: String
        let newSummary: String
        let automatic: Bool
        let policy: ReplacementPolicy
    }

    struct CaptureTaskDetails: Codable {
        let sessionId: String
        let taskId: String
        let rawCaptureId: String
        let idempotencyKey: String
        let taskTitle: String?
        let writerId: String
    }

    struct CurateDetails: Codable {
        let runId: String
        let inputCount: Int
        let outputCount: Int
        let replacedCount: Int
        let proposalCount: Int
    }

    struct ReindexDetails: Codable {
        let reindexedEntries: Int
    }

    struct ReembedDetails: Codable {
        let reembeddedChunks: Int
    }

    struct ArchiveDetails: Codable {
        let archivedCount: Int
        let maxConfidence: Double
        let maxImportance: Double
    }

    struct DeleteMemoryDetails: Codable {
        let deleted: Bool
        let summary: String
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)

        switch type {
        case "checkpoint":
            self = .checkpoint(try CheckpointDetails(from: decoder))
        case "plan":
            self = .plan(try PlanDetails(from: decoder))
        case "scan":
            self = .scan(try ScanDetails(from: decoder))
        case "commit_sync":
            self = .commitSync(try CommitSyncDetails(from: decoder))
        case "bundle_transfer":
            self = .bundleTransfer(try BundleTransferDetails(from: decoder))
        case "query":
            self = .query(try QueryDetails(from: decoder))
        case "watcher_health":
            self = .watcherHealth(try WatcherHealthDetails(from: decoder))
        case "memory_replacement":
            self = .memoryReplacement(try MemoryReplacementDetails(from: decoder))
        case "capture_task":
            self = .captureTask(try CaptureTaskDetails(from: decoder))
        case "curate":
            self = .curate(try CurateDetails(from: decoder))
        case "reindex":
            self = .reindex(try ReindexDetails(from: decoder))
        case "reembed":
            self = .reembed(try ReembedDetails(from: decoder))
        case "archive":
            self = .archive(try ArchiveDetails(from: decoder))
        case "delete_memory":
            self = .deleteMemory(try DeleteMemoryDetails(from: decoder))
        default:
            self = .unknown
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .checkpoint(let d):
            try container.encode("checkpoint", forKey: .type)
            try d.encode(to: encoder)
        case .plan(let d):
            try container.encode("plan", forKey: .type)
            try d.encode(to: encoder)
        case .scan(let d):
            try container.encode("scan", forKey: .type)
            try d.encode(to: encoder)
        case .commitSync(let d):
            try container.encode("commit_sync", forKey: .type)
            try d.encode(to: encoder)
        case .bundleTransfer(let d):
            try container.encode("bundle_transfer", forKey: .type)
            try d.encode(to: encoder)
        case .query(let d):
            try container.encode("query", forKey: .type)
            try d.encode(to: encoder)
        case .watcherHealth(let d):
            try container.encode("watcher_health", forKey: .type)
            try d.encode(to: encoder)
        case .memoryReplacement(let d):
            try container.encode("memory_replacement", forKey: .type)
            try d.encode(to: encoder)
        case .captureTask(let d):
            try container.encode("capture_task", forKey: .type)
            try d.encode(to: encoder)
        case .curate(let d):
            try container.encode("curate", forKey: .type)
            try d.encode(to: encoder)
        case .reindex(let d):
            try container.encode("reindex", forKey: .type)
            try d.encode(to: encoder)
        case .reembed(let d):
            try container.encode("reembed", forKey: .type)
            try d.encode(to: encoder)
        case .archive(let d):
            try container.encode("archive", forKey: .type)
            try d.encode(to: encoder)
        case .deleteMemory(let d):
            try container.encode("delete_memory", forKey: .type)
            try d.encode(to: encoder)
        case .unknown:
            try container.encode("unknown", forKey: .type)
        }
    }

    private enum CodingKeys: String, CodingKey {
        case type
    }
}
