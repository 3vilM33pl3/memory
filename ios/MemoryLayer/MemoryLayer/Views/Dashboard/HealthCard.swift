import SwiftUI

struct HealthCard: View {
    let serviceOK: Bool
    let databaseOK: Bool

    var body: some View {
        HStack(spacing: Theme.Spacing.lg) {
            statusPill(label: "Service", isOK: serviceOK)
            statusPill(label: "Database", isOK: databaseOK)
        }
        .padding(Theme.Spacing.md)
        .frame(maxWidth: .infinity)
        .background(Theme.Colors.secondaryBackground)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
    }

    private func statusPill(label: String, isOK: Bool) -> some View {
        HStack(spacing: Theme.Spacing.xs) {
            Circle()
                .fill(isOK ? Color.green : Color.red)
                .frame(width: 10, height: 10)
            Text(label)
                .font(.subheadline)
                .fontWeight(.medium)
        }
        .padding(.horizontal, Theme.Spacing.md)
        .padding(.vertical, Theme.Spacing.sm)
        .background(
            (isOK ? Color.green : Color.red).opacity(0.1)
        )
        .clipShape(Capsule())
    }
}
