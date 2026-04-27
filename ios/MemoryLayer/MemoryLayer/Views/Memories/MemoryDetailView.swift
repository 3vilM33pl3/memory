import SwiftUI

struct MemoryDetailView: View {
    let memoryId: String
    let connection: ConnectionManager

    @State private var memory: MemoryEntryResponse?
    @State private var isLoading = true
    @State private var error: String?

    var body: some View {
        ScrollView {
            if isLoading {
                ProgressView()
                    .frame(maxWidth: .infinity, minHeight: 300)
            } else if let memory {
                VStack(alignment: .leading, spacing: Theme.Spacing.lg) {
                    // Header
                    HStack {
                        MemoryTypeBadge(type: memory.memoryType)
                        ConfidenceBadge(confidence: memory.confidence)
                        Spacer()
                        Text(memory.status.rawValue.capitalized)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    // Summary
                    Text(memory.summary)
                        .font(.headline)

                    // Canonical text
                    Text(memory.canonicalText)
                        .font(.body)
                        .padding(Theme.Spacing.md)
                        .background(Theme.Colors.secondaryBackground)
                        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.sm))

                    // Tags
                    if !memory.tags.isEmpty {
                        VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                            Text("Tags")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            TagCloudView(tags: memory.tags)
                        }
                    }

                    // Sources
                    if !memory.sources.isEmpty {
                        VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
                            Text("Sources")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            ForEach(memory.sources) { source in
                                HStack {
                                    Image(systemName: sourceIcon(source.sourceKind))
                                        .foregroundStyle(.secondary)
                                    VStack(alignment: .leading) {
                                        Text(source.sourceKind.rawValue)
                                            .font(.caption)
                                            .fontWeight(.medium)
                                        if let path = source.filePath {
                                            Text(path)
                                                .font(.caption2)
                                                .foregroundStyle(.secondary)
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Related memories
                    if !memory.relatedMemories.isEmpty {
                        VStack(alignment: .leading, spacing: Theme.Spacing.sm) {
                            Text("Related Memories")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            ForEach(memory.relatedMemories) { related in
                                NavigationLink(value: related.memoryId) {
                                    HStack {
                                        MemoryTypeBadge(type: related.memoryType)
                                        Text(related.summary)
                                            .font(.caption)
                                            .lineLimit(1)
                                    }
                                }
                            }
                        }
                    }

                    // Metadata
                    VStack(alignment: .leading, spacing: Theme.Spacing.xs) {
                        Text("Details")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        metaRow("Created", DateFormatting.relativeString(from: memory.createdAt))
                        metaRow("Updated", DateFormatting.relativeString(from: memory.updatedAt))
                        metaRow("Version", "\(memory.versionNo)")
                        metaRow("Importance", String(format: "%.1f", memory.importance))
                    }
                }
                .padding(Theme.Spacing.lg)
            } else if let error {
                Text(error)
                    .foregroundStyle(.red)
                    .padding()
            }
        }
        .navigationTitle("Memory")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            await loadMemory()
        }
    }

    private func loadMemory() async {
        do {
            memory = try await connection.api.getMemory(id: memoryId)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    private func metaRow(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
            Spacer()
            Text(value)
                .font(.caption)
                .monospacedDigit()
        }
    }

    private func sourceIcon(_ kind: SourceKind) -> String {
        switch kind {
        case .file: return "doc"
        case .gitCommit: return "arrow.triangle.branch"
        case .taskPrompt: return "text.bubble"
        case .commandOutput: return "terminal"
        case .test: return "checkmark.circle"
        case .note: return "note.text"
        }
    }
}
