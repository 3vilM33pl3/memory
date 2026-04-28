import Foundation
import simd

@Observable
final class GraphViewModel {
    var nodes: [GraphNode] = []
    var edges: [GraphEdge] = []
    var selectedNodeId: String?
    var isLoading = false
    var error: String?
    var isSimulating = false
    var searchText = ""
    var layoutConfig = ForceLayoutConfig()

    private let connection: ConnectionManager
    private var simulationTask: Task<Void, Never>?
    private let maxNodes = 300

    init(connection: ConnectionManager) {
        self.connection = connection
    }

    var selectedNode: GraphNode? {
        guard let id = selectedNodeId else { return nil }
        return nodes.first { $0.id == id }
    }

    var nodeCount: Int { nodes.count }
    var edgeCount: Int { edges.count }

    // MARK: - Data Loading

    func loadGraph(project: String) async {
        guard !isLoading else { return }
        isLoading = true
        error = nil

        do {
            // Fetch all memories via paginated calls
            var allItems: [ProjectMemoryListItem] = []
            var page = 1
            let perPage = 100
            while true {
                let response = try await connection.api.getMemories(
                    project: project, page: page, perPage: perPage
                )
                allItems.append(contentsOf: response.items)
                if response.items.count < perPage { break }
                page += 1
            }

            // Sort by importance and cap
            let sorted = allItems.sorted { $0.importance > $1.importance }
            let capped = Array(sorted.prefix(maxNodes))
            let cappedIds = Set(capped.map(\.id))

            // Build nodes with random initial positions
            var graphNodes: [GraphNode] = capped.map { item in
                GraphNode(
                    id: item.id,
                    label: item.summary,
                    nodeKind: .memory,
                    memoryType: item.memoryType,
                    importance: item.importance,
                    confidence: item.confidence,
                    tags: item.tags,
                    memoryId: item.id,
                    symbolKind: nil,
                    position: SIMD3<Float>(
                        Float.random(in: -5...5),
                        Float.random(in: -5...5),
                        Float.random(in: -5...5)
                    ),
                    velocity: .zero
                )
            }

            // Batch-fetch details for relations using TaskGroup
            var allEdges: [GraphEdge] = []
            var seenEdgePairs = Set<String>()

            await withTaskGroup(of: [GraphEdge].self) { group in
                var launched = 0
                for node in graphNodes {
                    guard let memoryId = node.memoryId else { continue }
                    group.addTask { [weak self] in
                        guard let self else { return [] }
                        do {
                            let detail = try await self.connection.api.getMemory(id: memoryId)
                            return detail.relatedMemories.compactMap { rel in
                                guard cappedIds.contains(rel.memoryId) else { return nil }
                                let pairKey = [memoryId, rel.memoryId].sorted().joined(separator: "-")
                                return GraphEdge(
                                    id: pairKey,
                                    sourceId: memoryId,
                                    targetId: rel.memoryId,
                                    edgeKind: rel.relationType,
                                    direction: nil
                                )
                            }
                        } catch {
                            return []
                        }
                    }
                    launched += 1
                    // Limit concurrency to 10
                    if launched >= 10 {
                        if let result = await group.next() {
                            for edge in result {
                                if seenEdgePairs.insert(edge.id).inserted {
                                    allEdges.append(edge)
                                }
                            }
                        }
                        launched -= 1
                    }
                }
                for await result in group {
                    for edge in result {
                        if seenEdgePairs.insert(edge.id).inserted {
                            allEdges.append(edge)
                        }
                    }
                }
            }

            nodes = graphNodes
            edges = allEdges
            isLoading = false

            startSimulation()
        } catch {
            self.error = error.localizedDescription
            isLoading = false
        }
    }

    // MARK: - Code Graph Integration

    func addCodeGraphConnections(_ connections: [QueryGraphConnection]) {
        for conn in connections {
            let symbolId = "code-\(conn.symbol ?? conn.filePath)"
            if !nodes.contains(where: { $0.id == symbolId }) {
                let node = GraphNode(
                    id: symbolId,
                    label: conn.symbol ?? conn.filePath,
                    nodeKind: .codeSymbol,
                    memoryType: nil,
                    importance: conn.scoreBoost,
                    confidence: 1.0,
                    tags: [],
                    memoryId: nil,
                    symbolKind: conn.symbolKind,
                    position: SIMD3<Float>(
                        Float.random(in: -3...3),
                        Float.random(in: -3...3),
                        Float.random(in: -3...3)
                    ),
                    velocity: .zero
                )
                nodes.append(node)
            }

            if let neighbor = conn.neighborSymbol {
                let neighborId = "code-\(neighbor)"
                if !nodes.contains(where: { $0.id == neighborId }) {
                    let neighborNode = GraphNode(
                        id: neighborId,
                        label: neighbor,
                        nodeKind: .codeSymbol,
                        memoryType: nil,
                        importance: 0.5,
                        confidence: 1.0,
                        tags: [],
                        memoryId: nil,
                        symbolKind: nil,
                        position: SIMD3<Float>(
                            Float.random(in: -3...3),
                            Float.random(in: -3...3),
                            Float.random(in: -3...3)
                        ),
                        velocity: .zero
                    )
                    nodes.append(neighborNode)
                }

                let edgeId = [symbolId, neighborId].sorted().joined(separator: "-")
                if !edges.contains(where: { $0.id == edgeId }) {
                    edges.append(GraphEdge(
                        id: edgeId,
                        sourceId: symbolId,
                        targetId: neighborId,
                        edgeKind: conn.edgeKind ?? "code_ref",
                        direction: conn.direction
                    ))
                }
            }
        }

        startSimulation()
    }

    // MARK: - Force-Directed Layout

    func startSimulation() {
        simulationTask?.cancel()
        isSimulating = true

        simulationTask = Task { @MainActor [weak self] in
            guard let self else { return }
            while !Task.isCancelled && self.isSimulating {
                for _ in 0..<self.layoutConfig.maxIterationsPerFrame {
                    self.stepSimulation()
                }

                // Check convergence
                let maxVel = self.nodes.map { simd_length($0.velocity) }.max() ?? 0
                if maxVel < self.layoutConfig.convergenceThreshold {
                    self.isSimulating = false
                    break
                }

                try? await Task.sleep(for: .milliseconds(16))
            }
        }
    }

    func stopSimulation() {
        isSimulating = false
        simulationTask?.cancel()
        simulationTask = nil
    }

    func toggleSimulation() {
        if isSimulating {
            stopSimulation()
        } else {
            startSimulation()
        }
    }

    private func stepSimulation() {
        let count = nodes.count
        guard count > 0 else { return }

        // Repulsion: Coulomb's law between all node pairs
        var forces = [SIMD3<Float>](repeating: .zero, count: count)

        for i in 0..<count {
            for j in (i + 1)..<count {
                let delta = nodes[i].position - nodes[j].position
                let dist = max(simd_length(delta), 0.1)
                let repulsion = layoutConfig.repulsionStrength / (dist * dist)
                let direction = delta / dist
                forces[i] += direction * repulsion
                forces[j] -= direction * repulsion
            }
        }

        // Attraction: Hooke's law along edges
        let nodeIndex = Dictionary(uniqueKeysWithValues: nodes.enumerated().map { ($1.id, $0) })
        for edge in edges {
            guard let si = nodeIndex[edge.sourceId],
                  let ti = nodeIndex[edge.targetId] else { continue }
            let delta = nodes[ti].position - nodes[si].position
            let dist = simd_length(delta)
            guard dist > 0.01 else { continue }
            let displacement = dist - layoutConfig.springLength
            let direction = delta / dist
            let attraction = layoutConfig.springStrength * displacement
            forces[si] += direction * attraction
            forces[ti] -= direction * attraction
        }

        // Center gravity
        for i in 0..<count {
            let toCenter = -nodes[i].position
            forces[i] += toCenter * 0.01
        }

        // Apply forces with damping
        for i in 0..<count {
            nodes[i].velocity = (nodes[i].velocity + forces[i]) * layoutConfig.damping
            nodes[i].position += nodes[i].velocity
        }
    }
}
