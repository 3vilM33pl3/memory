import SwiftUI

struct SettingsView: View {
    let connection: ConnectionManager
    @Binding var isConnected: Bool
    @Binding var project: String?

    @State private var serviceURL = KeychainHelper.serviceURL ?? ""
    @State private var apiToken = KeychainHelper.apiToken ?? ""
    @State private var testResult: String?
    @State private var isTesting = false
    @State private var serviceVersion: String?

    var body: some View {
        NavigationStack {
            List {
                Section("Connection") {
                    TextField("Service URL", text: $serviceURL)
                        .textContentType(.URL)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .keyboardType(.URL)

                    SecureField("API Token", text: $apiToken)

                    Button(action: testConnection) {
                        HStack {
                            Text("Test Connection")
                            Spacer()
                            if isTesting {
                                ProgressView()
                            } else if let result = testResult {
                                Image(systemName: result == "OK" ? "checkmark.circle.fill" : "xmark.circle.fill")
                                    .foregroundStyle(result == "OK" ? .green : .red)
                            }
                        }
                    }

                    Button("Save") {
                        KeychainHelper.serviceURL = serviceURL
                        KeychainHelper.apiToken = apiToken.isEmpty ? nil : apiToken
                        Task {
                            await connection.configure(baseURL: serviceURL, token: apiToken.isEmpty ? nil : apiToken)
                            await connection.connect()
                        }
                    }
                }

                Section("Project") {
                    HStack {
                        Text("Current")
                        Spacer()
                        Text(project ?? "None")
                            .foregroundStyle(.secondary)
                    }

                    TextField("Change project", text: Binding(
                        get: { project ?? "" },
                        set: { val in
                            if !val.isEmpty {
                                project = val
                                KeychainHelper.defaultProject = val
                            }
                        }
                    ))
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                }

                Section("About") {
                    HStack {
                        Text("App Version")
                        Spacer()
                        Text("1.0.0")
                            .foregroundStyle(.secondary)
                    }
                    if let version = serviceVersion {
                        HStack {
                            Text("Service")
                            Spacer()
                            Text(version)
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                Section {
                    Button("Disconnect", role: .destructive) {
                        connection.disconnect()
                        KeychainHelper.serviceURL = nil
                        KeychainHelper.apiToken = nil
                        KeychainHelper.defaultProject = nil
                        project = nil
                        withAnimation {
                            isConnected = false
                        }
                    }
                }
            }
            .navigationTitle("Settings")
            .task {
                await loadServiceVersion()
            }
        }
    }

    private func testConnection() {
        isTesting = true
        testResult = nil
        Task {
            await connection.configure(baseURL: serviceURL, token: apiToken.isEmpty ? nil : apiToken)
            do {
                _ = try await connection.api.healthCheck()
                testResult = "OK"
            } catch {
                testResult = error.localizedDescription
            }
            isTesting = false
        }
    }

    private func loadServiceVersion() async {
        do {
            let health = try await connection.api.healthCheck()
            if let version = health["version"]?.value as? String {
                serviceVersion = version
            }
        } catch {
            // Ignore
        }
    }
}
