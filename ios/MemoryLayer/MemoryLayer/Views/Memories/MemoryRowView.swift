import SwiftUI

struct MemoryRowView: View {
    let memory: ProjectMemoryListItem

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
            HStack {
                MemoryTypeBadge(type: memory.memoryType)
                Spacer()
                ConfidenceBadge(confidence: memory.confidence)
            }

            Text(memory.summary)
                .font(.subheadline)
                .lineLimit(2)

            HStack {
                if !memory.tags.isEmpty {
                    HStack(spacing: 2) {
                        ForEach(memory.tags.prefix(3), id: \.self) { tag in
                            Text(tag)
                                .font(.caption2)
                                .padding(.horizontal, 4)
                                .padding(.vertical, 1)
                                .background(Color.secondary.opacity(0.12))
                                .clipShape(Capsule())
                        }
                        if memory.tags.count > 3 {
                            Text("+\(memory.tags.count - 3)")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                Spacer()
                RelativeTimestamp(dateString: memory.updatedAt)
            }

            // Confidence bar
            GeometryReader { geo in
                RoundedRectangle(cornerRadius: 1)
                    .fill(Theme.Colors.confidence(memory.confidence))
                    .frame(width: geo.size.width * memory.confidence, height: 2)
            }
            .frame(height: 2)
        }
        .padding(.vertical, Theme.Spacing.xs)
    }
}
