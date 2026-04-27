import Foundation

@Observable
final class MemoriesViewModel {
    var memories: [ProjectMemoryListItem] = []
    var total = 0
    var isLoading = false
    var error: String?
    var searchText = ""
    var selectedTypes: Set<MemoryType> = []
    var selectedStatus: MemoryStatus? = nil

    private var currentPage = 1
    private let perPage = 30
    private var hasMore = true
    private let connection: ConnectionManager

    init(connection: ConnectionManager) {
        self.connection = connection
    }

    var filteredMemories: [ProjectMemoryListItem] {
        var result = memories
        if !searchText.isEmpty {
            let query = searchText.lowercased()
            result = result.filter {
                $0.summary.lowercased().contains(query) ||
                $0.tags.contains(where: { $0.lowercased().contains(query) })
            }
        }
        if !selectedTypes.isEmpty {
            result = result.filter { selectedTypes.contains($0.memoryType) }
        }
        if let status = selectedStatus {
            result = result.filter { $0.status == status }
        }
        return result
    }

    func loadInitial(project: String) async {
        currentPage = 1
        hasMore = true
        isLoading = true
        do {
            let response = try await connection.api.getMemories(
                project: project, status: selectedStatus, page: 1, perPage: perPage
            )
            memories = response.items
            total = response.total
            hasMore = response.items.count == perPage
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    func loadMore(project: String) async {
        guard hasMore, !isLoading else { return }
        currentPage += 1
        do {
            let response = try await connection.api.getMemories(
                project: project, status: selectedStatus, page: currentPage, perPage: perPage
            )
            memories.append(contentsOf: response.items)
            hasMore = response.items.count == perPage
        } catch {
            currentPage -= 1
        }
    }
}
