import SwiftUI

struct ResumeView: View {
    let project: String
    let connection: ConnectionManager
    @State private var response: ResumeResponse?
    @State private var isLoading = true
    @State private var error: String?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                if isLoading {
                    ProgressView()
                        .frame(maxWidth: .infinity, minHeight: 300)
                } else if let response {
                    VStack(alignment: .leading, spacing: Theme.Spacing.lg) {
                        // Briefing
                        Text(response.briefing)
                            .font(.body)

                        // Attention items
                        if !response.attentionItems.isEmpty {
                            VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
                                Label("Attention", systemImage: "exclamationmark.triangle.fill")
                                    .font(.subheadline)
                                    .fontWeight(.bold)
                                    .foregroundStyle(.red)

                                ForEach(response.attentionItems, id: \.self) { item in
                                    Text(item)
                                        .font(.caption)
                                        .padding(Theme.Spacing.sm)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .background(Color.red.opacity(0.08))
                                        .overlay(
                                            RoundedRectangle(cornerRadius: Theme.Radius.sm)
                                                .stroke(Color.red.opacity(0.3), lineWidth: 1)
                                        )
                                        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.sm))
                                }
                            }
                        }

                        // Next steps
                        if let primary = response.primaryNextStep {
                            VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
                                Label("Next Steps", systemImage: "arrow.forward.circle")
                                    .font(.subheadline)
                                    .fontWeight(.bold)

                                actionRow(primary, isPrimary: true)

                                ForEach(response.secondaryNextSteps) { step in
                                    actionRow(step, isPrimary: false)
                                }
                            }
                        }

                        // Change summary
                        if !response.changeSummary.isEmpty {
                            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                                Text("Changes")
                                    .font(.subheadline)
                                    .fontWeight(.bold)
                                ForEach(response.changeSummary, id: \.self) { change in
                                    HStack(alignment: .top, spacing: 6) {
                                        Circle()
                                            .fill(Color.secondary)
                                            .frame(width: 4, height: 4)
                                            .offset(y: 6)
                                        Text(change)
                                            .font(.caption)
                                    }
                                }
                            }
                        }

                        // Context memories
                        if !response.contextItems.isEmpty {
                            VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
                                Text("Context")
                                    .font(.subheadline)
                                    .fontWeight(.bold)
                                ForEach(response.contextItems) { item in
                                    HStack {
                                        MemoryTypeBadge(type: item.memoryType)
                                        Text(item.summary)
                                            .font(.caption)
                                            .lineLimit(1)
                                    }
                                }
                            }
                        }
                    }
                    .padding(Theme.Spacing.lg)
                } else if let error {
                    Text(error)
                        .foregroundStyle(.red)
                        .padding()
                }
            }
            .navigationTitle("Resume")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .task {
                await load()
            }
        }
    }

    private func load() async {
        do {
            response = try await connection.api.resume(project: project)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    private func actionRow(_ action: ResumeAction, isPrimary: Bool) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(action.title)
                .font(.caption)
                .fontWeight(isPrimary ? .bold : .medium)
            Text(action.rationale)
                .font(.caption2)
                .foregroundStyle(.secondary)
            if let hint = action.commandHint {
                Text(hint)
                    .font(.caption2)
                    .fontDesign(.monospaced)
                    .padding(4)
                    .background(Color.secondary.opacity(0.1))
                    .clipShape(RoundedRectangle(cornerRadius: 4))
                    .textSelection(.enabled)
            }
        }
        .padding(Theme.Spacing.sm)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isPrimary ? Color.accentColor.opacity(0.08) : Color.secondary.opacity(0.06))
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.sm))
    }
}
