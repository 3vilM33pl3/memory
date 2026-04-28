import SwiftUI
import RealityKit

struct GraphRealityView: View {
    @Bindable var viewModel: GraphViewModel
    @State private var yaw: Float = 0
    @State private var pitch: Float = 0
    @State private var zoomLevel: Float = 1.0
    @State private var lastDragYaw: Float = 0
    @State private var lastDragPitch: Float = 0

    var body: some View {
        GeometryReader { geometry in
            ZStack {
                if #available(iOS 18.0, *) {
                    realityContent
                } else {
                    canvas2DFallback(size: geometry.size)
                }

                // Gesture overlay
                Color.clear
                    .contentShape(Rectangle())
                    .onTapGesture { location in
                        selectNode(at: location, in: geometry.size)
                    }
                    .gesture(
                        DragGesture()
                            .onChanged { value in
                                let dx = Float(value.translation.width) * 0.005
                                let dy = Float(value.translation.height) * 0.005
                                yaw = lastDragYaw + dx
                                pitch = lastDragPitch + dy
                            }
                            .onEnded { _ in
                                lastDragYaw = yaw
                                lastDragPitch = pitch
                            }
                    )
                    .gesture(
                        MagnificationGesture()
                            .onChanged { value in
                                zoomLevel = max(0.2, min(5.0, Float(value)))
                            }
                    )
            }
        }
    }

    // MARK: - RealityKit (iOS 18+)

    @available(iOS 18.0, *)
    private var realityContent: some View {
        RealityView { content in
            let root = Entity()
            root.name = "root"
            content.add(root)
        } update: { content in
            guard let root = content.entities.first(where: { $0.name == "root" }) else { return }

            let yawQuat = simd_quatf(angle: yaw, axis: SIMD3(0, 1, 0))
            let pitchQuat = simd_quatf(angle: pitch, axis: SIMD3(1, 0, 0))
            root.transform.rotation = yawQuat * pitchQuat
            root.transform.scale = SIMD3(repeating: zoomLevel)

            reconcileNodes(root: root, yawQuat: yawQuat, pitchQuat: pitchQuat)
            reconcileEdges(root: root)
            reconcileLabels(root: root, yawQuat: yawQuat, pitchQuat: pitchQuat)
        }
    }

    // MARK: - Canvas 2D Fallback (iOS 17)

    private func canvas2DFallback(size: CGSize) -> some View {
        Canvas { context, canvasSize in
            let centerX = canvasSize.width / 2
            let centerY = canvasSize.height / 2
            let scale = min(canvasSize.width, canvasSize.height) * 0.1 * CGFloat(zoomLevel)

            let yawQuat = simd_quatf(angle: yaw, axis: SIMD3(0, 1, 0))
            let pitchQuat = simd_quatf(angle: pitch, axis: SIMD3(1, 0, 0))
            let rotation = yawQuat * pitchQuat

            let nodePositions = Dictionary(uniqueKeysWithValues: viewModel.nodes.map { node -> (String, CGPoint) in
                let rotated = rotation.act(node.position)
                let x = centerX + CGFloat(rotated.x) * scale
                let y = centerY - CGFloat(rotated.y) * scale
                return (node.id, CGPoint(x: x, y: y))
            })

            // Draw edges
            for edge in viewModel.edges {
                guard let src = nodePositions[edge.sourceId],
                      let tgt = nodePositions[edge.targetId] else { continue }
                var path = Path()
                path.move(to: src)
                path.addLine(to: tgt)
                context.stroke(path, with: .color(.gray.opacity(0.3)), lineWidth: 1)
            }

            // Draw nodes
            for node in viewModel.nodes {
                guard let pos = nodePositions[node.id] else { continue }
                let radius = CGFloat(3 + node.importance * 8)
                let rect = CGRect(x: pos.x - radius, y: pos.y - radius, width: radius * 2, height: radius * 2)
                let isSelected = node.id == viewModel.selectedNodeId

                let color: Color = node.nodeKind == .codeSymbol ? .gray : (node.memoryType?.color ?? .gray)

                if node.nodeKind == .codeSymbol {
                    context.fill(Path(rect), with: .color(color.opacity(isSelected ? 1.0 : 0.8)))
                } else {
                    context.fill(Path(ellipseIn: rect), with: .color(color.opacity(isSelected ? 1.0 : 0.8)))
                }

                if isSelected {
                    let strokeRect = rect.insetBy(dx: -2, dy: -2)
                    context.stroke(Path(ellipseIn: strokeRect), with: .color(.white), lineWidth: 2)
                }
            }
        }
    }

    // MARK: - RealityKit Helpers

    @available(iOS 18.0, *)
    private func reconcileNodes(root: Entity, yawQuat: simd_quatf, pitchQuat: simd_quatf) {
        let existingNodeNames = Set(root.children.filter { $0.name.hasPrefix("node-") }.map(\.name))
        let currentNodeNames = Set(viewModel.nodes.map { "node-\($0.id)" })

        for name in existingNodeNames.subtracting(currentNodeNames) {
            if let entity = root.children.first(where: { $0.name == name }) {
                entity.removeFromParent()
            }
        }

        for node in viewModel.nodes {
            let entityName = "node-\(node.id)"
            let isSelected = node.id == viewModel.selectedNodeId

            if let existing = root.children.first(where: { $0.name == entityName }) {
                existing.position = node.position
                if let model = existing as? ModelEntity {
                    model.scale = SIMD3(repeating: isSelected ? 1.5 : 1.0)
                }
            } else {
                let radius: Float = 0.05 + Float(node.importance) * 0.15
                let entity: ModelEntity

                if node.nodeKind == .codeSymbol {
                    let mesh = MeshResource.generateBox(size: radius * 2)
                    var material = SimpleMaterial()
                    material.color = .init(tint: .lightGray)
                    material.metallic = .init(floatLiteral: 0.3)
                    entity = ModelEntity(mesh: mesh, materials: [material])
                } else {
                    let mesh = MeshResource.generateSphere(radius: radius)
                    let color = node.memoryType?.uiColor ?? .gray
                    var material = SimpleMaterial()
                    material.color = .init(tint: color)
                    material.metallic = .init(floatLiteral: 0)
                    entity = ModelEntity(mesh: mesh, materials: [material])
                }

                entity.name = entityName
                entity.position = node.position
                root.addChild(entity)
            }
        }
    }

    @available(iOS 18.0, *)
    private func reconcileEdges(root: Entity) {
        let existingEdgeNames = Set(root.children.filter { $0.name.hasPrefix("edge-") }.map(\.name))
        let currentEdgeNames = Set(viewModel.edges.map { "edge-\($0.id)" })

        for name in existingEdgeNames.subtracting(currentEdgeNames) {
            if let entity = root.children.first(where: { $0.name == name }) {
                entity.removeFromParent()
            }
        }

        let nodePositions = Dictionary(uniqueKeysWithValues: viewModel.nodes.map { ($0.id, $0.position) })

        for edge in viewModel.edges {
            let entityName = "edge-\(edge.id)"
            guard let srcPos = nodePositions[edge.sourceId],
                  let tgtPos = nodePositions[edge.targetId] else { continue }

            let delta = tgtPos - srcPos
            let dist = simd_length(delta)
            guard dist > 0.01 else { continue }

            let midpoint = (srcPos + tgtPos) / 2
            let direction = simd_normalize(delta)
            let up = SIMD3<Float>(0, 1, 0)
            let rotation = simd_quatf(from: up, to: direction)

            if let existing = root.children.first(where: { $0.name == entityName }) {
                existing.position = midpoint
                existing.transform.rotation = rotation
                existing.scale = SIMD3(1, dist, 1)
            } else {
                let mesh = MeshResource.generateCylinder(height: 1.0, radius: 0.005)
                var material = SimpleMaterial()
                material.color = .init(tint: .gray.withAlphaComponent(0.4))
                material.metallic = .init(floatLiteral: 0)
                let entity = ModelEntity(mesh: mesh, materials: [material])
                entity.name = entityName
                entity.position = midpoint
                entity.transform.rotation = rotation
                entity.scale = SIMD3(1, dist, 1)
                root.addChild(entity)
            }
        }
    }

    @available(iOS 18.0, *)
    private func reconcileLabels(root: Entity, yawQuat: simd_quatf, pitchQuat: simd_quatf) {
        let oldLabels = root.children.filter { $0.name.hasPrefix("label-") }
        for label in oldLabels { label.removeFromParent() }

        guard let selectedId = viewModel.selectedNodeId else { return }

        let neighborIds = Set(
            viewModel.edges.compactMap { edge -> String? in
                if edge.sourceId == selectedId { return edge.targetId }
                if edge.targetId == selectedId { return edge.sourceId }
                return nil
            }
        )

        let labelNodes = viewModel.nodes.filter { $0.id == selectedId || neighborIds.contains($0.id) }
        for node in labelNodes.prefix(10) {
            let truncated = String(node.label.prefix(30))
            let textMesh = MeshResource.generateText(
                truncated,
                extrusionDepth: 0.001,
                font: .systemFont(ofSize: 0.06),
                containerFrame: .zero,
                alignment: .center,
                lineBreakMode: .byTruncatingTail
            )
            var mat = SimpleMaterial()
            mat.color = .init(tint: .white)
            mat.metallic = .init(floatLiteral: 0)
            let labelEntity = ModelEntity(mesh: textMesh, materials: [mat])
            labelEntity.name = "label-\(node.id)"
            labelEntity.position = node.position + SIMD3(0, 0.15, 0)
            labelEntity.transform.rotation = (yawQuat * pitchQuat).inverse
            root.addChild(labelEntity)
        }
    }

    // MARK: - Hit Testing

    private func selectNode(at point: CGPoint, in size: CGSize) {
        let centerX = Float(size.width / 2)
        let centerY = Float(size.height / 2)
        let scale = min(Float(size.width), Float(size.height)) * 0.1 * zoomLevel

        let yawQuat = simd_quatf(angle: yaw, axis: SIMD3(0, 1, 0))
        let pitchQuat = simd_quatf(angle: pitch, axis: SIMD3(1, 0, 0))
        let rotation = yawQuat * pitchQuat

        var closestId: String?
        var closestDist: Float = 40

        for node in viewModel.nodes {
            let rotated = rotation.act(node.position)
            let screenX = centerX + rotated.x * scale
            let screenY = centerY - rotated.y * scale
            let dx = Float(point.x) - screenX
            let dy = Float(point.y) - screenY
            let dist = sqrt(dx * dx + dy * dy)
            if dist < closestDist {
                closestDist = dist
                closestId = node.id
            }
        }

        viewModel.selectedNodeId = closestId
    }
}
