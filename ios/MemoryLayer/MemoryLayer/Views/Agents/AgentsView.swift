import SwiftUI

struct AgentsView: View {
    let connection: ConnectionManager
    @State private var snapshot: AgentSnapshotResponse?
    @State private var isLoading = true
    @State private var error: String?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                if isLoading {
                    ProgressView()
                        .frame(maxWidth: .infinity, minHeight: 200)
                } else if let snapshot, !snapshot.sessions.isEmpty {
                    LazyVStack(spacing: Theme.Spacing.md) {
                        ForEach(snapshot.sessions) { session in
                            agentCard(session)
                        }
                    }
                    .padding(Theme.Spacing.lg)
                } else {
                    EmptyStateView(
                        icon: "person.2",
                        title: "No Agents",
                        message: "No active agent sessions."
                    )
                }
            }
            .navigationTitle("Agents")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .task {
                while !Task.isCancelled {
                    await refresh()
                    try? await Task.sleep(for: .seconds(AppConstants.agentPollInterval))
                }
            }
        }
    }

    private func refresh() async {
        do {
            snapshot = try await connection.api.getAgents()
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    private func agentCard(_ session: AgentSessionResponse) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
            HStack {
                Text(session.agentCli)
                    .font(.headline)

                Text(session.model)
                    .font(.caption)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.purple.opacity(0.15))
                    .clipShape(Capsule())

                Spacer()

                statusPill(session.status)
            }

            // Context usage ring
            HStack(spacing: Theme.Spacing.md) {
                ZStack {
                    Circle()
                        .stroke(Color.blue.opacity(0.15), lineWidth: 6)
                    Circle()
                        .trim(from: 0, to: session.contextPercent / 100)
                        .stroke(Color.blue, style: StrokeStyle(lineWidth: 6, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                    Text("\(Int(session.contextPercent))%")
                        .font(.caption2)
                        .fontWeight(.medium)
                }
                .frame(width: 44, height: 44)

                VStack(alignment: .leading, spacing: 2) {
                    Text("Tokens: \(formatNumber(session.totalInputTokens + session.totalOutputTokens))")
                        .font(.caption)
                    Text("Turns: \(session.turnCount)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text("Branch: \(session.gitBranch)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            if !session.currentTasks.isEmpty {
                Text(session.currentTasks.joined(separator: ", "))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .padding(Theme.Spacing.md)
        .background(Theme.Colors.secondaryBackground)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
    }

    private func statusPill(_ status: AgentStatus) -> some View {
        let (text, color): (String, Color) = switch status {
        case .working: ("Working", .green)
        case .waiting: ("Waiting", .orange)
        case .done: ("Done", .secondary)
        }
        return Text(text)
            .font(.caption2)
            .fontWeight(.medium)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(color.opacity(0.15))
            .foregroundStyle(color)
            .clipShape(Capsule())
    }

    private func formatNumber(_ n: Int) -> String {
        if n >= 1_000_000 { return String(format: "%.1fM", Double(n) / 1_000_000) }
        if n >= 1_000 { return String(format: "%.1fK", Double(n) / 1_000) }
        return "\(n)"
    }
}
