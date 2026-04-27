import Foundation

enum DateFormatting {
    private static let iso8601: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static let iso8601NoFrac: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    private static let relative: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .abbreviated
        return f
    }()

    static func parse(_ string: String) -> Date? {
        iso8601.date(from: string) ?? iso8601NoFrac.date(from: string)
    }

    static func relativeString(from string: String) -> String {
        guard let date = parse(string) else { return string }
        return relative.localizedString(for: date, relativeTo: Date())
    }
}
