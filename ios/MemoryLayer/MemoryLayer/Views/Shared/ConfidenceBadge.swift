import SwiftUI

struct ConfidenceBadge: View {
    let confidence: Double

    private var label: String {
        if confidence >= 0.7 { return "High" }
        if confidence >= 0.4 { return "Medium" }
        return "Low"
    }

    var body: some View {
        Text(label)
            .font(.caption2)
            .fontWeight(.medium)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Theme.Colors.confidence(confidence).opacity(0.2))
            .foregroundStyle(Theme.Colors.confidence(confidence))
            .clipShape(Capsule())
    }
}
