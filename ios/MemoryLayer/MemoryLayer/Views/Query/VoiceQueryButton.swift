import SwiftUI
import Speech
import AVFoundation

struct VoiceQueryButton: View {
    @Binding var text: String
    @State private var isRecording = false
    @State private var recognizer: SFSpeechRecognizer? = SFSpeechRecognizer()
    @State private var recognitionRequest: SFSpeechAudioBufferRecognitionRequest?
    @State private var recognitionTask: SFSpeechRecognitionTask?
    @State private var audioEngine = AVAudioEngine()
    @State private var pulseAnimation = false

    var body: some View {
        Button(action: toggleRecording) {
            ZStack {
                if isRecording {
                    Circle()
                        .stroke(Color.red.opacity(0.3), lineWidth: 3)
                        .scaleEffect(pulseAnimation ? 1.4 : 1.0)
                        .opacity(pulseAnimation ? 0 : 1)
                        .animation(.easeInOut(duration: 1.0).repeatForever(autoreverses: false), value: pulseAnimation)
                        .frame(width: 36, height: 36)
                }

                Image(systemName: isRecording ? "mic.fill" : "mic")
                    .font(.title3)
                    .foregroundStyle(isRecording ? .red : .secondary)
            }
            .frame(width: 36, height: 36)
        }
        .buttonStyle(.plain)
    }

    private func toggleRecording() {
        if isRecording {
            stopRecording()
        } else {
            startRecording()
        }
    }

    private func startRecording() {
        SFSpeechRecognizer.requestAuthorization { status in
            guard status == .authorized else { return }

            Task { @MainActor in
                do {
                    let request = SFSpeechAudioBufferRecognitionRequest()
                    request.shouldReportPartialResults = true
                    self.recognitionRequest = request

                    let inputNode = audioEngine.inputNode
                    let recordingFormat = inputNode.outputFormat(forBus: 0)

                    inputNode.installTap(onBus: 0, bufferSize: 1024, format: recordingFormat) { buffer, _ in
                        request.append(buffer)
                    }

                    audioEngine.prepare()
                    try audioEngine.start()

                    recognitionTask = recognizer?.recognitionTask(with: request) { result, error in
                        if let result {
                            Task { @MainActor in
                                self.text = result.bestTranscription.formattedString
                            }
                        }
                        if error != nil || (result?.isFinal ?? false) {
                            Task { @MainActor in
                                self.stopRecording()
                            }
                        }
                    }

                    isRecording = true
                    pulseAnimation = true
                    HapticEngine.light()
                } catch {
                    // Failed to start recording
                }
            }
        }
    }

    private func stopRecording() {
        audioEngine.stop()
        audioEngine.inputNode.removeTap(onBus: 0)
        recognitionRequest?.endAudio()
        recognitionTask?.cancel()
        recognitionRequest = nil
        recognitionTask = nil
        isRecording = false
        pulseAnimation = false
        HapticEngine.light()
    }
}
