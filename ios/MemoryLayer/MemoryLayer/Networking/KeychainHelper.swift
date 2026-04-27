import Foundation
import Security

enum KeychainHelper {
    private static let service = "com.memorylayer.app"

    static func save(key: String, value: String) {
        guard let data = value.data(using: .utf8) else { return }
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        SecItemDelete(query as CFDictionary)
        var addQuery = query
        addQuery[kSecValueData as String] = data
        SecItemAdd(addQuery as CFDictionary, nil)
    }

    static func load(key: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static func delete(key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        SecItemDelete(query as CFDictionary)
    }

    // Convenience accessors
    static var serviceURL: String? {
        get { load(key: AppConstants.Keychain.serviceURL) }
        set {
            if let newValue { save(key: AppConstants.Keychain.serviceURL, value: newValue) }
            else { delete(key: AppConstants.Keychain.serviceURL) }
        }
    }

    static var apiToken: String? {
        get { load(key: AppConstants.Keychain.apiToken) }
        set {
            if let newValue { save(key: AppConstants.Keychain.apiToken, value: newValue) }
            else { delete(key: AppConstants.Keychain.apiToken) }
        }
    }

    static var defaultProject: String? {
        get { load(key: AppConstants.Keychain.defaultProject) }
        set {
            if let newValue { save(key: AppConstants.Keychain.defaultProject, value: newValue) }
            else { delete(key: AppConstants.Keychain.defaultProject) }
        }
    }
}
