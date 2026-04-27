import Foundation

enum AppConstants {
    static let defaultPort = 4040
    static let requestTimeout: TimeInterval = 30
    static let pingInterval: TimeInterval = 30
    static let dashboardPollInterval: TimeInterval = 30
    static let agentPollInterval: TimeInterval = 10

    enum Animation {
        static let standard: Double = 0.3
        static let spring: Double = 0.5
        static let ringDuration: Double = 1.0
    }

    enum Keychain {
        static let serviceURL = "com.memorylayer.serviceURL"
        static let apiToken = "com.memorylayer.apiToken"
        static let defaultProject = "com.memorylayer.defaultProject"
    }
}
