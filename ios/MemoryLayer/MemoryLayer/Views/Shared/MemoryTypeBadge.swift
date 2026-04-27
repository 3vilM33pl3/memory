import SwiftUI

struct MemoryTypeBadge: View {
    let type: MemoryType

    var body: some View {
        Label(type.displayName, systemImage: type.icon)
            .font(.caption2)
            .fontWeight(.medium)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(type.color.opacity(0.15))
            .foregroundStyle(type.color)
            .clipShape(Capsule())
    }
}
