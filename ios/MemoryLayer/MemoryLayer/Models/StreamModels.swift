import Foundation

enum StreamRequest: Encodable {
    case health
    case projectOverview(project: String)
    case projectMemories(project: String)
    case memoryDetail(memoryId: String)
    case subscribeProject(project: String)
    case subscribeMemory(memoryId: String)
    case unsubscribeMemory
    case ping

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .health:
            try container.encode("health", forKey: .type)
        case .projectOverview(let project):
            try container.encode("project_overview", forKey: .type)
            try container.encode(project, forKey: .project)
        case .projectMemories(let project):
            try container.encode("project_memories", forKey: .type)
            try container.encode(project, forKey: .project)
        case .memoryDetail(let memoryId):
            try container.encode("memory_detail", forKey: .type)
            try container.encode(memoryId, forKey: .memoryId)
        case .subscribeProject(let project):
            try container.encode("subscribe_project", forKey: .type)
            try container.encode(project, forKey: .project)
        case .subscribeMemory(let memoryId):
            try container.encode("subscribe_memory", forKey: .type)
            try container.encode(memoryId, forKey: .memoryId)
        case .unsubscribeMemory:
            try container.encode("unsubscribe_memory", forKey: .type)
        case .ping:
            try container.encode("ping", forKey: .type)
        }
    }

    private enum CodingKeys: String, CodingKey {
        case type
        case project
        case memoryId = "memory_id"
    }
}

enum StreamResponse: Codable {
    case health(value: [String: AnyCodable])
    case projectOverview(value: ProjectOverviewResponse)
    case projectMemories(value: ProjectMemoriesResponse)
    case memoryDetail(value: MemoryEntryResponse?)
    case projectSnapshot(overview: ProjectOverviewResponse, memories: ProjectMemoriesResponse)
    case projectChanged(overview: ProjectOverviewResponse, memories: ProjectMemoriesResponse)
    case memorySnapshot(detail: MemoryEntryResponse?)
    case memoryChanged(detail: MemoryEntryResponse?)
    case activity(event: ActivityEvent)
    case ack(message: String)
    case pong
    case error(message: String)

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)

        switch type {
        case "health":
            let value = try container.decode([String: AnyCodable].self, forKey: .value)
            self = .health(value: value)
        case "project_overview":
            let value = try container.decode(ProjectOverviewResponse.self, forKey: .value)
            self = .projectOverview(value: value)
        case "project_memories":
            let value = try container.decode(ProjectMemoriesResponse.self, forKey: .value)
            self = .projectMemories(value: value)
        case "memory_detail":
            let value = try container.decodeIfPresent(MemoryEntryResponse.self, forKey: .value)
            self = .memoryDetail(value: value)
        case "project_snapshot":
            let overview = try container.decode(ProjectOverviewResponse.self, forKey: .overview)
            let memories = try container.decode(ProjectMemoriesResponse.self, forKey: .memories)
            self = .projectSnapshot(overview: overview, memories: memories)
        case "project_changed":
            let overview = try container.decode(ProjectOverviewResponse.self, forKey: .overview)
            let memories = try container.decode(ProjectMemoriesResponse.self, forKey: .memories)
            self = .projectChanged(overview: overview, memories: memories)
        case "memory_snapshot":
            let detail = try container.decodeIfPresent(MemoryEntryResponse.self, forKey: .detail)
            self = .memorySnapshot(detail: detail)
        case "memory_changed":
            let detail = try container.decodeIfPresent(MemoryEntryResponse.self, forKey: .detail)
            self = .memoryChanged(detail: detail)
        case "activity":
            let event = try container.decode(ActivityEvent.self, forKey: .event)
            self = .activity(event: event)
        case "ack":
            let message = try container.decode(String.self, forKey: .message)
            self = .ack(message: message)
        case "pong":
            self = .pong
        case "error":
            let message = try container.decode(String.self, forKey: .message)
            self = .error(message: message)
        default:
            self = .error(message: "Unknown stream response type: \(type)")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .pong:
            try container.encode("pong", forKey: .type)
        case .ack(let message):
            try container.encode("ack", forKey: .type)
            try container.encode(message, forKey: .message)
        case .error(let message):
            try container.encode("error", forKey: .type)
            try container.encode(message, forKey: .message)
        default:
            break
        }
    }

    private enum CodingKeys: String, CodingKey {
        case type, value, overview, memories, detail, event, message
    }
}

// Simple type-erased Codable for health response
struct AnyCodable: Codable {
    let value: Any

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let string = try? container.decode(String.self) {
            value = string
        } else if let int = try? container.decode(Int.self) {
            value = int
        } else if let double = try? container.decode(Double.self) {
            value = double
        } else if let bool = try? container.decode(Bool.self) {
            value = bool
        } else {
            value = "unknown"
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        if let string = value as? String {
            try container.encode(string)
        } else if let int = value as? Int {
            try container.encode(int)
        } else if let double = value as? Double {
            try container.encode(double)
        } else if let bool = value as? Bool {
            try container.encode(bool)
        }
    }
}
