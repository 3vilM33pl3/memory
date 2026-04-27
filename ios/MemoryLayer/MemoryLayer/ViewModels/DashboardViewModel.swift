import Foundation

@Observable
final class DashboardViewModel {
    var overview: ProjectOverviewResponse?
    var isLoading = false
    var error: String?

    private var pollTask: Task<Void, Never>?
    private let connection: ConnectionManager

    init(connection: ConnectionManager) {
        self.connection = connection
    }

    var isHealthy: Bool {
        overview?.serviceStatus == "ok"
    }

    var isDatabaseHealthy: Bool {
        overview?.databaseStatus == "ok"
    }

    var pendingProposals: Int {
        overview?.pendingReplacementProposals ?? 0
    }

    func startPolling(project: String) {
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            guard let self else { return }
            while !Task.isCancelled {
                await self.refresh(project: project)
                try? await Task.sleep(for: .seconds(AppConstants.dashboardPollInterval))
            }
        }
    }

    func stopPolling() {
        pollTask?.cancel()
    }

    func refresh(project: String) async {
        isLoading = overview == nil
        do {
            overview = try await connection.api.getOverview(project: project)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }
}
