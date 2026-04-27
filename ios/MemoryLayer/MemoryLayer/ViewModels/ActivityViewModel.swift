import Foundation

@Observable
final class ActivityViewModel {
    var events: [ActivityEvent] = []
    var isLoading = false
    var error: String?
    var filterKind: ActivityKind?

    private var streamTask: Task<Void, Never>?
    private let connection: ConnectionManager

    init(connection: ConnectionManager) {
        self.connection = connection
    }

    var filteredEvents: [ActivityEvent] {
        guard let kind = filterKind else { return events }
        return events.filter { $0.kind == kind }
    }

    func loadInitial(project: String) async {
        isLoading = true
        do {
            events = try await connection.api.getActivities(project: project)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    func subscribeToStream(project: String) {
        streamTask?.cancel()
        streamTask = Task { [weak self] in
            guard let self else { return }
            self.connection.ws.send(.subscribeProject(project: project))
            for await event in self.connection.ws.activityStream() {
                if !Task.isCancelled {
                    self.events.insert(event, at: 0)
                }
            }
        }
    }

    func unsubscribe() {
        streamTask?.cancel()
    }
}
