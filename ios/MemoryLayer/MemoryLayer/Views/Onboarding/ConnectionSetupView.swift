import SwiftUI

struct ConnectionSetupView: View {
    @Binding var isConnected: Bool
    let connection: ConnectionManager

    @State private var serviceURL = "http://192.168.1.1:4040"
    @State private var apiToken = ""
    @State private var isTesting = false
    @State private var error: String?
    @State private var shake = false

    var body: some View {
        NavigationStack {
            VStack(spacing: Theme.Spacing.xl) {
                Spacer()

                Image(systemName: "brain.head.profile")
                    .font(.system(size: 60))
                    .foregroundStyle(.tint)

                Text("Memory Layer")
                    .font(.largeTitle)
                    .fontWeight(.bold)

                Text("Connect to your service")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                VStack(spacing: Theme.Spacing.md) {
                    TextField("Service URL", text: $serviceURL)
                        .textFieldStyle(.roundedBorder)
                        .textContentType(.URL)
                        .autocapitalization(.none)
                        .keyboardType(.URL)

                    SecureField("API Token (optional)", text: $apiToken)
                        .textFieldStyle(.roundedBorder)
                }
                .padding(.horizontal, Theme.Spacing.xl)

                if let error {
                    Text(error)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .modifier(ShakeEffect(animatableData: shake ? 1 : 0))
                }

                Button(action: testConnection) {
                    if isTesting {
                        ProgressView()
                            .frame(maxWidth: .infinity)
                    } else {
                        Text("Connect")
                            .frame(maxWidth: .infinity)
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(isTesting || serviceURL.isEmpty)
                .padding(.horizontal, Theme.Spacing.xl)

                Spacer()
                Spacer()
            }
            .navigationBarHidden(true)
        }
    }

    private func testConnection() {
        isTesting = true
        error = nil

        Task {
            await connection.configure(baseURL: serviceURL, token: apiToken.isEmpty ? nil : apiToken)
            await connection.connect()

            if connection.state == .connected {
                KeychainHelper.serviceURL = serviceURL
                KeychainHelper.apiToken = apiToken.isEmpty ? nil : apiToken
                withAnimation(.spring(duration: AppConstants.Animation.spring)) {
                    isConnected = true
                }
            } else if case .error(let msg) = connection.state {
                self.error = msg
                withAnimation(.default) { shake.toggle() }
            }
            isTesting = false
        }
    }
}

struct ShakeEffect: GeometryEffect {
    var animatableData: CGFloat

    func effectValue(size: CGSize) -> ProjectionTransform {
        let offset = sin(animatableData * .pi * 4) * 8
        return ProjectionTransform(CGAffineTransform(translationX: offset, y: 0))
    }
}
