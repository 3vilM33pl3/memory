import Foundation
import Network

enum ConnectionState: Equatable {
    case disconnected
    case connecting
    case connected
    case error(String)

    static func == (lhs: ConnectionState, rhs: ConnectionState) -> Bool {
        switch (lhs, rhs) {
        case (.disconnected, .disconnected),
             (.connecting, .connecting),
             (.connected, .connected):
            return true
        case (.error(let a), .error(let b)):
            return a == b
        default:
            return false
        }
    }
}

@Observable
final class ConnectionManager {
    var state: ConnectionState = .disconnected
    var isNetworkAvailable = true

    let api: APIClient
    let ws: WebSocketManager

    private let monitor = NWPathMonitor()
    private let monitorQueue = DispatchQueue(label: "com.memorylayer.network")

    init() {
        self.api = APIClient()
        self.ws = WebSocketManager()

        monitor.pathUpdateHandler = { [weak self] path in
            Task { @MainActor in
                self?.isNetworkAvailable = path.status == .satisfied
                if path.status == .satisfied && self?.state == .disconnected {
                    self?.reconnect()
                }
            }
        }
        monitor.start(queue: monitorQueue)
    }

    func configure(baseURL: String, token: String?) async {
        await api.configure(baseURL: baseURL, token: token)
        if let wsURL = URL(string: baseURL.replacingOccurrences(of: "http", with: "ws") + "/ws") {
            ws.configure(url: wsURL, token: token)
        }
    }

    func connect() async {
        state = .connecting
        do {
            _ = try await api.healthCheck()
            state = .connected
            ws.connect()
        } catch {
            state = .error(error.localizedDescription)
        }
    }

    func disconnect() {
        ws.disconnect()
        state = .disconnected
    }

    private func reconnect() {
        Task {
            await connect()
        }
    }

    deinit {
        monitor.cancel()
    }
}
