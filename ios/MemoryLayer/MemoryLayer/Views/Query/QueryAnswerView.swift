import SwiftUI

struct QueryAnswerView: View {
    let response: QueryResponse

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
            HStack {
                Image(systemName: "sparkles")
                    .foregroundStyle(.tint)
                Text("Answer")
                    .font(.headline)
                Spacer()
                ConfidenceBadge(confidence: response.confidence)
            }

            if response.insufficientEvidence {
                HStack(spacing: Theme.Spacing.xs) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                    Text("Insufficient evidence")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            }

            Text(response.answer)
                .font(.body)

            // Citations
            if !response.answerCitations.isEmpty {
                HStack(spacing: 4) {
                    ForEach(response.answerCitations) { citation in
                        Text("\(citation.resultNumber)")
                            .font(.caption2)
                            .fontWeight(.bold)
                            .foregroundStyle(.white)
                            .frame(width: 20, height: 20)
                            .background(Color.blue)
                            .clipShape(Circle())
                    }
                }
            }

            HStack {
                Text("\(response.diagnostics.totalDurationMs)ms")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                Text(response.answerGeneration.method.rawValue)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(Theme.Spacing.md)
        .background(Theme.Colors.secondaryBackground)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
    }
}
