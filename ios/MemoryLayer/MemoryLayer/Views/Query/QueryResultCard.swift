import SwiftUI

struct QueryResultCard: View {
    let result: QueryResult
    let index: Int

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
            HStack {
                Text("#\(index)")
                    .font(.caption)
                    .fontWeight(.bold)
                    .foregroundStyle(.white)
                    .frame(width: 24, height: 24)
                    .background(Color.blue)
                    .clipShape(Circle())

                MemoryTypeBadge(type: result.memoryType)

                Text(result.matchKind.rawValue)
                    .font(.caption2)
                    .padding(.horizontal, 4)
                    .padding(.vertical, 1)
                    .background(Color.secondary.opacity(0.12))
                    .clipShape(Capsule())

                Spacer()

                Text(String(format: "%.0f%%", result.score * 100))
                    .font(.caption)
                    .fontWeight(.medium)
                    .monospacedDigit()
            }

            Text(result.summary)
                .font(.subheadline)
                .fontWeight(.medium)

            Text(result.snippet)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(3)

            // Score bar
            GeometryReader { geo in
                RoundedRectangle(cornerRadius: 2)
                    .fill(Color.blue.opacity(0.2))
                    .overlay(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(Color.blue)
                            .frame(width: geo.size.width * min(result.score, 1.0))
                    }
            }
            .frame(height: 4)

            if !result.tags.isEmpty {
                HStack(spacing: 4) {
                    ForEach(result.tags.prefix(4), id: \.self) { tag in
                        Text(tag)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .padding(Theme.Spacing.md)
        .background(Theme.Colors.secondaryBackground)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
    }
}
