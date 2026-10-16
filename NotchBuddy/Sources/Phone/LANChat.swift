#if LAN_LINK
import Foundation
import Observation
import PhotosUI
import SwiftUI
import UIKit

// Chat from the iPhone through Coucou on the Windows/Linux computer (docs/ANDROID_LINK.md,
// "Capabilities and chat"). The phone sends text and gets text back; the computer asks the
// provider. With Claude Code ticked on the computer, the answer comes from the Claude
// subscription its CLI is signed in with: no API key anywhere.
//
// Photos go first, as `chatImage` lines (a JPEG in base64 parts that each fit a line), then
// the `chatSend` names them in "images". The computer saves them where Claude Code or
// Gemini CLI can read them; other providers refuse photos.

struct LANChatModel: Identifiable, Equatable {
    let id: String
    let label: String
}

struct LANChatMessage: Identifiable, Equatable {
    enum Role { case user, assistant }
    let id: String
    let role: Role
    var text: String
    var failed = false
    var done = false
    /// Small JPEGs of the photos sent with it, to show in the bubble.
    var thumbnails: [Data] = []
}

/// A photo ready to send: a JPEG no larger than the computer takes, and its thumbnail.
struct LANChatPhoto: Identifiable, Hashable, Sendable {
    let id = UUID()
    let jpeg: Data
    let thumbnail: Data

    static let maxSide: CGFloat = 1600
    /// Bytes of JPEG per `chatImage` line: in base64 it stays well under the 64 KiB line.
    static let partBytes = 36_000

    /// Scaled down to `maxSide`, as JPEG. Nil when the image cannot be read.
    nonisolated static func make(from image: UIImage) -> LANChatPhoto? {
        let big = scaled(image, maxSide: maxSide)
        var quality: CGFloat = 0.75
        var jpeg = big.jpegData(compressionQuality: quality)
        // The computer takes 4 MB per photo.
        while let j = jpeg, j.count > 3_500_000, quality > 0.3 {
            quality -= 0.15
            jpeg = big.jpegData(compressionQuality: quality)
        }
        guard let jpeg, let thumb = scaled(image, maxSide: 200).jpegData(compressionQuality: 0.7) else { return nil }
        return LANChatPhoto(jpeg: jpeg, thumbnail: thumb)
    }

    nonisolated static func make(from data: Data) -> LANChatPhoto? {
        UIImage(data: data).flatMap(make(from:))
    }

    nonisolated private static func scaled(_ image: UIImage, maxSide: CGFloat) -> UIImage {
        let size = image.size
        let longest = max(size.width, size.height)
        guard longest > 0 else { return image }
        let ratio = min(1, maxSide / longest)
        let target = CGSize(width: (size.width * ratio).rounded(), height: (size.height * ratio).rounded())
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1
        format.opaque = true
        return UIGraphicsImageRenderer(size: target, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: target))
        }
    }

    /// The `chatImage` lines of this photo.
    func lines(id: String) -> [[String: Any]] {
        let parts = stride(from: 0, to: jpeg.count, by: Self.partBytes).map {
            jpeg.subdata(in: $0..<min($0 + Self.partBytes, jpeg.count))
        }
        return parts.enumerated().map { index, piece in
            ["type": "chatImage", "id": id, "part": index, "parts": parts.count, "data": piece.base64EncodedString()]
        }
    }
}

@MainActor
@Observable
final class LANChat {
    static let shared = LANChat()

    private(set) var models: [LANChatModel] = []
    private(set) var messages: [LANChatMessage] = []
    /// The answer on its way, if any: the computer runs one at a time.
    private(set) var running: String?
    /// The computer said which models it allows (maybe none).
    private(set) var modelsKnown = false
    var selectedModel: String {
        didSet { UserDefaults.standard.set(selectedModel, forKey: "lanChatModel") }
    }

    static let maxText = 4000
    static let maxPhotos = 4

    /// Claude Code and Gemini CLI read photos; the API providers don't take them here.
    static func takesPhotos(_ model: String) -> Bool {
        model.hasPrefix("claude-code/") || model.hasPrefix("gemini-cli/")
    }
    var selectedTakesPhotos: Bool { Self.takesPhotos(selectedModel) }

    private init() {
        selectedModel = UserDefaults.standard.string(forKey: "lanChatModel") ?? ""
    }

    var connected: Bool { LANLink.shared.state == .connected }
    var offered: Bool { connected && LANLink.shared.offeredCaps.contains("chat") }

    var selectedLabel: String {
        models.first { $0.id == selectedModel }?.label ?? "Model"
    }

    func requestModels() {
        LANLink.shared.sendChat(["type": "chatModels"])
    }

    // MARK: From the computer (LANLink)

    func received(models raw: [[String: Any]]) {
        models = raw.compactMap { item in
            guard let id = item["id"] as? String, !id.isEmpty else { return nil }
            return LANChatModel(id: id, label: item["label"] as? String ?? id)
        }
        modelsKnown = true
        if !models.contains(where: { $0.id == selectedModel }) {
            // Claude Code first: it needs no API key.
            selectedModel = models.first { $0.id.hasPrefix("claude-code/") }?.id ?? models.first?.id ?? ""
        }
    }

    func delta(id: String, text: String) {
        guard let i = index(of: id) else { return }
        messages[i].text += text
    }

    func done(id: String, text: String?) {
        guard let i = index(of: id) else { return }
        if let text { messages[i].text = text }
        messages[i].done = true
        if running == id { running = nil }
    }

    func failed(id: String, message: String) {
        guard let i = index(of: id) else { return }
        let current = messages[i].text.trimmingCharacters(in: .whitespacesAndNewlines)
        messages[i].text = current.isEmpty ? message : current + "\n\n⚠︎ " + message
        messages[i].failed = true
        messages[i].done = true
        if running == id { running = nil }
    }

    /// The connection went away: an answer still running will not come.
    func linkClosed() {
        if let id = running { failed(id: id, message: "The connection to your computer was lost.") }
        modelsKnown = false
    }

    // MARK: From the user

    @discardableResult
    func send(_ text: String, photos: [LANChatPhoto] = []) -> Bool {
        let trimmed = String(text.trimmingCharacters(in: .whitespacesAndNewlines).prefix(Self.maxText))
        let photos = Array(photos.prefix(Self.maxPhotos))
        guard !trimmed.isEmpty || !photos.isEmpty, running == nil, !selectedModel.isEmpty else { return false }
        guard photos.isEmpty || selectedTakesPhotos else { return false }
        let id = Self.newID("p")
        messages.append(LANChatMessage(id: "u" + id, role: .user, text: trimmed, done: true,
                                       thumbnails: photos.map(\.thumbnail)))
        messages.append(LANChatMessage(id: id, role: .assistant, text: ""))
        running = id
        let model = selectedModel
        Task {
            var imageIDs: [String] = []
            for photo in photos {
                let imageID = Self.newID("i")
                for line in photo.lines(id: imageID) {
                    guard await LANLink.shared.sendWhenConnected(line) else {
                        self.failed(id: id, message: "Not connected to your computer.")
                        return
                    }
                }
                imageIDs.append(imageID)
            }
            var message: [String: Any] = ["type": "chatSend", "id": id, "model": model, "text": trimmed]
            if !imageIDs.isEmpty { message["images"] = imageIDs }
            let sent = await LANLink.shared.sendWhenConnected(message)
            if !sent { self.failed(id: id, message: "Not connected to your computer.") }
        }
        return true
    }

    private static func newID(_ prefix: String) -> String {
        prefix + String(UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(24))
    }

    func cancel() {
        guard let id = running else { return }
        LANLink.shared.sendChat(["type": "chatCancel", "id": id])
        failed(id: id, message: "Stopped.")
    }

    func reset() {
        LANLink.shared.sendChat(["type": "chatReset"])
        messages = []
        running = nil
    }

    private func index(of id: String) -> Int? {
        messages.firstIndex { $0.id == id }
    }
}

// MARK: - The view

struct LANChatView: View {
    @State private var draft = ""
    @FocusState private var focused: Bool
    @State private var photos: [LANChatPhoto] = []
    @State private var pickerItems: [PhotosPickerItem] = []
    @State private var showLibrary = false
    @State private var showCamera = false
    @State private var dictation = Dictation()
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        let chat = LANChat.shared
        VStack(spacing: 0) {
            if let reason = unavailable(chat) {
                ContentUnavailableView {
                    Label("Chat", systemImage: "bubble.left.and.text.bubble.right")
                } description: {
                    Text(reason)
                }
            } else {
                messages(chat)
                composer(chat)
            }
        }
        .navigationTitle("Chat")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if chat.offered, !chat.models.isEmpty {
                ToolbarItem(placement: .topBarLeading) {
                    Menu {
                        Picker("Model", selection: Binding(get: { chat.selectedModel },
                                                           set: { chat.selectedModel = $0 })) {
                            ForEach(chat.models) { Text($0.label).tag($0.id) }
                        }
                    } label: {
                        Label(chat.selectedLabel, systemImage: "cpu")
                            .labelStyle(.titleAndIcon)
                            .font(.subheadline)
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        chat.reset()
                    } label: {
                        Image(systemName: "square.and.pencil")
                    }
                    .accessibilityLabel("New chat")
                    .disabled(chat.messages.isEmpty)
                }
            }
            // Above the keyboard: put it away.
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button {
                    focused = false
                } label: {
                    Image(systemName: "keyboard.chevron.compact.down")
                }
                .accessibilityLabel("Hide keyboard")
            }
        }
        .onAppear { if chat.offered { chat.requestModels() } }
        .onChange(of: chat.offered) { _, offered in if offered { chat.requestModels() } }
        .onChange(of: dictation.transcript) { _, words in
            if dictation.isRecording { draft = dictation.prefix + words }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase != .active { dictation.stop() }
        }
        .onDisappear { dictation.stop() }
        .photosPicker(isPresented: $showLibrary, selection: $pickerItems,
                      maxSelectionCount: max(1, LANChat.maxPhotos - photos.count), matching: .images)
        .onChange(of: pickerItems) { _, items in
            guard !items.isEmpty else { return }
            pickerItems = []
            Task {
                for item in items {
                    guard let data = try? await item.loadTransferable(type: Data.self) else { continue }
                    let photo = await Task.detached { LANChatPhoto.make(from: data) }.value
                    if let photo, photos.count < LANChat.maxPhotos { photos.append(photo) }
                }
            }
        }
        .fullScreenCover(isPresented: $showCamera) {
            CameraPicker { image in
                if let photo = LANChatPhoto.make(from: image), photos.count < LANChat.maxPhotos { photos.append(photo) }
            }
            .ignoresSafeArea()
        }
    }

    private func unavailable(_ chat: LANChat) -> String? {
        if !chat.connected {
            return "Connect to Coucou on your computer first."
        }
        if !chat.offered {
            return "On your computer, open Coucou Settings → Phone, turn on \"Let the phone chat with my AI providers\" and tick a Claude Code model (no API key needed)."
        }
        if chat.modelsKnown, chat.models.isEmpty {
            return "No model is allowed yet. On your computer, tick one under Settings → Phone → Models the phone may use (Claude Code needs no API key)."
        }
        return nil
    }

    private func messages(_ chat: LANChat) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    if chat.messages.isEmpty {
                        Text("Ask anything. The answer comes from \(chat.selectedLabel), through your computer.")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity)
                            .padding(.top, 40)
                    }
                    ForEach(chat.messages) { message in
                        LANChatBubble(message: message, waiting: chat.running == message.id)
                            .id(message.id)
                    }
                }
                .padding(16)
            }
            .scrollDismissesKeyboard(.interactively)
            .onChange(of: chat.messages.last?.text) { _, _ in
                guard let id = chat.messages.last?.id else { return }
                withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(id, anchor: .bottom) }
            }
        }
    }

    private func composer(_ chat: LANChat) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if !photos.isEmpty { attachedPhotos(chat) }
            HStack(alignment: .bottom, spacing: 8) {
                attachMenu(chat)
                HStack(alignment: .bottom, spacing: 6) {
                    TextField(dictation.isRecording ? "Listening…" : "Message", text: $draft, axis: .vertical)
                        .lineLimit(1...6)
                        .focused($focused)
                    micButton
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .glassCard(cornerRadius: 20)
                sendButton(chat)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    /// Photo library or camera; photos only for a model that reads them.
    private func attachMenu(_ chat: LANChat) -> some View {
        Menu {
            Button {
                showLibrary = true
            } label: {
                Label("Photo Library", systemImage: "photo.on.rectangle")
            }
            if UIImagePickerController.isSourceTypeAvailable(.camera) {
                Button {
                    showCamera = true
                } label: {
                    Label("Take Photo", systemImage: "camera")
                }
            }
        } label: {
            Image(systemName: "plus.circle.fill")
                .font(.system(size: 32))
                .symbolRenderingMode(.hierarchical)
        }
        .accessibilityLabel("Add a photo")
        .disabled(!chat.selectedTakesPhotos || photos.count >= LANChat.maxPhotos || chat.running != nil)
    }

    private func attachedPhotos(_ chat: LANChat) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(photos) { photo in
                        ZStack(alignment: .topTrailing) {
                            lanChatThumbnail(photo.thumbnail, size: 64)
                            Button {
                                photos.removeAll { $0.id == photo.id }
                            } label: {
                                Image(systemName: "xmark.circle.fill")
                                    .font(.system(size: 20))
                                    .symbolRenderingMode(.palette)
                                    .foregroundStyle(.white, .black.opacity(0.6))
                            }
                            .accessibilityLabel("Remove photo")
                            .offset(x: 6, y: -6)
                        }
                    }
                }
                .padding(.top, 6)
                .padding(.trailing, 6)
            }
            if !chat.selectedTakesPhotos {
                Text("Photos work only with Claude Code or Gemini CLI. Pick one of them above.")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
        }
    }

    private var micButton: some View {
        Button {
            Task { await dictation.toggle(startingFrom: draft) }
        } label: {
            Image(systemName: dictation.isRecording ? "waveform" : "mic.fill")
                .font(.system(size: 17))
                .foregroundStyle(dictation.isRecording ? Color.red : Color.secondary)
                .symbolEffect(.variableColor.iterative, isActive: dictation.isRecording)
        }
        .accessibilityLabel(dictation.isRecording ? "Stop dictation" : "Dictate")
        .contextMenu {
            Picker("Dictation language", selection: $dictation.localeID) {
                ForEach(Dictation.languages, id: \.self) { id in
                    Text(Locale.current.localizedString(forIdentifier: id) ?? id).tag(id)
                }
            }
        }
    }

    @ViewBuilder private func sendButton(_ chat: LANChat) -> some View {
        Group {
            if chat.running != nil {
                Button {
                    chat.cancel()
                } label: {
                    Image(systemName: "stop.circle.fill").font(.system(size: 32))
                }
                .accessibilityLabel("Stop")
            } else {
                Button {
                    dictation.stop()
                    if chat.send(draft, photos: photos) {
                        draft = ""
                        photos = []
                    }
                } label: {
                    Image(systemName: "arrow.up.circle.fill").font(.system(size: 32))
                }
                .accessibilityLabel("Send")
                .disabled(!canSend(chat))
            }
        }
    }

    private func canSend(_ chat: LANChat) -> Bool {
        let hasText = !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        guard !chat.selectedModel.isEmpty, hasText || !photos.isEmpty else { return false }
        return photos.isEmpty || chat.selectedTakesPhotos
    }
}

/// A photo as a small rounded square.
@MainActor func lanChatThumbnail(_ data: Data, size: CGFloat) -> some View {
    Group {
        if let image = UIImage(data: data) {
            Image(uiImage: image).resizable().scaledToFill()
        } else {
            Color.secondary.opacity(0.2)
        }
    }
    .frame(width: size, height: size)
    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
}

/// The camera, for one photo.
private struct CameraPicker: UIViewControllerRepresentable {
    let onImage: (UIImage) -> Void
    @Environment(\.dismiss) private var dismiss

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = .camera
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ picker: UIImagePickerController, context: Context) {}

    @MainActor final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let parent: CameraPicker
        init(_ parent: CameraPicker) { self.parent = parent }

        func imagePickerController(_ picker: UIImagePickerController,
                                   didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
            if let image = info[.originalImage] as? UIImage { parent.onImage(image) }
            parent.dismiss()
        }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
            parent.dismiss()
        }
    }
}

private struct LANChatBubble: View {
    let message: LANChatMessage
    let waiting: Bool

    var body: some View {
        HStack {
            if message.role == .user { Spacer(minLength: 40) }
            VStack(alignment: .leading, spacing: 8) {
                if !message.thumbnails.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(Array(message.thumbnails.enumerated()), id: \.offset) { _, data in
                            lanChatThumbnail(data, size: 72)
                        }
                    }
                }
                if message.text.isEmpty && waiting {
                    ProgressView().padding(.vertical, 4)
                } else if !message.text.isEmpty {
                    Text(rendered)
                        .textSelection(.enabled)
                        .foregroundStyle(message.failed ? Color.red : Color.primary)
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(message.role == .user ? Color.accentColor.opacity(0.18) : Color.secondary.opacity(0.12),
                        in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            if message.role == .assistant { Spacer(minLength: 40) }
        }
    }

    /// Light Markdown (bold, code, links), keeping the line breaks.
    private var rendered: AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        return (try? AttributedString(markdown: message.text, options: options)) ?? AttributedString(message.text)
    }
}
#endif
