#if LAN_LINK
import AVFoundation
import Foundation
import Observation
import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers
import UserNotifications

// The beta chat of Coucou's owner and testers, through Coucou on the Windows/Linux computer
// (capability `betachat`, docs/ANDROID_LINK.md "Beta chat"). The iPhone has no GitHub sign-in of
// its own: it reads and writes the chat as the account signed in on the computer. Attachments go up
// and come down in base64 parts that each fit a line of the link. A new message from someone else
// plays the sound picked (made here, none a recording) and, in the background, a notification.

struct LANBetaFile: Identifiable, Equatable, Sendable {
    var id: String { url }
    let name: String
    let mime: String
    let size: Int
    let url: String
}

struct LANBetaMessage: Identifiable, Equatable, Sendable {
    let id: Int
    let login: String
    let text: String
    let build: Int?
    let files: [LANBetaFile]
    let at: Date?
    let owner: Bool
    let mine: Bool

    static func from(_ raw: [String: Any]) -> LANBetaMessage? {
        guard let id = (raw["id"] as? NSNumber)?.intValue, id > 0 else { return nil }
        let files: [LANBetaFile] = (raw["files"] as? [[String: Any]] ?? []).compactMap { f in
            guard let url = f["url"] as? String, url.hasPrefix("https://gist.githubusercontent.com/"), !url.contains("..") else { return nil }
            let name = (f["name"] as? String ?? "file").replacingOccurrences(of: "/", with: "")
            return LANBetaFile(name: String(name.prefix(80)), mime: f["mime"] as? String ?? "application/octet-stream",
                               size: (f["size"] as? NSNumber)?.intValue ?? 0, url: url)
        }
        let at = (raw["at"] as? NSNumber).map { Date(timeIntervalSince1970: $0.doubleValue) }
        return LANBetaMessage(id: id, login: String((raw["login"] as? String ?? "").prefix(39)),
                              text: String((raw["text"] as? String ?? "").prefix(2000)),
                              build: (raw["build"] as? NSNumber)?.intValue, files: Array(files.prefix(4)),
                              at: at, owner: raw["owner"] as? Bool ?? false, mine: raw["mine"] as? Bool ?? false)
    }
}

/// A file picked to send.
struct LANBetaOut: Identifiable, Sendable {
    let id = UUID()
    let name: String
    let mime: String
    let data: Data
    let thumbnail: Data?
}

struct LANBetaPending: Identifiable, Equatable {
    var id: String { key }
    let key: String
    let text: String
    let names: [String]
    var failed = false
}

@MainActor
@Observable
final class LANBetaChat {
    static let shared = LANBetaChat()

    static let maxFiles = 4
    static let maxFileBytes = 7 * 1024 * 1024
    /// Raw bytes per `betaChatUp` line (40 000 base64 characters).
    static let partBytes = 30_000

    private(set) var messages: [LANBetaMessage] = []
    private(set) var pending: [LANBetaPending] = []
    private(set) var owner = false
    private(set) var page = ""
    private(set) var error: String?
    private(set) var unread = 0
    /// Photos fetched, by address.
    private(set) var images: [String: UIImage] = [:]
    /// Videos and files fetched, as files to share or open.
    private(set) var saved: [String: URL] = [:]
    private(set) var loading: Set<String> = []
    var onScreen = false {
        didSet { if onScreen { unread = 0 } }
    }
    var sound: String {
        didSet { UserDefaults.standard.set(sound, forKey: "betaChatSound") }
    }
    var volume: Double {
        didSet { UserDefaults.standard.set(volume, forKey: "betaChatVolume") }
    }

    @ObservationIgnored private var pageParts: [Int: [LANBetaMessage]] = [:]
    @ObservationIgnored private var pageCount = 0
    @ObservationIgnored private var downs: [String: [Int: String]] = [:]
    @ObservationIgnored private var outgoing: [String: (String, [LANBetaOut])] = [:]

    private init() {
        sound = UserDefaults.standard.string(forKey: "betaChatSound") ?? "chime"
        volume = UserDefaults.standard.object(forKey: "betaChatVolume") as? Double ?? 0.8
        LANBetaSounds.install()
    }

    var offered: Bool { LANLink.shared.state == .connected && LANLink.shared.offeredCaps.contains("betachat") }

    func refresh(full: Bool = false) {
        LANLink.shared.sendBetaChat(["type": "betaChatGet", "full": full])
    }

    // MARK: From the computer

    func receive(_ type: String, _ m: [String: Any]) {
        switch type {
        case "betaChatPage":
            error = m["error"] as? String
            if error != nil { return }
            owner = m["owner"] as? Bool ?? false
            if let link = m["page"] as? String, link.hasPrefix("https://github.com/") { page = link }
            let part = (m["part"] as? NSNumber)?.intValue ?? 0
            let parts = max(1, min(64, (m["parts"] as? NSNumber)?.intValue ?? 1))
            if part == 0 || parts != pageCount { pageParts = [:]; pageCount = parts }
            pageParts[part] = (m["messages"] as? [[String: Any]] ?? []).compactMap(LANBetaMessage.from)
            guard pageParts.count == pageCount else { return }
            var seen = Set<Int>()
            messages = (0..<pageCount).flatMap { pageParts[$0] ?? [] }.filter { seen.insert($0.id).inserted }.sorted { $0.id < $1.id }
            pageParts = [:]
            if onScreen { unread = 0 }
        case "betaChatSent":
            guard let key = m["key"] as? String else { return }
            if let raw = m["message"] as? [String: Any], let message = LANBetaMessage.from(raw) {
                outgoing[key] = nil
                pending.removeAll { $0.key == key }
                if !messages.contains(where: { $0.id == message.id }) { messages.append(message) }
            } else {
                fail(key)
            }
        case "betaChatDeleted":
            if m["error"] != nil { refresh(full: true) }
        case "betaChatDown":
            guard let url = m["url"] as? String else { return }
            if m["error"] != nil { downs[url] = nil; loading.remove(url); return }
            let part = (m["part"] as? NSNumber)?.intValue ?? 0
            let parts = (m["parts"] as? NSNumber)?.intValue ?? 1
            downs[url, default: [:]][part] = m["data"] as? String ?? ""
            guard let got = downs[url], got.count == parts else { return }
            downs[url] = nil
            loading.remove(url)
            let text = (0..<parts).map { got[$0] ?? "" }.joined()
            guard let data = Data(base64Encoded: text), data.count <= Self.maxFileBytes else { return }
            if let image = UIImage(data: data), file(url)?.mime.hasPrefix("image/") ?? true {
                images[url] = image
            } else if let f = file(url) {
                let dir = FileManager.default.temporaryDirectory.appendingPathComponent("beta-chat", isDirectory: true)
                try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                let target = dir.appendingPathComponent(f.name)
                if (try? data.write(to: target)) != nil { saved[url] = target }
            }
        case "betaChatNews":
            let count = (m["count"] as? NSNumber)?.intValue ?? 1
            unread = onScreen ? 0 : count
            LANBetaSounds.play(sound, volume: volume)
            if UIApplication.shared.applicationState != .active {
                notify(login: m["login"] as? String ?? "", text: m["text"] as? String ?? "")
            }
            if onScreen { refresh() }
        case "betaChatChanged":
            if onScreen { refresh() }
        default:
            break
        }
    }

    private func file(_ url: String) -> LANBetaFile? {
        for m in messages { if let f = m.files.first(where: { $0.url == url }) { return f } }
        return nil
    }

    private func notify(login: String, text: String) {
        let content = UNMutableNotificationContent()
        content.title = login.isEmpty ? "Beta chat" : "@\(login)"
        content.body = text.isEmpty ? "New message in the beta chat" : String(text.prefix(200))
        content.sound = LANBetaSounds.notificationSound(sound)
        content.threadIdentifier = "betachat"
        let request = UNNotificationRequest(identifier: "betachat-\(UUID().uuidString)", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    func linkClosed() {
        for i in pending.indices { pending[i].failed = true }
    }

    // MARK: From the user

    @discardableResult
    func send(_ text: String, files: [LANBetaOut]) -> Bool {
        let body = String(text.trimmingCharacters(in: .whitespacesAndNewlines).prefix(2000))
        let files = Array(files.prefix(Self.maxFiles))
        guard !body.isEmpty || !files.isEmpty else { return false }
        let key = "p" + String(UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(20))
        outgoing[key] = (body, files)
        pending.append(LANBetaPending(key: key, text: body, names: files.map(\.name)))
        deliver(key)
        return true
    }

    func retry(_ key: String) {
        guard let i = pending.firstIndex(where: { $0.key == key }) else { return }
        pending[i].failed = false
        deliver(key)
    }

    private func deliver(_ key: String) {
        guard let item = outgoing[key] else { return }
        let (body, files) = item
        Task {
            var named: [[String: Any]] = []
            for (n, f) in files.enumerated() {
                let fid = "\(key)-\(n)"
                let parts = max(1, (f.data.count + Self.partBytes - 1) / Self.partBytes)
                for part in 0..<parts {
                    let from = part * Self.partBytes
                    let piece = f.data.subdata(in: from..<min(f.data.count, from + Self.partBytes))
                    let line: [String: Any] = ["type": "betaChatUp", "fid": fid, "part": part, "parts": parts, "data": piece.base64EncodedString()]
                    guard await LANLink.shared.sendWhenConnected(line) else { self.fail(key); return }
                }
                named.append(["fid": fid, "name": f.name, "mime": f.mime])
            }
            let ok = await LANLink.shared.sendWhenConnected(["type": "betaChatSend", "key": key, "text": body, "files": named])
            if !ok { self.fail(key) }
        }
    }

    private func fail(_ key: String) {
        if let i = pending.firstIndex(where: { $0.key == key }) { pending[i].failed = true }
    }

    func delete(_ id: Int) {
        messages.removeAll { $0.id == id }
        LANLink.shared.sendBetaChat(["type": "betaChatDelete", "id": id])
    }

    func fetch(_ f: LANBetaFile) {
        guard images[f.url] == nil, saved[f.url] == nil, !loading.contains(f.url) else { return }
        loading.insert(f.url)
        LANLink.shared.sendBetaChat(["type": "betaChatFile", "url": f.url])
    }
}

// MARK: - The sounds

/// The five sounds of a new message, as on the computer, made sample by sample (none is a recording).
/// Written once as WAV files into Library/Sounds, where a notification can use them too.
enum LANBetaSounds {
    static let kinds = ["chime", "hee", "quack", "knock", "mochi", "none"]
    private static let rate = 44_100.0
    @MainActor private static var player: AVAudioPlayer?

    static func label(_ kind: String) -> String {
        switch kind {
        case "hee": return "“Hee-hee!” (in the spirit of Michael Jackson)"
        case "quack": return "Quack"
        case "knock": return "Knock on the door"
        case "mochi": return "Mochi"
        case "none": return "No sound"
        default: return "Three notes"
        }
    }

    private static func tone(_ s: inout [Double], f0: Double, t0: Double, d: Double, vol: Double, f1: Double? = nil,
                             shape: (Double) -> Double = sin, vibrato: Double = 0) {
        let start = Int(t0 * rate), n = Int(d * rate), end = f1 ?? f0
        var phase = 0.0
        for i in 0..<n where start + i < s.count {
            let t = Double(i) / rate
            let f = f0 * pow(end / f0, t / d) + vibrato * sin(2 * .pi * 9 * t)
            phase += 2 * .pi * f / rate
            let env = min(1, t / 0.012) * exp(-5 * t / d)
            s[start + i] += shape(phase) * env * vol
        }
    }

    private static func triangle(_ p: Double) -> Double { let x = (p / (2 * .pi)).truncatingRemainder(dividingBy: 1); return 4 * abs(x - 0.5) - 1 }
    private static func saw(_ p: Double) -> Double { 2 * (p / (2 * .pi)).truncatingRemainder(dividingBy: 1) - 1 }

    static func samples(_ kind: String) -> [Double]? {
        var s: [Double]
        switch kind {
        case "hee":
            s = Array(repeating: 0, count: Int(0.8 * rate))
            tone(&s, f0: 700, t0: 0, d: 0.34, vol: 0.30, f1: 1500, shape: triangle, vibrato: 40)
            tone(&s, f0: 1300, t0: 0.37, d: 0.22, vol: 0.24, f1: 1700, shape: triangle)
            tone(&s, f0: 1700, t0: 0.60, d: 0.12, vol: 0.16, f1: 1200)
        case "chime":
            s = Array(repeating: 0, count: Int(1.0 * rate))
            tone(&s, f0: 1318.5, t0: 0, d: 0.35, vol: 0.26)
            tone(&s, f0: 1046.5, t0: 0.14, d: 0.35, vol: 0.26)
            tone(&s, f0: 1568, t0: 0.28, d: 0.6, vol: 0.26)
        case "quack":
            s = Array(repeating: 0, count: Int(0.5 * rate))
            tone(&s, f0: 260, t0: 0, d: 0.17, vol: 0.35, f1: 180, shape: saw)
            tone(&s, f0: 260, t0: 0.22, d: 0.17, vol: 0.35, f1: 180, shape: saw)
        case "knock":
            s = Array(repeating: 0, count: Int(0.55 * rate))
            var seed: UInt64 = 7
            for t0 in [0.0, 0.18, 0.36] {
                let start = Int(t0 * rate)
                var low = 0.0
                for i in 0..<Int(0.08 * rate) where start + i < s.count {
                    seed = seed &* 6364136223846793005 &+ 1442695040888963407
                    let noise = (Double(seed >> 33) / Double(1 << 31) * 2 - 1) * exp(-Double(i) / (rate * 0.012))
                    low += (noise - low) * 0.08
                    s[start + i] += low * 2.2
                }
                tone(&s, f0: 110, t0: t0, d: 0.09, vol: 0.5)
            }
        case "mochi":
            s = Array(repeating: 0, count: Int(0.2 * rate))
            tone(&s, f0: 880, t0: 0, d: 0.12, vol: 0.25, f1: 1320)
        default:
            return nil
        }
        return s
    }

    /// 16-bit mono PCM in a WAV file.
    static func wav(_ samples: [Double], volume: Double = 1) -> Data {
        var pcm = Data(capacity: samples.count * 2)
        for x in samples {
            let v = Int16(max(-1, min(1, x * volume)) * Double(Int16.max))
            withUnsafeBytes(of: v.littleEndian) { pcm.append(contentsOf: $0) }
        }
        var d = Data()
        func put32(_ v: UInt32) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        func put16(_ v: UInt16) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        d.append(contentsOf: Array("RIFF".utf8)); put32(UInt32(36 + pcm.count))
        d.append(contentsOf: Array("WAVE".utf8)); d.append(contentsOf: Array("fmt ".utf8))
        put32(16); put16(1); put16(1); put32(UInt32(rate)); put32(UInt32(rate) * 2); put16(2); put16(16)
        d.append(contentsOf: Array("data".utf8)); put32(UInt32(pcm.count))
        d.append(pcm)
        return d
    }

    private static func fileName(_ kind: String) -> String { "coucou-chat-\(kind).wav" }

    /// Library/Sounds: the only place a notification takes a sound of the app's own from.
    static func install() {
        guard let library = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask).first else { return }
        let dir = library.appendingPathComponent("Sounds", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        for kind in kinds {
            let url = dir.appendingPathComponent(fileName(kind))
            if FileManager.default.fileExists(atPath: url.path) { continue }
            if let s = samples(kind) { try? wav(s).write(to: url) }
        }
    }

    static func notificationSound(_ kind: String) -> UNNotificationSound? {
        kind == "none" ? nil : UNNotificationSound(named: UNNotificationSoundName(fileName(kind)))
    }

    @MainActor static func play(_ kind: String, volume: Double) {
        guard volume > 0, let s = samples(kind) else { return }
        try? AVAudioSession.sharedInstance().setCategory(.ambient, options: [.mixWithOthers])
        player = try? AVAudioPlayer(data: wav(s, volume: volume))
        player?.play()
    }
}

// MARK: - The view

struct LANBetaChatView: View {
    @State private var draft = ""
    @State private var files: [LANBetaOut] = []
    @State private var pickerItems: [PhotosPickerItem] = []
    @State private var showLibrary = false
    @State private var showFiles = false
    @State private var note: String?
    @FocusState private var focused: Bool
    @Environment(\.openURL) private var openURL

    var body: some View {
        let chat = LANBetaChat.shared
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    Text("With the owner and the beta testers, as your computer's GitHub account. The chat is public on GitHub: no passwords or keys here.")
                        .font(.caption).foregroundStyle(.secondary)
                    if let error = chat.error {
                        Text(errorText(error)).font(.caption).foregroundStyle(.red)
                    }
                    if chat.messages.isEmpty && chat.pending.isEmpty {
                        Text("No messages yet. Say hello to the testers!")
                            .font(.subheadline).foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity).padding(.top, 40)
                    }
                    ForEach(chat.messages) { m in
                        LANBetaBubble(message: m, owner: chat.owner).id("m\(m.id)")
                    }
                    ForEach(chat.pending) { p in
                        VStack(alignment: .trailing, spacing: 4) {
                            Text((p.names.map { "📎 \($0)" } + [p.text]).filter { !$0.isEmpty }.joined(separator: "\n"))
                                .padding(.horizontal, 14).padding(.vertical, 10)
                                .background(Color.accentColor.opacity(0.35), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                            if p.failed {
                                Button("Not sent. Tap to retry") { chat.retry(p.key) }.font(.caption).tint(.red)
                            } else {
                                Text("Sending ✓").font(.caption2).foregroundStyle(.secondary)
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .trailing)
                        .id(p.key)
                    }
                }
                .padding(16)
            }
            .scrollDismissesKeyboard(.interactively)
            .onChange(of: chat.messages.count + chat.pending.count) { _, _ in
                let last = chat.pending.last.map { $0.key } ?? chat.messages.last.map { "m\($0.id)" }
                if let last { withAnimation { proxy.scrollTo(last, anchor: .bottom) } }
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) { composer(chat) }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Picker("Sound", selection: Binding(get: { chat.sound }, set: { chat.sound = $0; LANBetaSounds.play($0, volume: chat.volume) })) {
                        ForEach(LANBetaSounds.kinds, id: \.self) { Text(LANBetaSounds.label($0)).tag($0) }
                    }
                    if !chat.page.isEmpty, let url = URL(string: chat.page) {
                        Button { openURL(url) } label: { Label("On GitHub", systemImage: "arrow.up.right.square") }
                    }
                } label: {
                    Image(systemName: "bell")
                }
                .accessibilityLabel("Sound of a new message")
            }
        }
        .onAppear {
            chat.onScreen = true
            chat.refresh(full: true)
        }
        .onDisappear { chat.onScreen = false }
        .task {
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(10))
                if chat.onScreen { chat.refresh() }
            }
        }
        .photosPicker(isPresented: $showLibrary, selection: $pickerItems, maxSelectionCount: max(1, LANBetaChat.maxFiles - files.count),
                      matching: .any(of: [.images, .videos]))
        .onChange(of: pickerItems) { _, items in
            guard !items.isEmpty else { return }
            pickerItems = []
            Task {
                for item in items {
                    guard files.count < LANBetaChat.maxFiles, let data = try? await item.loadTransferable(type: Data.self) else { continue }
                    let type = item.supportedContentTypes.first
                    if type?.conforms(to: .image) == true, let photo = LANChatPhoto.make(from: data) {
                        files.append(LANBetaOut(name: "photo.jpg", mime: "image/jpeg", data: photo.jpeg, thumbnail: photo.thumbnail))
                    } else if data.count <= LANBetaChat.maxFileBytes {
                        let ext = type?.preferredFilenameExtension ?? "mov"
                        files.append(LANBetaOut(name: "video.\(ext)", mime: type?.preferredMIMEType ?? "video/quicktime", data: data, thumbnail: nil))
                    } else {
                        note = "Too big for the chat: at most 4 attachments of 7 MB each."
                    }
                }
            }
        }
        .fileImporter(isPresented: $showFiles, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result else { return }
            for url in urls where files.count < LANBetaChat.maxFiles {
                let scoped = url.startAccessingSecurityScopedResource()
                defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                guard let data = try? Data(contentsOf: url) else { continue }
                if data.count > LANBetaChat.maxFileBytes { note = "Too big for the chat: at most 4 attachments of 7 MB each."; continue }
                let type = UTType(filenameExtension: url.pathExtension)
                files.append(LANBetaOut(name: url.lastPathComponent, mime: type?.preferredMIMEType ?? "application/octet-stream", data: data, thumbnail: nil))
            }
        }
    }

    private func errorText(_ code: String) -> String {
        switch code {
        case "notSignedIn", "notAllowed": return "Your computer's GitHub account is not in the beta any more."
        case "signInAgain": return "Sign in again in Coucou on your computer (Settings > Account)."
        case "tooBig": return "Too big for the chat: at most 4 attachments of 7 MB each."
        default: return "GitHub could not be reached. Coucou tries again."
        }
    }

    private func composer(_ chat: LANBetaChat) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if !files.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(files) { f in
                            ZStack(alignment: .topTrailing) {
                                if let thumb = f.thumbnail {
                                    lanChatThumbnail(thumb, size: 56)
                                } else {
                                    Image(systemName: f.mime.hasPrefix("video/") ? "film" : "doc")
                                        .frame(width: 56, height: 56)
                                        .background(Color.secondary.opacity(0.2), in: RoundedRectangle(cornerRadius: 10))
                                }
                                Button { files.removeAll { $0.id == f.id } } label: {
                                    Image(systemName: "xmark.circle.fill").symbolRenderingMode(.palette).foregroundStyle(.white, .black.opacity(0.6))
                                }
                                .accessibilityLabel("Remove")
                                .offset(x: 6, y: -6)
                            }
                        }
                    }
                    .padding(.top, 6).padding(.trailing, 6)
                }
            }
            if let note { Text(note).font(.caption).foregroundStyle(.orange) }
            HStack(alignment: .bottom, spacing: 8) {
                Menu {
                    Button { showLibrary = true } label: { Label("Photo or Video", systemImage: "photo.on.rectangle") }
                    Button { showFiles = true } label: { Label("File", systemImage: "doc") }
                    Button { pastePhoto() } label: { Label("Paste Photo", systemImage: "doc.on.clipboard") }
                } label: {
                    Image(systemName: "paperclip.circle.fill").font(.system(size: 32)).symbolRenderingMode(.hierarchical)
                }
                .accessibilityLabel("Attach")
                .disabled(files.count >= LANBetaChat.maxFiles)
                TextField("Write to the testers", text: $draft, axis: .vertical)
                    .lineLimit(1...6)
                    .focused($focused)
                    .padding(.horizontal, 14).padding(.vertical, 10)
                    .glassCard(cornerRadius: 20)
                Button {
                    if chat.send(draft, files: files) {
                        draft = ""
                        files = []
                        note = nil
                    }
                } label: {
                    Image(systemName: "arrow.up.circle.fill").font(.system(size: 32))
                }
                .accessibilityLabel("Send")
                .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && files.isEmpty)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private func pastePhoto() {
        guard let image = UIPasteboard.general.image, let photo = LANChatPhoto.make(from: image) else {
            note = "There is no photo in the clipboard."
            return
        }
        files.append(LANBetaOut(name: "clipboard.jpg", mime: "image/jpeg", data: photo.jpeg, thumbnail: photo.thumbnail))
    }
}

private struct LANBetaBubble: View {
    let message: LANBetaMessage
    let owner: Bool

    var body: some View {
        let chat = LANBetaChat.shared
        HStack(alignment: .top, spacing: 6) {
            if message.mine { Spacer(minLength: 40) }
            VStack(alignment: message.mine ? .trailing : .leading, spacing: 3) {
                HStack(spacing: 4) {
                    Text("@\(message.login)").font(.caption2).foregroundStyle(.secondary)
                    if message.owner { Text("· owner").font(.caption2.weight(.semibold)).foregroundStyle(.orange) }
                }
                VStack(alignment: .leading, spacing: 6) {
                    if let build = message.build { Text("🧪 build \(build)").font(.caption).foregroundStyle(.orange) }
                    if !message.text.isEmpty { Text(message.text).textSelection(.enabled) }
                    ForEach(message.files) { f in attachment(f, chat) }
                }
                .padding(.horizontal, 14).padding(.vertical, 10)
                .background(message.mine ? Color.accentColor.opacity(0.35) : Color.secondary.opacity(0.14),
                            in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                .contextMenu {
                    if message.mine || owner {
                        Button(role: .destructive) { chat.delete(message.id) } label: { Label("Delete", systemImage: "trash") }
                    }
                }
                if let at = message.at {
                    Text(at.formatted(date: .omitted, time: .shortened) + (message.mine ? " ✓✓" : ""))
                        .font(.caption2).foregroundStyle(.secondary)
                }
            }
            if !message.mine { Spacer(minLength: 40) }
        }
    }

    @ViewBuilder private func attachment(_ f: LANBetaFile, _ chat: LANBetaChat) -> some View {
        if f.mime.hasPrefix("image/") {
            Group {
                if let image = chat.images[f.url] {
                    Image(uiImage: image).resizable().scaledToFit().frame(maxWidth: 240, maxHeight: 240)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                } else {
                    Label(f.name, systemImage: "photo").font(.caption)
                }
            }
            .onAppear { chat.fetch(f) }
        } else if let url = chat.saved[f.url] {
            ShareLink(item: url) { Label(f.name, systemImage: f.mime.hasPrefix("video/") ? "play.rectangle" : "doc") }
                .font(.caption)
        } else {
            Button {
                chat.fetch(f)
            } label: {
                Label(chat.loading.contains(f.url) ? "Loading…" : "\(f.name) · \(ByteCountFormatter.string(fromByteCount: Int64(f.size), countStyle: .file))",
                      systemImage: f.mime.hasPrefix("video/") ? "play.rectangle" : "doc")
            }
            .font(.caption)
        }
    }
}

/// The Chat tab: the AI chat and, when the computer offers it, the beta chat beside it.
struct LANChatTab: View {
    @State private var beta = false

    var body: some View {
        let chat = LANBetaChat.shared
        VStack(spacing: 0) {
            if chat.offered {
                Picker("Chat", selection: $beta) {
                    Text("AI chat").tag(false)
                    Text(chat.unread > 0 ? "Beta chat (\(chat.unread))" : "Beta chat").tag(true)
                }
                .pickerStyle(.segmented)
                .padding(.horizontal, 16)
                .padding(.vertical, 6)
            }
            if beta && chat.offered {
                LANBetaChatView().navigationTitle("Beta chat").navigationBarTitleDisplayMode(.inline)
            } else {
                LANChatView()
            }
        }
    }
}
#endif
