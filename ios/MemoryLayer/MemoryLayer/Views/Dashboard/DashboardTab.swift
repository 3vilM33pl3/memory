import SwiftUI

struct DashboardTab: View {
    let project: String
    let connection: ConnectionManager
    @State private var viewModel: DashboardViewModel
    @State private var showAgents = false
    @State private var showResume = false

    init(project: String, connection: ConnectionManager) {
        self.project = project
        self.connection = connection
        self._viewModel = State(initialValue: DashboardViewModel(connection: connection))
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: Theme.Spacing.lg) {
                    if viewModel.isLoading {
                        ProgressView()
                            .frame(maxWidth: .infinity, minHeight: 200)
                    } else if let overview = viewModel.overview {
                        HealthCard(
                            serviceOK: viewModel.isHealthy,
                            databaseOK: viewModel.isDatabaseHealthy
                        )

                        StatsRingView(
                            active: overview.activeMemories,
                            archived: overview.archivedMemories,
                            recent7d: overview.recentMemories7d,
                            total: overview.memoryEntriesTotal
                        )

                        QuickActionsGrid(
                            pendingProposals: viewModel.pendingProposals,
                            onResume: { showResume = true },
                            onAgents: { showAgents = true }
                        )
                    }

                    if let error = viewModel.error {
                        Text(error)
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                }
                .padding(Theme.Spacing.lg)
            }
            .refreshable {
                await viewModel.refresh(project: project)
            }
            .navigationTitle(project)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    ConnectionStatusIndicator(state: connection.state)
                }
            }
            .sheet(isPresented: $showAgents) {
                AgentsView(connection: connection)
            }
            .sheet(isPresented: $showResume) {
                ResumeView(project: project, connection: connection)
            }
            .onAppear {
                viewModel.startPolling(project: project)
            }
            .onDisappear {
                viewModel.stopPolling()
            }
        }
    }
}
