import Foundation

@Observable
final class ReviewViewModel {
    var proposals: [ReplacementProposalRecord] = []
    var isLoading = false
    var error: String?
    var toastMessage: String?

    private let connection: ConnectionManager

    init(connection: ConnectionManager) {
        self.connection = connection
    }

    var pendingCount: Int { proposals.count }

    func load(project: String) async {
        isLoading = true
        do {
            let response = try await connection.api.getReplacementProposals(project: project)
            proposals = response.proposals
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    func approve(project: String, proposal: ReplacementProposalRecord) {
        proposals.removeAll { $0.id == proposal.id }
        HapticEngine.success()
        Task {
            do {
                _ = try await connection.api.approveProposal(project: project, id: proposal.id)
            } catch {
                proposals.insert(proposal, at: 0)
                toastMessage = "Approve failed: \(error.localizedDescription)"
                HapticEngine.error()
            }
        }
    }

    func reject(project: String, proposal: ReplacementProposalRecord) {
        proposals.removeAll { $0.id == proposal.id }
        HapticEngine.warning()
        Task {
            do {
                _ = try await connection.api.rejectProposal(project: project, id: proposal.id)
            } catch {
                proposals.insert(proposal, at: 0)
                toastMessage = "Reject failed: \(error.localizedDescription)"
                HapticEngine.error()
            }
        }
    }
}
