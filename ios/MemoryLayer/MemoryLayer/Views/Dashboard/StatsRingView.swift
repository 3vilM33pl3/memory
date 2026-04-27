import SwiftUI

struct StatsRingView: View {
    let active: Int
    let archived: Int
    let recent7d: Int
    let total: Int

    @State private var animationProgress: CGFloat = 0

    private var maxVal: CGFloat {
        CGFloat(max(total, 1))
    }

    var body: some View {
        VStack(spacing: Theme.Spacing.md) {
            ZStack {
                ring(value: CGFloat(active), maxValue: maxVal, color: .green, width: 16)
                    .frame(width: 140, height: 140)

                ring(value: CGFloat(archived), maxValue: maxVal, color: .orange, width: 16)
                    .frame(width: 108, height: 108)

                ring(value: CGFloat(recent7d), maxValue: maxVal, color: .blue, width: 16)
                    .frame(width: 76, height: 76)

                Text("\(total)")
                    .font(.title2)
                    .fontWeight(.bold)
                    .monospacedDigit()
            }
            .padding(Theme.Spacing.md)

            HStack(spacing: Theme.Spacing.lg) {
                legendItem(color: .green, label: "Active", count: active)
                legendItem(color: .orange, label: "Archived", count: archived)
                legendItem(color: .blue, label: "7 days", count: recent7d)
            }
        }
        .padding(Theme.Spacing.md)
        .background(Theme.Colors.secondaryBackground)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
        .onAppear {
            withAnimation(.easeInOut(duration: AppConstants.Animation.ringDuration)) {
                animationProgress = 1
            }
        }
    }

    private func ring(value: CGFloat, maxValue: CGFloat, color: Color, width: CGFloat) -> some View {
        let fraction = min(value / maxValue, 1.0)
        return Circle()
            .stroke(color.opacity(0.15), lineWidth: width)
            .overlay(
                Circle()
                    .trim(from: 0, to: fraction * animationProgress)
                    .stroke(color, style: StrokeStyle(lineWidth: width, lineCap: .round))
                    .rotationEffect(.degrees(-90))
            )
    }

    private func legendItem(color: Color, label: String, count: Int) -> some View {
        HStack(spacing: 4) {
            Circle().fill(color).frame(width: 8, height: 8)
            Text("\(label): \(count)")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }
}
