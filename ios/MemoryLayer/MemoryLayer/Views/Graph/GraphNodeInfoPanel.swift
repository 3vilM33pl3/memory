import SwiftUI

struct GraphNodeInfoPanel: View {
    let node: GraphNode
    let project: String
    let connection: ConnectionManager

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text(node.label)
                    .font(.headline)

                HStack(spacing: 8) {
                    if node.nodeKind == .codeSymbol {
                        Label(node.symbolKind ?? "Symbol", systemImage: "chevron.left.forwardslash.chevron.right")
                            .font(.caption)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(.gray.opacity(0.2))
                            .clipShape(Capsule())
                    } else if let memoryType = node.memoryType {
                        MemoryTypeBadge(type: memoryType)
                    }

                    ConfidenceBadge(confidence: node.confidence)
                }

                LabeledContent("Importance") {
                    Text(String(format: "%.0f%%", node.importance * 100))
                }

                LabeledContent("Kind") {
                    Text(node.nodeKind == .codeSymbol ? "Code Symbol" : "Memory")
                }

                if !node.tags.isEmpty {
                    TagCloudView(tags: node.tags)
                }

                if let memoryId = node.memoryId {
                    NavigationLink(destination: MemoryDetailView(
                        memoryId: memoryId,
                        connection: connection
                    )) {
                        Label("View Memory", systemImage: "arrow.right.circle")
                    }
                    .buttonStyle(.borderedProminent)
                    .padding(.top, 8)
                }
            }
            .padding()
        }
        .navigationTitle("Node Details")
        .navigationBarTitleDisplayMode(.inline)
    }
}
