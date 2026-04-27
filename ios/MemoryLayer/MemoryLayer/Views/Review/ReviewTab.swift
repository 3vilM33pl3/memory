import SwiftUI

struct ReviewTab: View {
    let project: String
    let connection: ConnectionManager
    @State private var viewModel: ReviewViewModel

    init(project: String, connection: ConnectionManager) {
        self.project = project
        self.connection = connection
        self._viewModel = State(initialValue: ReviewViewModel(connection: connection))
    }

    var body: some View {
        NavigationStack {
            VStack {
                if viewModel.isLoading {
                    ProgressView()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if viewModel.proposals.isEmpty {
                    EmptyStateView(
                        icon: "checkmark.seal",
                        title: "All Caught Up",
                        message: "No replacement proposals to review."
                    )
                } else {
                    VStack(spacing: Theme.Spacing.lg) {
                        Text("\(viewModel.proposals.count) proposals remaining")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        SwipeableCardStack(
                            proposals: $viewModel.proposals,
                            onApprove: { proposal in
                                viewModel.approve(project: project, proposal: proposal)
                            },
                            onReject: { proposal in
                                viewModel.reject(project: project, proposal: proposal)
                            }
                        )

                        // Explicit buttons for accessibility
                        if let top = viewModel.proposals.first {
                            HStack(spacing: Theme.Spacing.xxl) {
                                Button(action: {
                                    viewModel.reject(project: project, proposal: top)
                                }) {
                                    Image(systemName: "xmark.circle.fill")
                                        .font(.system(size: 50))
                                        .foregroundStyle(.red)
                                }

                                Button(action: {
                                    viewModel.approve(project: project, proposal: top)
                                }) {
                                    Image(systemName: "checkmark.circle.fill")
                                        .font(.system(size: 50))
                                        .foregroundStyle(.green)
                                }
                            }
                        }
                    }
                    .padding(Theme.Spacing.lg)
                }

                if let toast = viewModel.toastMessage {
                    Text(toast)
                        .font(.caption)
                        .padding(Theme.Spacing.sm)
                        .background(Color.red.opacity(0.15))
                        .clipShape(Capsule())
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }
            }
            .refreshable {
                await viewModel.load(project: project)
            }
            .navigationTitle("Review")
            .task {
                await viewModel.load(project: project)
            }
        }
    }
}
