import SwiftUI

struct MemoryFilterSheet: View {
    @Binding var selectedTypes: Set<MemoryType>
    @Binding var selectedStatus: MemoryStatus?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section("Status") {
                    Button(action: { selectedStatus = nil }) {
                        HStack {
                            Text("All")
                            Spacer()
                            if selectedStatus == nil {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(.tint)
                            }
                        }
                    }
                    ForEach(MemoryStatus.allCases) { status in
                        Button(action: { selectedStatus = status }) {
                            HStack {
                                Text(status.rawValue.capitalized)
                                Spacer()
                                if selectedStatus == status {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(.tint)
                                }
                            }
                        }
                    }
                }

                Section("Memory Type") {
                    ForEach(MemoryType.allCases) { type in
                        Button(action: {
                            if selectedTypes.contains(type) {
                                selectedTypes.remove(type)
                            } else {
                                selectedTypes.insert(type)
                            }
                        }) {
                            HStack {
                                MemoryTypeBadge(type: type)
                                Spacer()
                                if selectedTypes.contains(type) {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(.tint)
                                }
                            }
                        }
                    }
                }
            }
            .navigationTitle("Filters")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Reset") {
                        selectedTypes = []
                        selectedStatus = nil
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}
