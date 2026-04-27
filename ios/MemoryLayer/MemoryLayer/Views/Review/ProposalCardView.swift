import SwiftUI

struct ProposalCardView: View {
    let proposal: ReplacementProposalRecord

    var body: some View {
        VStack(spacing: 0) {
            // Current memory
            VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
                HStack {
                    Text("Current")
                        .font(.caption)
                        .fontWeight(.bold)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                Text(proposal.targetSummary)
                    .font(.subheadline)
            }
            .padding(Theme.Spacing.md)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.red.opacity(0.05))

            Divider()

            // Proposed replacement
            VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
                HStack {
                    Text("Proposed")
                        .font(.caption)
                        .fontWeight(.bold)
                        .foregroundStyle(.green)
                    Spacer()
                    MemoryTypeBadge(type: proposal.candidateMemoryType)
                }
                Text(proposal.candidateSummary)
                    .font(.subheadline)
                Text(proposal.candidateCanonicalText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(4)
            }
            .padding(Theme.Spacing.md)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.green.opacity(0.05))

            Divider()

            // Score + reasons
            VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                HStack {
                    Text("Score: \(String(format: "%.0f%%", proposal.score * 100))")
                        .font(.caption)
                        .fontWeight(.medium)
                    Spacer()
                    Text(proposal.policy)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                ForEach(proposal.reasons, id: \.self) { reason in
                    HStack(alignment: .top, spacing: 4) {
                        Text("*")
                            .font(.caption)
                        Text(reason)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding(Theme.Spacing.md)
        }
        .background(Theme.Colors.secondaryBackground)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.lg))
        .shadow(color: .black.opacity(0.1), radius: 8, y: 4)
    }
}
