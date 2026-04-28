import SwiftUI

enum AppSection: String, CaseIterable, Identifiable {
    case dashboard, memories, query, review, activity, graph

    var id: String { rawValue }

    var label: String {
        switch self {
        case .dashboard: return "Dashboard"
        case .memories: return "Memories"
        case .query: return "Query"
        case .review: return "Review"
        case .activity: return "Activity"
        case .graph: return "Graph"
        }
    }

    var systemImage: String {
        switch self {
        case .dashboard: return "gauge"
        case .memories: return "brain"
        case .query: return "magnifyingglass"
        case .review: return "arrow.2.squarepath"
        case .activity: return "clock"
        case .graph: return "point.3.connected.trianglepath.dotted"
        }
    }
}

struct ContentView: View {
    let project: String
    let connection: ConnectionManager
    @Binding var showSettings: Bool
    var isConnected: Binding<Bool>
    var projectBinding: Binding<String?>

    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var selectedSection: AppSection? = .dashboard

    var body: some View {
        if sizeClass == .regular {
            iPadLayout
        } else {
            iPhoneLayout
        }
    }

    // MARK: - iPad: Sidebar + Detail

    private var iPadLayout: some View {
        NavigationSplitView {
            List(selection: $selectedSection) {
                ForEach(AppSection.allCases) { section in
                    Label(section.label, systemImage: section.systemImage)
                        .tag(section)
                }
            }
            .navigationTitle("Memory Layer")
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button(action: { showSettings = true }) {
                        Image(systemName: "gearshape")
                    }
                }
            }
        } detail: {
            sectionView(for: selectedSection ?? .dashboard)
        }
        .sheet(isPresented: $showSettings) {
            SettingsView(
                connection: connection,
                isConnected: isConnected,
                project: projectBinding
            )
        }
    }

    // MARK: - iPhone: Tab Bar

    private var iPhoneLayout: some View {
        TabView(selection: $selectedSection) {
            ForEach(AppSection.allCases) { section in
                sectionView(for: section)
                    .tabItem {
                        Label(section.label, systemImage: section.systemImage)
                    }
                    .tag(Optional(section))
            }
        }
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
                isConnected: isConnected,
                project: projectBinding
            )
        }
    }

    // MARK: - Section Content

    @ViewBuilder
    private func sectionView(for section: AppSection) -> some View {
        switch section {
        case .dashboard:
            DashboardTab(project: project, connection: connection)
        case .memories:
            MemoriesTab(project: project, connection: connection)
        case .query:
            QueryTab(project: project, connection: connection)
        case .review:
            ReviewTab(project: project, connection: connection)
        case .activity:
            ActivityTab(project: project, connection: connection)
        case .graph:
            GraphTab(project: project, connection: connection)
        }
    }
}
