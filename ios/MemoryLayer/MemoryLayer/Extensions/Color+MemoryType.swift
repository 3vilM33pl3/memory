import SwiftUI

extension MemoryType {
    var color: Color {
        switch self {
        case .architecture: return .blue
        case .convention: return .teal
        case .decision: return .orange
        case .incident: return .red
        case .debugging: return .yellow
        case .environment: return .gray
        case .domainFact: return .indigo
        case .plan: return .purple
        case .implementation: return .green
        case .user: return .cyan
        case .feedback: return .pink
        case .project: return .mint
        case .reference: return .brown
        }
    }

    var icon: String {
        switch self {
        case .architecture: return "building.2"
        case .convention: return "list.bullet.rectangle"
        case .decision: return "arrow.triangle.branch"
        case .incident: return "exclamationmark.triangle"
        case .debugging: return "ladybug"
        case .environment: return "gearshape"
        case .domainFact: return "book"
        case .plan: return "map"
        case .implementation: return "hammer"
        case .user: return "person"
        case .feedback: return "bubble.left"
        case .project: return "folder"
        case .reference: return "link"
        }
    }

    var displayName: String {
        switch self {
        case .domainFact: return "Domain Fact"
        default: return rawValue.capitalized
        }
    }
}

extension ActivityKind {
    var color: Color {
        switch self {
        case .curate: return .purple
        case .query, .queryError: return .blue
        case .captureTask, .scan: return .green
        case .checkpoint, .plan: return .orange
        case .memoryReplacement: return .indigo
        case .watcherHealth: return .teal
        case .archive, .deleteMemory: return .red
        case .reindex, .reembed: return .cyan
        case .commitSync: return .mint
        case .bundleExport, .bundleImport: return .brown
        }
    }

    var icon: String {
        switch self {
        case .checkpoint: return "flag"
        case .scan: return "doc.text.magnifyingglass"
        case .plan: return "map"
        case .commitSync: return "arrow.triangle.2.circlepath"
        case .bundleExport: return "arrow.up.doc"
        case .bundleImport: return "arrow.down.doc"
        case .query: return "magnifyingglass"
        case .queryError: return "magnifyingglass.circle.fill"
        case .watcherHealth: return "heart.text.square"
        case .memoryReplacement: return "arrow.2.squarepath"
        case .captureTask: return "tray.and.arrow.down"
        case .curate: return "wand.and.stars"
        case .reindex: return "arrow.clockwise"
        case .reembed: return "cube"
        case .archive: return "archivebox"
        case .deleteMemory: return "trash"
        }
    }
}
