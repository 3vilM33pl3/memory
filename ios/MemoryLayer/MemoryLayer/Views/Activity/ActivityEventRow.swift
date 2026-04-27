import SwiftUI

struct ActivityEventRow: View {
    let event: ActivityEvent
    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
            HStack(spacing: Theme.Spacing.sm) {
                Image(systemName: event.kind.icon)
                    .foregroundStyle(event.kind.color)
                    .frame(width: 24)

                VStack(alignment: .leading, spacing: 2) {
                    Text(event.summary)
                        .font(.subheadline)
                        .lineLimit(isExpanded ? nil : 2)

                    RelativeTimestamp(dateString: event.recordedAt)
                }

                Spacer()

                if event.details != nil {
                    Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            if isExpanded, let details = event.details {
                detailView(details)
                    .padding(.leading, 36)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            if event.details != nil {
                withAnimation(.easeInOut(duration: 0.2)) {
                    isExpanded.toggle()
                }
            }
        }
    }

    @ViewBuilder
    private func detailView(_ details: ActivityDetails) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            switch details {
            case .query(let d):
                Text("Query: \(d.query)")
                    .font(.caption)
                Text("Results: \(d.resultCount), \(d.totalDurationMs)ms")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            case .curate(let d):
                Text("Input: \(d.inputCount) -> Output: \(d.outputCount)")
                    .font(.caption)
                if d.proposalCount > 0 {
                    Text("Proposals: \(d.proposalCount)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            case .captureTask(let d):
                if let title = d.taskTitle {
                    Text(title)
                        .font(.caption)
                }
                Text("Writer: \(d.writerId)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            case .checkpoint(let d):
                if let note = d.note {
                    Text(note)
                        .font(.caption)
                }
                if let branch = d.gitBranch {
                    Text("Branch: \(branch)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            case .plan(let d):
                Text("\(d.title) (\(d.action))")
                    .font(.caption)
                Text("\(d.completedItems)/\(d.totalItems) complete")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            case .scan(let d):
                Text("Candidates: \(d.candidateCount), Files: \(d.filesConsidered)")
                    .font(.caption)
            case .memoryReplacement(let d):
                Text("Replaced: \(d.oldSummary)")
                    .font(.caption)
                Text("With: \(d.newSummary)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            case .archive(let d):
                Text("Archived: \(d.archivedCount)")
                    .font(.caption)
            case .reindex(let d):
                Text("Reindexed: \(d.reindexedEntries)")
                    .font(.caption)
            case .reembed(let d):
                Text("Reembedded: \(d.reembeddedChunks)")
                    .font(.caption)
            default:
                EmptyView()
            }
        }
        .padding(Theme.Spacing.sm)
        .background(Color.secondary.opacity(0.06))
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.sm))
    }
}
