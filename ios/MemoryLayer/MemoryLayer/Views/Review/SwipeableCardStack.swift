import SwiftUI

struct SwipeableCardStack: View {
    @Binding var proposals: [ReplacementProposalRecord]
    let onApprove: (ReplacementProposalRecord) -> Void
    let onReject: (ReplacementProposalRecord) -> Void

    @State private var offset: CGSize = .zero
    @State private var overlayOpacity: Double = 0

    private let swipeThreshold: CGFloat = 100

    var body: some View {
        ZStack {
            // Background cards
            ForEach(Array(proposals.prefix(3).enumerated().reversed()), id: \.element.id) { index, proposal in
                if index > 0 {
                    ProposalCardView(proposal: proposal)
                        .scaleEffect(1.0 - CGFloat(index) * 0.05)
                        .offset(y: CGFloat(index) * 8)
                        .allowsHitTesting(false)
                }
            }

            // Top card with drag
            if let top = proposals.first {
                ProposalCardView(proposal: top)
                    .offset(offset)
                    .rotationEffect(.degrees(Double(offset.width) / 20))
                    .overlay {
                        ZStack {
                            // Approve overlay
                            Image(systemName: "checkmark.circle.fill")
                                .font(.system(size: 60))
                                .foregroundStyle(.green)
                                .opacity(offset.width > 0 ? min(Double(offset.width) / swipeThreshold, 1) : 0)

                            // Reject overlay
                            Image(systemName: "xmark.circle.fill")
                                .font(.system(size: 60))
                                .foregroundStyle(.red)
                                .opacity(offset.width < 0 ? min(Double(-offset.width) / swipeThreshold, 1) : 0)
                        }
                    }
                    .gesture(
                        DragGesture()
                            .onChanged { gesture in
                                offset = gesture.translation
                            }
                            .onEnded { gesture in
                                if gesture.translation.width > swipeThreshold {
                                    flyOff(direction: .right)
                                    onApprove(top)
                                } else if gesture.translation.width < -swipeThreshold {
                                    flyOff(direction: .left)
                                    onReject(top)
                                } else {
                                    withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                                        offset = .zero
                                    }
                                }
                            }
                    )
            }
        }
        .frame(height: 400)
    }

    private enum Direction { case left, right }

    private func flyOff(direction: Direction) {
        let x: CGFloat = direction == .right ? 500 : -500
        withAnimation(.easeOut(duration: 0.3)) {
            offset = CGSize(width: x, height: 0)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            offset = .zero
        }
    }
}
