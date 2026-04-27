import SwiftUI

struct QueryTab: View {
    let project: String
    let connection: ConnectionManager
    @State private var viewModel: QueryViewModel

    init(project: String, connection: ConnectionManager) {
        self.project = project
        self.connection = connection
        self._viewModel = State(initialValue: QueryViewModel(connection: connection))
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Input area
                HStack(spacing: Theme.Spacing.sm) {
                    TextField("Ask a question...", text: $viewModel.queryText, axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .lineLimit(1...4)
                        .submitLabel(.search)
                        .onSubmit { runQuery() }

                    VoiceQueryButton(text: $viewModel.queryText)

                    Button(action: runQuery) {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.title2)
                    }
                    .disabled(viewModel.queryText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || viewModel.isLoading)
                }
                .padding(Theme.Spacing.md)

                Divider()

                // Results
                ScrollView {
                    VStack(spacing: Theme.Spacing.md) {
                        if viewModel.isLoading {
                            ProgressView("Searching...")
                                .padding(.top, Theme.Spacing.xxl)
                        } else if let response = viewModel.response {
                            QueryAnswerView(response: response)
                                .padding(.horizontal, Theme.Spacing.md)

                            ForEach(Array(response.results.enumerated()), id: \.element.id) { index, result in
                                QueryResultCard(result: result, index: index + 1)
                                    .padding(.horizontal, Theme.Spacing.md)
                            }
                        } else if let error = viewModel.error {
                            Text(error)
                                .foregroundStyle(.red)
                                .padding()
                        }
                    }
                    .padding(.top, Theme.Spacing.md)
                }
            }
            .navigationTitle("Query")
        }
    }

    private func runQuery() {
        Task { await viewModel.runQuery(project: project) }
    }
}
