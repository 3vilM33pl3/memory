import SwiftUI
import UIKit

enum Theme {
    // MARK: - Spacing
    enum Spacing {
        static let xs: CGFloat = 4
        static let sm: CGFloat = 8
        static let md: CGFloat = 12
        static let lg: CGFloat = 16
        static let xl: CGFloat = 24
        static let xxl: CGFloat = 32
    }

    // MARK: - Colors
    enum Colors {
        static let background = Color(uiColor: .systemBackground)
        static let secondaryBackground = Color(uiColor: .secondarySystemBackground)
        static let tertiaryBackground = Color(uiColor: .tertiarySystemBackground)
        static let label = Color(uiColor: .label)
        static let secondaryLabel = Color(uiColor: .secondaryLabel)

        static let healthy = Color.green
        static let warning = Color.orange
        static let error = Color.red
        static let info = Color.blue

        // Confidence thresholds
        static func confidence(_ value: Double) -> Color {
            if value >= 0.7 { return .green }
            if value >= 0.4 { return .orange }
            return .red
        }
    }

    // MARK: - Corner Radius
    enum Radius {
        static let sm: CGFloat = 6
        static let md: CGFloat = 10
        static let lg: CGFloat = 16
    }
}
