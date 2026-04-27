import Foundation

enum MemoryType: String, Codable, CaseIterable, Identifiable {
    case architecture
    case convention
    case decision
    case incident
    case debugging
    case environment
    case domainFact = "domain_fact"
    case plan
    case implementation
    case user
    case feedback
    case project
    case reference

    var id: String { rawValue }
}

enum MemoryStatus: String, Codable, CaseIterable, Identifiable {
    case active
    case archived

    var id: String { rawValue }
}

enum SourceKind: String, Codable, CaseIterable, Identifiable {
    case taskPrompt = "task_prompt"
    case file
    case gitCommit = "git_commit"
    case commandOutput = "command_output"
    case test
    case note

    var id: String { rawValue }
}

enum QueryMatchKind: String, Codable {
    case lexical
    case semantic
    case hybrid
}

enum ActivityKind: String, Codable, CaseIterable, Identifiable {
    case checkpoint
    case scan
    case plan
    case commitSync = "commit_sync"
    case bundleExport = "bundle_export"
    case bundleImport = "bundle_import"
    case query
    case queryError = "query_error"
    case watcherHealth = "watcher_health"
    case memoryReplacement = "memory_replacement"
    case captureTask = "capture_task"
    case curate
    case reindex
    case reembed
    case archive
    case deleteMemory = "delete_memory"

    var id: String { rawValue }
}

enum ReplacementPolicy: String, Codable, CaseIterable, Identifiable {
    case conservative
    case balanced
    case aggressive

    var id: String { rawValue }
}

enum MemoryRelationType: String, Codable, CaseIterable, Identifiable {
    case supersedes
    case relatedTo = "related_to"
    case contradicts
    case refines
    case dependsOn = "depends_on"

    var id: String { rawValue }
}

enum AgentStatus: String, Codable {
    case working
    case waiting
    case done
}

enum QueryAnswerMethod: String, Codable {
    case deterministic
    case llm
    case fallback
}
