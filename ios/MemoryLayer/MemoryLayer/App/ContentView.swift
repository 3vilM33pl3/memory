import SwiftUI

struct ContentView: View {
    let project: String
    let connection: ConnectionManager

    var body: some View {
        TabView {
            DashboardTab(project: project, connection: connection)
                .tabItem {
                    Label("Dashboard", systemImage: "gauge")
                }

            MemoriesTab(project: project, connection: connection)
                .tabItem {
                    Label("Memories", systemImage: "brain")
                }

            QueryTab(project: project, connection: connection)
                .tabItem {
                    Label("Query", systemImage: "magnifyingglass")
                }

            ReviewTab(project: project, connection: connection)
                .tabItem {
                    Label("Review", systemImage: "arrow.2.squarepath")
                }

            ActivityTab(project: project, connection: connection)
                .tabItem {
                    Label("Activity", systemImage: "clock")
                }
        }
    }
}
