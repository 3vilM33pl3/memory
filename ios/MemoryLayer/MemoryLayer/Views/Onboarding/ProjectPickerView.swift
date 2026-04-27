import SwiftUI

struct ProjectPickerView: View {
    @Binding var selectedProject: String?
    let connection: ConnectionManager

    @State private var projectSlug = ""
    @State private var isLoading = false
    @State private var detectedProject: String?

    var body: some View {
        NavigationStack {
            VStack(spacing: Theme.Spacing.xl) {
                Spacer()

                Image(systemName: "folder.badge.gearshape")
                    .font(.system(size: 50))
                    .foregroundStyle(.tint)

                Text("Choose Project")
                    .font(.title2)
                    .fontWeight(.bold)

                if let detected = detectedProject {
                    Text("Detected: \(detected)")
                        .foregroundStyle(.secondary)

                    Button("Use \(detected)") {
                        selectProject(detected)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                }

                VStack(spacing: Theme.Spacing.sm) {
                    TextField("Or enter project slug", text: $projectSlug)
                        .textFieldStyle(.roundedBorder)
                        .autocapitalization(.none)

                    Button("Connect") {
                        selectProject(projectSlug)
                    }
                    .buttonStyle(.bordered)
                    .disabled(projectSlug.isEmpty)
                }
                .padding(.horizontal, Theme.Spacing.xl)

                Spacer()
                Spacer()
            }
            .task {
                await detectProject()
            }
        }
    }

    private func detectProject() async {
        // Try to detect project from a known slug
        // The overview endpoint requires a project name, so we try common ones
        // In practice, the user will know their project name
    }

    private func selectProject(_ slug: String) {
        KeychainHelper.defaultProject = slug
        withAnimation(.spring(duration: AppConstants.Animation.spring)) {
            selectedProject = slug
        }
    }
}
