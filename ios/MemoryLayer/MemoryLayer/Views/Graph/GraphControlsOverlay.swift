import SwiftUI

struct GraphControlsOverlay: View {
    @Bindable var viewModel: GraphViewModel
    let project: String
    let connection: ConnectionManager
    var onSearch: (String) async -> Void

    var body: some View {
        VStack {
            // Search bar at top
            HStack {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("Search code graph...", text: $viewModel.searchText)
                    .textFieldStyle(.plain)
                    .onSubmit {
                        Task { await onSearch(viewModel.searchText) }
                    }
                if !viewModel.searchText.isEmpty {
                    Button {
                        viewModel.searchText = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding(10)
            .background(.ultraThinMaterial)
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .padding(.horizontal)

            Spacer()

            // Bottom controls
            HStack(alignment: .bottom) {
                // Legend
                VStack(alignment: .leading, spacing: 4) {
                    Text("Legend")
                        .font(.caption.bold())
                    ForEach(MemoryType.allCases) { type in
                        HStack(spacing: 6) {
                            Circle()
                                .fill(type.color)
                                .frame(width: 8, height: 8)
                            Text(type.displayName)
                                .font(.caption2)
                        }
                    }
                    HStack(spacing: 6) {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(.gray)
                            .frame(width: 8, height: 8)
                        Text("Code Symbol")
                            .font(.caption2)
                    }
                }
                .padding(10)
                .background(.ultraThinMaterial)
                .clipShape(RoundedRectangle(cornerRadius: 10))

                Spacer()

                // Stats and controls
                VStack(alignment: .trailing, spacing: 8) {
                    Text("\(viewModel.nodeCount) nodes, \(viewModel.edgeCount) edges")
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    Button {
                        viewModel.toggleSimulation()
                    } label: {
                        Image(systemName: viewModel.isSimulating ? "pause.circle.fill" : "play.circle.fill")
                            .font(.title2)
                    }
                }
                .padding(10)
                .background(.ultraThinMaterial)
                .clipShape(RoundedRectangle(cornerRadius: 10))
            }
            .padding(.horizontal)
            .padding(.bottom, 8)
        }
    }
}
