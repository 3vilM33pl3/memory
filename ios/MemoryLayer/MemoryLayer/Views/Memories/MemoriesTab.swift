import SwiftUI

struct MemoriesTab: View {
    let project: String
    let connection: ConnectionManager
    @State private var viewModel: MemoriesViewModel
    @State private var showFilter = false

    init(project: String, connection: ConnectionManager) {
        self.project = project
        self.connection = connection
        self._viewModel = State(initialValue: MemoriesViewModel(connection: connection))
    }

    var body: some View {
        NavigationStack {
            Group {
                if viewModel.isLoading && viewModel.memories.isEmpty {
                    ProgressView()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if viewModel.filteredMemories.isEmpty {
                    EmptyStateView(
                        icon: "brain",
                        title: "No Memories",
                        message: "No memories match your filters."
                    )
                } else {
                    List {
                        ForEach(viewModel.filteredMemories) { memory in
                            NavigationLink(value: memory.id) {
                                MemoryRowView(memory: memory)
                            }
                        }
                        if viewModel.memories.count < viewModel.total {
                            ProgressView()
                                .frame(maxWidth: .infinity)
                                .onAppear {
                                    Task { await viewModel.loadMore(project: project) }
                                }
                        }
                    }
                    .listStyle(.plain)
                }
            }
            .searchable(text: $viewModel.searchText, prompt: "Search memories")
            .refreshable {
                await viewModel.loadInitial(project: project)
            }
            .navigationTitle("Memories")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: { showFilter = true }) {
                        Image(systemName: "line.3.horizontal.decrease.circle")
                    }
                }
            }
            .navigationDestination(for: String.self) { memoryId in
                MemoryDetailView(memoryId: memoryId, connection: connection)
            }
            .sheet(isPresented: $showFilter) {
                MemoryFilterSheet(
                    selectedTypes: $viewModel.selectedTypes,
                    selectedStatus: $viewModel.selectedStatus
                )
                .presentationDetents([.medium])
            }
            .task {
                await viewModel.loadInitial(project: project)
            }
        }
    }
}
