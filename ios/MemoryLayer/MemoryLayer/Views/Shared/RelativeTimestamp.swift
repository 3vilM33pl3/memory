import SwiftUI

struct RelativeTimestamp: View {
    let dateString: String

    var body: some View {
        Text(DateFormatting.relativeString(from: dateString))
            .font(.caption)
            .foregroundStyle(.secondary)
    }
}
