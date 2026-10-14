#if LAN_LINK
import Foundation
import Observation
import SwiftUI

// Chat from the iPhone through Coucou on the Windows/Linux computer (docs/ANDROID_LINK.md,
// "Capabilities and chat"). The phone sends text and gets text back; the computer asks the
// provider. With Claude Code ticked on the computer, the answer comes from the Claude
// subscription its CLI is signed in with: no API key anywhere.

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
    func send(_ text: String) -> Bool {
        let trimmed = String(text.trimmingCharacters(in: .whitespacesAndNewlines).prefix(Self.maxText))
        guard !trimmed.isEmpty, running == nil, !selectedModel.isEmpty else { return false }
        let id = "p" + String(UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(24))
        messages.append(LANChatMessage(id: "u" + id, role: .user, text: trimmed, done: true))
        messages.append(LANChatMessage(id: id, role: .assistant, text: ""))
        running = id
        let model = selectedModel
        Task {
            let sent = await LANLink.shared.sendWhenConnected(
                ["type": "chatSend", "id": id, "model": model, "text": trimmed])
            if !sent { self.failed(id: id, message: "Not connected to your computer.") }
        }
        return true
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
        }
        .onAppear { if chat.offered { chat.requestModels() } }
        .onChange(of: chat.offered) { _, offered in if offered { chat.requestModels() } }
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
        HStack(alignment: .bottom, spacing: 8) {
            TextField("Message", text: $draft, axis: .vertical)
                .lineLimit(1...6)
                .focused($focused)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .glassCard(cornerRadius: 20)
            if chat.running != nil {
                Button {
                    chat.cancel()
                } label: {
                    Image(systemName: "stop.circle.fill").font(.system(size: 32))
                }
                .accessibilityLabel("Stop")
            } else {
                Button {
                    if chat.send(draft) { draft = "" }
                } label: {
                    Image(systemName: "arrow.up.circle.fill").font(.system(size: 32))
                }
                .accessibilityLabel("Send")
                .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || chat.selectedModel.isEmpty)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }
}

private struct LANChatBubble: View {
    let message: LANChatMessage
    let waiting: Bool

    var body: some View {
        HStack {
            if message.role == .user { Spacer(minLength: 40) }
            Group {
                if message.text.isEmpty && waiting {
                    ProgressView().padding(.vertical, 4)
                } else {
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
