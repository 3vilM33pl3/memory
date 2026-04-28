import Foundation
import simd

enum GraphNodeKind: String, Codable {
    case memory
    case codeSymbol
}

struct GraphNode: Identifiable {
    let id: String
    let label: String
    let nodeKind: GraphNodeKind
    let memoryType: MemoryType?
    let importance: Double
    let confidence: Double
    let tags: [String]
    let memoryId: String?
    let symbolKind: String?
    var position: SIMD3<Float>
    var velocity: SIMD3<Float>
}

struct GraphEdge: Identifiable {
    let id: String
    let sourceId: String
    let targetId: String
    let edgeKind: String
    let direction: String?
}

struct ForceLayoutConfig {
    var repulsionStrength: Float = 500
    var springStrength: Float = 0.01
    var springLength: Float = 2.0
    var damping: Float = 0.9
    var maxIterationsPerFrame: Int = 5
    var convergenceThreshold: Float = 0.01
}
