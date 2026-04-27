import SwiftUI

struct ActivityTab: View {
    let project: String
    let connection: ConnectionManager
    @State private var viewModel: ActivityViewModel

    init(project: String, connection: ConnectionManager) {
        self.project = project
        self.connection = connection
        self._viewModel = State(initialValue: ActivityViewModel(connection: connection))
    }

    private let filterKinds: [ActivityKind?] = [nil, .captureTask, .curate, .query, .checkpoint]

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Filter chips
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: Theme.Spacing.sm) {
                        filterChip(label: "All", kind: nil)
                        filterChip(label: "Capture", kind: .captureTask)
                        filterChip(label: "Curate", kind: .curate)
                        filterChip(label: "Query", kind: .query)
                        filterChip(label: "Checkpoint", kind: .checkpoint)
                    }
                    .padding(.horizontal, Theme.Spacing.md)
                    .padding(.vertical, Theme.Spacing.sm)
                }

                Divider()

                if viewModel.isLoading && viewModel.events.isEmpty {
                    ProgressView()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if viewModel.filteredEvents.isEmpty {
                    EmptyStateView(
                        icon: "clock",
                        title: "No Activity",
                        message: "No activity events yet."
                    )
                } else {
                    List(viewModel.filteredEvents) { event in
                        ActivityEventRow(event: event)
                    }
                    .listStyle(.plain)
                }
            }
            .refreshable {
                await viewModel.loadInitial(project: project)
            }
            .navigationTitle("Activity")
            .task {
                await viewModel.loadInitial(project: project)
                viewModel.subscribeToStream(project: project)
            }
            .onDisappear {
                viewModel.unsubscribe()
            }
        }
    }

    private func filterChip(label: String, kind: ActivityKind?) -> some View {
        let isSelected = viewModel.filterKind == kind
        return Button(label) {
            viewModel.filterKind = kind
        }
        .font(.caption)
        .padding(.horizontal, Theme.Spacing.md)
        .padding(.vertical, Theme.Spacing.xs)
        .background(isSelected ? Color.accentColor : Color.secondary.opacity(0.12))
        .foregroundStyle(isSelected ? .white : .primary)
        .clipShape(Capsule())
    }
}
