import Foundation

enum WebSocketState: Equatable {
    case disconnected
    case connecting
    case connected
    case reconnecting(attempt: Int)
}

@Observable
final class WebSocketManager {
    var state: WebSocketState = .disconnected

    private var task: URLSessionWebSocketTask?
    private var session: URLSession
    private var url: URL?
    private var token: String?
    private var pingTask: Task<Void, Never>?
    private var receiveTask: Task<Void, Never>?
    private var reconnectTask: Task<Void, Never>?
    private var reconnectAttempt = 0
    private let maxReconnectDelay: TimeInterval = 30

    private var activityContinuations: [UUID: AsyncStream<ActivityEvent>.Continuation] = [:]
    private var projectChangeContinuations: [UUID: AsyncStream<(ProjectOverviewResponse, ProjectMemoriesResponse)>.Continuation] = [:]

    init() {
        self.session = URLSession(configuration: .default)
    }

    func configure(url: URL, token: String?) {
        self.url = url
        self.token = token
    }

    func connect() {
        guard let url else { return }
        disconnect()
        state = .connecting

        var request = URLRequest(url: url)
        if let token {
            request.setValue(token, forHTTPHeaderField: "x-api-token")
        }

        task = session.webSocketTask(with: request)
        task?.resume()

        state = .connected
        reconnectAttempt = 0
        startReceiving()
        startPinging()
    }

    func disconnect() {
        pingTask?.cancel()
        receiveTask?.cancel()
        reconnectTask?.cancel()
        task?.cancel(with: .normalClosure, reason: nil)
        task = nil
        state = .disconnected
    }

    func send(_ request: StreamRequest) {
        guard let task, state == .connected else { return }
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        guard let data = try? encoder.encode(request),
              let string = String(data: data, encoding: .utf8) else { return }

        task.send(.string(string)) { [weak self] error in
            if error != nil {
                self?.handleDisconnect()
            }
        }
    }

    func activityStream() -> AsyncStream<ActivityEvent> {
        let id = UUID()
        return AsyncStream { continuation in
            activityContinuations[id] = continuation
            continuation.onTermination = { [weak self] _ in
                self?.activityContinuations.removeValue(forKey: id)
            }
        }
    }

    func projectChangeStream() -> AsyncStream<(ProjectOverviewResponse, ProjectMemoriesResponse)> {
        let id = UUID()
        return AsyncStream { continuation in
            projectChangeContinuations[id] = continuation
            continuation.onTermination = { [weak self] _ in
                self?.projectChangeContinuations.removeValue(forKey: id)
            }
        }
    }

    private func startReceiving() {
        receiveTask?.cancel()
        receiveTask = Task { [weak self] in
            guard let self else { return }
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase

            while !Task.isCancelled {
                guard let task = self.task else { break }
                do {
                    let message = try await task.receive()
                    switch message {
                    case .string(let text):
                        guard let data = text.data(using: .utf8) else { continue }
                        if let response = try? decoder.decode(StreamResponse.self, from: data) {
                            self.handleResponse(response)
                        }
                    case .data(let data):
                        if let response = try? decoder.decode(StreamResponse.self, from: data) {
                            self.handleResponse(response)
                        }
                    @unknown default:
                        break
                    }
                } catch {
                    if !Task.isCancelled {
                        self.handleDisconnect()
                    }
                    break
                }
            }
        }
    }

    private func handleResponse(_ response: StreamResponse) {
        switch response {
        case .activity(let event):
            for cont in activityContinuations.values {
                cont.yield(event)
            }
        case .projectChanged(let overview, let memories),
             .projectSnapshot(let overview, let memories):
            for cont in projectChangeContinuations.values {
                cont.yield((overview, memories))
            }
        case .pong:
            break
        case .error(let message):
            print("WebSocket error: \(message)")
        default:
            break
        }
    }

    private func startPinging() {
        pingTask?.cancel()
        pingTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(AppConstants.pingInterval))
                self?.send(.ping)
            }
        }
    }

    private func handleDisconnect() {
        guard state != .disconnected else { return }
        task?.cancel(with: .abnormalClosure, reason: nil)
        task = nil

        reconnectAttempt += 1
        state = .reconnecting(attempt: reconnectAttempt)

        reconnectTask?.cancel()
        reconnectTask = Task { [weak self] in
            guard let self else { return }
            let delay = min(pow(2.0, Double(self.reconnectAttempt - 1)), self.maxReconnectDelay)
            try? await Task.sleep(for: .seconds(delay))
            if !Task.isCancelled {
                self.connect()
            }
        }
    }
}
