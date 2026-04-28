import SwiftUI

struct GraphTab: View {
    let project: String
    let connection: ConnectionManager
    @State private var viewModel: GraphViewModel

    @Environment(\.horizontalSizeClass) private var sizeClass

    init(project: String, connection: ConnectionManager) {
        self.project = project
        self.connection = connection
        self._viewModel = State(initialValue: GraphViewModel(connection: connection))
    }

    var body: some View {
        NavigationStack {
            ZStack {
                if viewModel.isLoading && viewModel.nodes.isEmpty {
                    ProgressView("Loading knowledge graph...")
                } else if let error = viewModel.error, viewModel.nodes.isEmpty {
                    VStack(spacing: 12) {
                        Image(systemName: "exclamationmark.triangle")
                            .font(.largeTitle)
                            .foregroundStyle(.secondary)
                        Text(error)
                            .foregroundStyle(.secondary)
                        Button("Retry") {
                            Task { await viewModel.loadGraph(project: project) }
                        }
                        .buttonStyle(.borderedProminent)
                    }
                } else {
                    GraphRealityView(viewModel: viewModel)

                    GraphControlsOverlay(
                        viewModel: viewModel,
                        project: project,
                        connection: connection
                    ) { query in
                        await searchCodeGraph(query: query)
                    }
                }
            }
            .navigationTitle("Knowledge Graph")
            .navigationBarTitleDisplayMode(.inline)
            .task {
                await viewModel.loadGraph(project: project)
            }
            .modifier(NodeInfoModifier(
                viewModel: viewModel,
                project: project,
                connection: connection,
                sizeClass: sizeClass
            ))
        }
    }

    private func searchCodeGraph(query: String) async {
        guard !query.isEmpty else { return }
        do {
            let request = QueryRequest(
                project: project,
                query: query,
                filters: [:],
                topK: 10,
                minConfidence: nil,
                history: nil
            )
            let response = try await connection.api.query(request)
            let connections = response.results.flatMap { $0.graphConnections ?? [] }
            if !connections.isEmpty {
                viewModel.addCodeGraphConnections(connections)
            }
        } catch {
            // Search failed silently - the graph still works
        }
    }
}

private struct NodeInfoModifier: ViewModifier {
    let viewModel: GraphViewModel
    let project: String
    let connection: ConnectionManager
    let sizeClass: UserInterfaceSizeClass?

    func body(content: Content) -> some View {
        if sizeClass == .regular {
            content.inspector(isPresented: .init(
                get: { viewModel.selectedNode != nil },
                set: { if !$0 { viewModel.selectedNodeId = nil } }
            )) {
                if let node = viewModel.selectedNode {
                    GraphNodeInfoPanel(node: node, project: project, connection: connection)
                }
            }
        } else {
            content.sheet(isPresented: .init(
                get: { viewModel.selectedNode != nil },
                set: { if !$0 { viewModel.selectedNodeId = nil } }
            )) {
                if let node = viewModel.selectedNode {
                    NavigationStack {
                        GraphNodeInfoPanel(node: node, project: project, connection: connection)
                    }
                    .presentationDetents([.medium, .large])
                }
            }
        }
    }
}
