import SwiftUI

@main
struct MemoryLayerApp: App {
    @State private var connection = ConnectionManager()
    @State private var isConnected = false
    @State private var project: String? = KeychainHelper.defaultProject
    @State private var showSettings = false

    var body: some Scene {
        WindowGroup {
            Group {
                if isConnected, let project {
                    ContentView(project: project, connection: connection)
                        .toolbar {
                            ToolbarItem(placement: .navigationBarTrailing) {
                                Button(action: { showSettings = true }) {
                                    Image(systemName: "gearshape")
                                }
                            }
                        }
                        .sheet(isPresented: $showSettings) {
                            SettingsView(
                                connection: connection,
                                isConnected: $isConnected,
                                project: $project
                            )
                        }
                } else if isConnected && project == nil {
                    ProjectPickerView(
                        selectedProject: $project,
                        connection: connection
                    )
                } else {
                    ConnectionSetupView(
                        isConnected: $isConnected,
                        connection: connection
                    )
                }
            }
            .preferredColorScheme(.dark)
            .task {
                // Auto-connect if credentials are saved
                if let url = KeychainHelper.serviceURL {
                    await connection.configure(baseURL: url, token: KeychainHelper.apiToken)
                    await connection.connect()
                    if connection.state == .connected {
                        isConnected = true
                    }
                }
            }
        }
    }
}
