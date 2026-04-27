import SwiftUI

struct QuickActionsGrid: View {
    let pendingProposals: Int
    var onResume: () -> Void
    var onAgents: () -> Void

    var body: some View {
        LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: Theme.Spacing.md) {
            actionCard(icon: "magnifyingglass", title: "Query", color: .blue)
            actionCard(icon: "arrow.clockwise", title: "Resume", color: .orange) {
                onResume()
            }
            actionCard(icon: "arrow.2.squarepath", title: "Review", color: .purple, badge: pendingProposals)
            actionCard(icon: "person.2", title: "Agents", color: .teal) {
                onAgents()
            }
        }
    }

    private func actionCard(
        icon: String,
        title: String,
        color: Color,
        badge: Int = 0,
        action: (() -> Void)? = nil
    ) -> some View {
        Button(action: { action?() }) {
            VStack(spacing: Theme.Spacing.sm) {
                ZStack(alignment: .topTrailing) {
                    Image(systemName: icon)
                        .font(.title2)
                        .foregroundStyle(color)
                        .frame(width: 44, height: 44)
                        .background(color.opacity(0.12))
                        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.sm))

                    if badge > 0 {
                        Text("\(badge)")
                            .font(.caption2)
                            .fontWeight(.bold)
                            .foregroundStyle(.white)
                            .padding(4)
                            .background(Color.red)
                            .clipShape(Circle())
                            .offset(x: 6, y: -6)
                    }
                }
                Text(title)
                    .font(.caption)
                    .fontWeight(.medium)
            }
            .frame(maxWidth: .infinity)
            .padding(Theme.Spacing.md)
            .background(Theme.Colors.secondaryBackground)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
        }
        .buttonStyle(.plain)
    }
}
