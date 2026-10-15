#if LAN_LINK
import CryptoKit
import Foundation
import Network
import Observation
import Security
import UIKit
import UserNotifications

// Coucou on iPhone, linked to Coucou for Windows/Linux over the local network.
//
// The iCloud link (PhoneLink + CloudKit) only exists between a Mac and an iPhone.
// Coucou for Windows/Linux already runs the phone link of the Android app
// (docs/ANDROID_LINK.md, protocol v1): TLS with a pinned self-signed certificate,
// one JSON object per line. This file speaks that protocol, so the same iPhone
// app works with a Windows computer. Compiled only with the LAN_LINK condition
// (the sideloaded build); the App Store build keeps iCloud.

/// A computer paired with this iPhone: what the `coucou://pair` link holds.
struct LANPairing: Codable, Equatable, Sendable {
    var host: String
    var port: UInt16
    /// Lowercase hex SHA-256 of the computer's certificate (the only one accepted).
    var fingerprint: String
    var token: String
    var name: String
    /// Where the computer is away from home (its Tailscale address, say); tried when the home address fails.
    var awayHost: String? = nil

    /// `coucou://pair?v=1&host=…&port=…&fp=…&token=…&name=…` (extra relay fields are ignored).
    static func parse(_ text: String) -> LANPairing? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let parts = URLComponents(string: trimmed),
              parts.scheme?.lowercased() == "coucou", parts.host?.lowercased() == "pair" else { return nil }
        var query: [String: String] = [:]
        for item in parts.queryItems ?? [] { if let value = item.value { query[item.name] = value } }
        let tokenChars = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_")
        guard query["v"] == "1",
              let host = query["host"], !host.isEmpty, host.count <= 255,
              let portText = query["port"], let port = UInt16(portText), port > 0,
              let fingerprint = query["fp"]?.lowercased(), fingerprint.count == 64,
              fingerprint.allSatisfy({ $0.isHexDigit }),
              let token = query["token"], (16...128).contains(token.count),
              token.unicodeScalars.allSatisfy({ tokenChars.contains($0) })
        else { return nil }
        let name = String((query["name"] ?? "").prefix(64))
        return LANPairing(host: host, port: port, fingerprint: fingerprint, token: token,
                          name: name.isEmpty ? host : name)
    }
}

/// The pairing (with its secret token) lives in the Keychain, never in a file.
enum LANPairingStore {
    private static let service = "fr.louisraille.Coucou.lan-link"
    private static let account = "pairing"

    private static var baseQuery: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    static func load() -> LANPairing? {
        var query = baseQuery
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
        return try? JSONDecoder().decode(LANPairing.self, from: data)
    }

    static func save(_ pairing: LANPairing) {
        guard let data = try? JSONEncoder().encode(pairing) else { return }
        delete()
        var item = baseQuery
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    static func delete() {
        SecItemDelete(baseQuery as CFDictionary)
    }
}

// MARK: - The socket (TLS, pinned certificate, one JSON object per line)

/// One connection to the computer. Lives on its own queue; every event is
/// handed to the main thread in order.
final class LANSocket: @unchecked Sendable {
    enum Event: Sendable {
        case ready(NWEndpoint?)
        case line(Data)
        case closed(String)
    }

    private let queue: DispatchQueue
    private let connection: NWConnection
    private let onEvent: @Sendable (Event) -> Void
    private let mismatch: Flag
    private var buffer = Data()
    private var finished = false

    private final class Flag: @unchecked Sendable { var value = false }

    init(endpoint: NWEndpoint, fingerprint: String, onEvent: @escaping @Sendable (Event) -> Void) {
        let queue = DispatchQueue(label: "fr.louisraille.Coucou.lan")
        let mismatch = Flag()
        self.queue = queue
        self.mismatch = mismatch
        self.onEvent = onEvent
        let expected = fingerprint.lowercased()
        let tls = NWProtocolTLS.Options()
        // No certificate authority, no host name: the computer's certificate is
        // accepted only if its SHA-256 is the one from the pairing (pinning).
        sec_protocol_options_set_verify_block(tls.securityProtocolOptions, { _, trust, complete in
            let ok = LANSocket.certificate(of: trust) == expected
            if !ok { mismatch.value = true }
            complete(ok)
        }, queue)
        let tcp = NWProtocolTCP.Options()
        tcp.connectionTimeout = 5
        tcp.enableKeepalive = true
        tcp.keepaliveIdle = 30
        let parameters = NWParameters(tls: tls, tcp: tcp)
        parameters.includePeerToPeer = false
        connection = NWConnection(to: endpoint, using: parameters)
    }

    /// Lowercase hex SHA-256 of the peer's leaf certificate.
    static func certificate(of trust: sec_trust_t) -> String? {
        let secTrust = sec_trust_copy_ref(trust).takeRetainedValue()
        guard let chain = SecTrustCopyCertificateChain(secTrust) as? [SecCertificate], let leaf = chain.first else { return nil }
        let der = SecCertificateCopyData(leaf) as Data
        return SHA256.hash(data: der).map { String(format: "%02x", $0) }.joined()
    }

    func start() {
        connection.stateUpdateHandler = { [weak self] state in self?.handle(state) }
        connection.start(queue: queue)
    }

    private func handle(_ state: NWConnection.State) {
        switch state {
        case .ready:
            onEvent(.ready(connection.currentPath?.remoteEndpoint))
            receive()
        case .waiting(let error):
            // Refused or unreachable: say so now rather than wait for the network to change.
            finish(mismatch.value ? "certificate" : "\(error)")
            connection.cancel()
        case .failed(let error):
            finish(mismatch.value ? "certificate" : "\(error)")
        case .cancelled:
            finish("closed")
        default:
            break
        }
    }

    private func receive() {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { [weak self] data, _, complete, error in
            guard let self else { return }
            if let data, !data.isEmpty {
                self.buffer.append(data)
                self.split()
            }
            if complete || error != nil {
                self.finish(error.map { "\($0)" } ?? "closed by the computer")
                self.connection.cancel()
                return
            }
            if !self.finished { self.receive() }
        }
    }

    private func split() {
        while let newline = buffer.firstIndex(of: 0x0A) {
            let line = Data(buffer[buffer.startIndex..<newline])
            buffer.removeSubrange(buffer.startIndex...newline)
            if !line.isEmpty { onEvent(.line(line)) }
        }
        if buffer.count > 64 * 1024 {
            finish("line too long")
            connection.cancel()
        }
    }

    func send(_ line: Data) {
        queue.async { [self] in
            guard !finished else { return }
            var data = line
            data.append(0x0A)
            connection.send(content: data, completion: .contentProcessed { _ in })
        }
    }

    func close() {
        queue.async { [self] in
            finished = true
            connection.cancel()
        }
    }

    private func finish(_ reason: String) {
        guard !finished else { return }
        finished = true
        onEvent(.closed(reason))
    }
}

// MARK: - What the computer sends

struct LANSession {
    var pillId: String
    var agent: String
    var state: String
    var statusText: String
    var updatedAt: Date
    var steps: [String]
    var finalLine: String
    var project: String
    var color: String
}

struct LANApproval {
    var pillId: String
    var fingerprint: String
    var tool: String
    var command: String
    var createdAt: Date
    var truncated: Bool
}

struct LANQuestion {
    var pillId: String
    var fingerprint: String
    var createdAt: Date
    var payload: QuestionPayload
}

// MARK: - The link

@MainActor
@Observable
final class LANLink {
    static let shared = LANLink()

    enum State: Equatable {
        case unpaired
        case connecting
        case connected
        case offline(String)
        /// The computer refused the pairing code (paired again on the computer, or unpaired).
        case rejected
    }

    private(set) var pairing: LANPairing? = LANPairingStore.load()
    private(set) var state: State = .unpaired
    private(set) var desktopName = ""
    private(set) var offeredCaps: Set<String> = []
    /// A pairing link was opened (QR code, paste): waiting for the user's OK.
    var proposedPairing: LANPairing?

    @ObservationIgnored private var socket: LANSocket?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var retryTask: Task<Void, Never>?
    @ObservationIgnored private var pingTask: Task<Void, Never>?
    @ObservationIgnored private var lastHeard = Date()
    @ObservationIgnored private var failures = 0
    /// The away address answered last time: try it first next time.
    @ObservationIgnored private var preferAway = UserDefaults.standard.bool(forKey: "lanPreferAway")
    @ObservationIgnored private var browser: NWBrowser?
    @ObservationIgnored private var searchedSinceWelcome = false
    @ObservationIgnored private var backgroundTask: UIBackgroundTaskIdentifier = .invalid

    @ObservationIgnored private var sessions: [LANSession] = []
    @ObservationIgnored private var approvals: [String: LANApproval] = [:]
    @ObservationIgnored private var questions: [String: LANQuestion] = [:]
    @ObservationIgnored private var notified: Set<String> = []

    static let protocolVersion = 1
    private static let askedCaps = ["details", "answers", "chat"]

    // MARK: Pairing

    func pair(_ new: LANPairing) {
        LANPairingStore.save(new)
        pairing = new
        proposedPairing = nil
        failures = 0
        resetContent()
        connect()
    }

    /// The address to try away from home (empty: none).
    func setAwayHost(_ text: String) {
        guard var updated = pairing else { return }
        let cleaned = text.trimmingCharacters(in: .whitespacesAndNewlines)
        updated.awayHost = cleaned.isEmpty ? nil : String(cleaned.prefix(255))
        LANPairingStore.save(updated)
        pairing = updated
        failures = 0
        connect()
    }

    func unpair() {
        if let socket { send(["type": "bye"], on: socket) }
        disconnect()
        LANPairingStore.delete()
        LANLiveActivity.shared.end()
        pairing = nil
        state = .unpaired
        resetContent()
        publish()
    }

    // MARK: Connecting

    func start() {
        guard pairing != nil else {
            state = .unpaired
            publish()
            return
        }
        connect()
    }

    /// The app came to the front (or was pulled to refresh): connect if needed.
    func wake() {
        endBackgroundTask()
        LANKeepAlive.shared.stop()
        guard pairing != nil else { return }
        switch state {
        case .connected:
            send(["type": "ping"])
        case .connecting:
            break
        case .rejected:
            break
        case .unpaired, .offline:
            failures = 0
            connect()
        }
    }

    /// The app went to the background: iOS gives it a little time, then
    /// suspends it, and the connection goes with it.
    func enterBackground() {
        // Kept alive (LANKeepAlive): the link stays open and approvals keep arriving.
        if pairing != nil, state != .rejected, LANKeepAlive.shared.enabled {
            LANKeepAlive.shared.start()
            if LANKeepAlive.shared.running { return }
        }
        guard backgroundTask == .invalid, state == .connected else { return }
        // iOS calls this on the main thread, and the task must end before it returns.
        backgroundTask = UIApplication.shared.beginBackgroundTask(withName: "Coucou link") {
            MainActor.assumeIsolated { LANLink.shared.backgroundTimeEnded() }
        }
    }

    private func backgroundTimeEnded() {
        disconnect(polite: true)
        if pairing != nil, state != .rejected {
            state = .offline("Paused while Coucou is in the background")
            publish()
        }
        endBackgroundTask()
    }

    private func endBackgroundTask() {
        guard backgroundTask != .invalid else { return }
        UIApplication.shared.endBackgroundTask(backgroundTask)
        backgroundTask = .invalid
    }

    func connect(to endpoint: NWEndpoint? = nil) {
        guard let pairing else { return }
        retryTask?.cancel()
        disconnect()
        generation += 1
        let current = generation
        state = .connecting
        publish()
        // Every other try goes to the away address, when there is one.
        let away = pairing.awayHost.flatMap { $0.isEmpty ? nil : $0 }
        let useAway = away != nil && ((failures % 2 == 1) != preferAway)
        let host = useAway ? away! : pairing.host
        let target = endpoint ?? .hostPort(host: NWEndpoint.Host(host),
                                           port: NWEndpoint.Port(rawValue: pairing.port) ?? 47821)
        let socket = LANSocket(endpoint: target, fingerprint: pairing.fingerprint) { event in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { LANLink.shared.handle(event, generation: current) }
            }
        }
        self.socket = socket
        socket.start()
    }

    private func disconnect(polite: Bool = false) {
        pingTask?.cancel()
        pingTask = nil
        if let socket {
            if polite { send(["type": "bye"], on: socket) }
            socket.close()
        }
        socket = nil
        generation += 1
    }

    private func handle(_ event: LANSocket.Event, generation: Int) {
        guard generation == self.generation else { return }
        switch event {
        case .ready(let endpoint):
            remember(endpoint)
            var hello: [String: Any] = ["type": "hello", "v": Self.protocolVersion,
                                        "token": pairing?.token ?? "", "device": UIDevice.current.name]
            hello["caps"] = Self.askedCaps
            send(hello)
        case .line(let data):
            lastHeard = .now
            guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let type = object["type"] as? String else { return }
            receive(type, object)
        case .closed(let reason):
            if UIApplication.shared.applicationState != .active { BackgroundLog.add("Link closed: \(reason)") }
            socket = nil
            pingTask?.cancel()
            pingTask = nil
            approvals = [:]
            questions = [:]
            rebuild()
            LANChat.shared.linkClosed()
            guard state != .rejected, pairing != nil else { return }
            if UIApplication.shared.applicationState == .background, !LANKeepAlive.shared.running {
                state = .offline("Paused while Coucou is in the background")
                publish()
                return
            }
            failures += 1
            state = .offline(describe(reason))
            publish()
            // The computer's address may have changed (a new IP from the router): look for it.
            if !searchedSinceWelcome || reason == "certificate" {
                searchedSinceWelcome = true
                search()
            } else {
                scheduleRetry()
            }
        }
    }

    private func describe(_ reason: String) -> String {
        if reason == "certificate" { return "A different computer answered at the saved address" }
        if reason.contains("Connection refused") || reason.contains("61") {
            return "The computer refused the connection. Is the phone link on in Coucou?"
        }
        if reason.contains("timed out") || reason.contains("60") {
            return "No answer from the computer. Same Wi-Fi? Is it awake?"
        }
        return "Can't reach the computer"
    }

    private func scheduleRetry() {
        let delays: [Double] = [2, 5, 10, 30, 60]
        let delay = delays[min(failures - 1, delays.count - 1)]
        retryTask?.cancel()
        retryTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(delay))
            guard !Task.isCancelled else { return }
            self?.connect()
        }
    }

    /// The address that worked is saved, so the next launch goes straight there.
    private func remember(_ endpoint: NWEndpoint?) {
        guard var updated = pairing, case .hostPort(let host, let port)? = endpoint else { return }
        let text: String
        switch host {
        case .ipv4(let address): text = "\(address)"
        default: return
        }
        let cleaned = text.split(separator: "%").first.map(String.init) ?? text
        // Reached through the away address: the home one stays as it was.
        preferAway = cleaned == updated.awayHost
        UserDefaults.standard.set(preferAway, forKey: "lanPreferAway")
        guard cleaned != updated.awayHost else { return }
        guard cleaned != updated.host || port.rawValue != updated.port else { return }
        updated.host = cleaned
        updated.port = port.rawValue
        LANPairingStore.save(updated)
        pairing = updated
    }

    // MARK: Finding the computer again (DNS-SD, `_coucou._tcp`)

    private func search() {
        guard let pairing else { return }
        browser?.cancel()
        let shortId = String(pairing.fingerprint.prefix(16))
        let browser = NWBrowser(for: .bonjourWithTXTRecord(type: "_coucou._tcp", domain: nil), using: .tcp)
        self.browser = browser
        let current = generation
        browser.browseResultsChangedHandler = { results, _ in
            let match = results.first { result in
                guard case .bonjour(let txt) = result.metadata else { return false }
                let version = txt["v"]
                return txt["fp"]?.lowercased() == shortId && (version == nil || version == "1")
            }
            guard let endpoint = match?.endpoint else { return }
            DispatchQueue.main.async {
                MainActor.assumeIsolated { LANLink.shared.found(endpoint, generation: current) }
            }
        }
        browser.stateUpdateHandler = { state in
            if case .failed = state {
                DispatchQueue.main.async {
                    MainActor.assumeIsolated { LANLink.shared.searchEnded(generation: current) }
                }
            }
        }
        browser.start(queue: .main)
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(8))
            self?.searchEnded(generation: current)
        }
    }

    private func found(_ endpoint: NWEndpoint, generation: Int) {
        guard generation == self.generation, browser != nil else { return }
        browser?.cancel()
        browser = nil
        // The full certificate is still checked when connecting: the short id is only a hint.
        connect(to: endpoint)
    }

    private func searchEnded(generation: Int) {
        guard generation == self.generation, let browser else { return }
        browser.cancel()
        self.browser = nil
        scheduleRetry()
    }

    // MARK: Messages from the computer

    private func receive(_ type: String, _ message: [String: Any]) {
        switch type {
        case "welcome":
            guard (message["v"] as? Int) == Self.protocolVersion else {
                state = .offline("This computer speaks a different version of Coucou")
                disconnect()
                publish()
                return
            }
            desktopName = message["desktop"] as? String ?? pairing?.name ?? ""
            offeredCaps = Set(message["caps"] as? [String] ?? [])
            if UIApplication.shared.applicationState != .active, state != .connected { BackgroundLog.add("Link back") }
            state = .connected
            failures = 0
            searchedSinceWelcome = false
            startPinging()
            publish()
            if offeredCaps.contains("chat") { send(["type": "chatModels"]) }
        case "error":
            if message["code"] as? String == "auth" {
                state = .rejected
                retryTask?.cancel()
                disconnect()
                publish()
            }
        case "sessions":
            let list = message["sessions"] as? [[String: Any]] ?? []
            sessions = list.compactMap(Self.session)
            rebuild()
        case "approval":
            guard let approval = Self.approval(message) else { return }
            approvals[approval.fingerprint] = approval
            rebuild()
            notify(approval)
        case "question":
            guard let question = Self.question(message) else { return }
            questions[question.fingerprint] = question
            rebuild()
        case "approvalResolved":
            guard let fingerprint = message["fingerprint"] as? String else { return }
            approvals[fingerprint] = nil
            questions[fingerprint] = nil
            UNUserNotificationCenter.current().removeDeliveredNotifications(
                withIdentifiers: [PhoneLink.approvalNotificationID(fingerprint)])
            rebuild()
        case "chatModels":
            LANChat.shared.received(models: message["models"] as? [[String: Any]] ?? [])
        case "chatDelta":
            guard let id = message["id"] as? String, let text = message["text"] as? String else { return }
            LANChat.shared.delta(id: id, text: text)
        case "chatDone":
            guard let id = message["id"] as? String else { return }
            LANChat.shared.done(id: id, text: message["text"] as? String)
        case "chatError":
            guard let id = message["id"] as? String else { return }
            LANChat.shared.failed(id: id, message: message["message"] as? String ?? "The computer could not answer.")
        default:
            break   // pong, prefs and anything newer
        }
    }

    private static func session(_ raw: [String: Any]) -> LANSession? {
        guard let pillId = raw["pillId"] as? String, !pillId.isEmpty else { return nil }
        let millis = (raw["updatedAt"] as? NSNumber)?.doubleValue ?? Date().timeIntervalSince1970 * 1000
        return LANSession(pillId: pillId,
                          agent: raw["agent"] as? String ?? "",
                          state: raw["state"] as? String ?? "idle",
                          statusText: raw["statusText"] as? String ?? "",
                          updatedAt: Date(timeIntervalSince1970: millis / 1000),
                          steps: (raw["steps"] as? [String] ?? []).map { String($0.prefix(200)) },
                          finalLine: String((raw["finalLine"] as? String ?? "").prefix(200)),
                          project: String((raw["project"] as? String ?? "").prefix(64)),
                          color: raw["color"] as? String ?? "")
    }

    private static func approval(_ raw: [String: Any]) -> LANApproval? {
        guard let pillId = raw["pillId"] as? String, let fingerprint = raw["fingerprint"] as? String,
              !fingerprint.isEmpty else { return nil }
        let millis = (raw["createdAt"] as? NSNumber)?.doubleValue ?? Date().timeIntervalSince1970 * 1000
        return LANApproval(pillId: pillId, fingerprint: fingerprint,
                           tool: raw["tool"] as? String ?? "",
                           command: raw["command"] as? String ?? "",
                           createdAt: Date(timeIntervalSince1970: millis / 1000),
                           truncated: raw["truncated"] as? Bool ?? false)
    }

    private static func question(_ raw: [String: Any]) -> LANQuestion? {
        guard let pillId = raw["pillId"] as? String, let fingerprint = raw["fingerprint"] as? String,
              let list = raw["questions"] as? [[String: Any]], !list.isEmpty else { return nil }
        let items: [QuestionPayload.Item] = list.map { item in
            let options = (item["options"] as? [[String: Any]] ?? []).map {
                QuestionPayload.Option(label: $0["label"] as? String ?? "",
                                       description: $0["description"] as? String ?? "")
            }
            return QuestionPayload.Item(question: item["question"] as? String ?? "",
                                        header: item["header"] as? String ?? "",
                                        options: options,
                                        multiSelect: item["multiSelect"] as? Bool ?? false)
        }
        let millis = (raw["createdAt"] as? NSNumber)?.doubleValue ?? Date().timeIntervalSince1970 * 1000
        return LANQuestion(pillId: pillId, fingerprint: fingerprint,
                           createdAt: Date(timeIntervalSince1970: millis / 1000),
                           payload: QuestionPayload(items: items))
    }

    // MARK: Messages to the computer

    @discardableResult
    private func send(_ message: [String: Any], on target: LANSocket? = nil) -> Bool {
        guard let socket = target ?? socket,
              let data = try? JSONSerialization.data(withJSONObject: message) else { return false }
        socket.send(data)
        return true
    }

    /// Sends once connected, connecting first if needed (an action from a
    /// notification can wake the app after iOS closed the connection).
    func sendWhenConnected(_ message: [String: Any]) async -> Bool {
        if state != .connected {
            guard pairing != nil, state != .rejected else { return false }
            if state != .connecting { connect() }
            for _ in 0..<40 where state != .connected {
                try? await Task.sleep(for: .milliseconds(200))
                if state == .rejected { return false }
            }
        }
        guard state == .connected else { return false }
        return send(message)
    }

    /// A chat message, only to a computer that offered chat on this connection.
    func sendChat(_ message: [String: Any]) {
        guard state == .connected, offeredCaps.contains("chat") else { return }
        send(message)
    }

    func decide(_ decision: Decision, fingerprint: String) async -> Bool {
        await sendWhenConnected(["type": "decision", "fingerprint": fingerprint, "decision": decision.rawValue])
    }

    func answer(fingerprint: String, picks: [[String]]) async -> Bool {
        guard offeredCaps.contains("answers") else { return false }
        return await sendWhenConnected(["type": "answer", "fingerprint": fingerprint, "picks": picks])
    }

    private func startPinging() {
        pingTask?.cancel()
        lastHeard = .now
        pingTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(20))
                guard let self, !Task.isCancelled, self.state == .connected else { return }
                if Date().timeIntervalSince(self.lastHeard) > 50 {
                    // Silent for too long: start again.
                    self.connect()
                    return
                }
                self.send(["type": "ping"])
            }
        }
    }

    // MARK: Handing it to the app

    private func resetContent() {
        sessions = []
        approvals = [:]
        questions = [:]
        rebuild()
    }

    /// The sessions as the rest of the app knows them (SessionItem), with the
    /// pending approval or question of each.
    private func rebuild() {
        var list = sessions
        // An approval or question for a session not in the list yet still shows.
        for pillId in Set(approvals.values.map(\.pillId)).union(questions.values.map(\.pillId))
        where !list.contains(where: { $0.pillId == pillId }) {
            list.append(LANSession(pillId: pillId, agent: "", state: "approval", statusText: "", updatedAt: .now,
                                   steps: [], finalLine: "", project: "", color: ""))
        }
        let name = desktopName.isEmpty ? (pairing?.name ?? "") : desktopName
        let items = list.map { session -> SessionItem in
            let approval = approvals.values.filter { $0.pillId == session.pillId }.max { $0.createdAt < $1.createdAt }
            let question = questions.values.filter { $0.pillId == session.pillId }.max { $0.createdAt < $1.createdAt }
            return SessionItem(lan: session, approval: approval, question: question, computer: name)
        }
        let sessions = items.filter { PillCatalog.isSession($0.id) }
        PhoneLink.shared.applyLAN(sessions)
        LANLiveActivity.shared.update(sessions)
    }

    private func publish() {
        PhoneLink.shared.lanStateChanged(state)
    }

    /// A banner for a new approval while Coucou is not on screen.
    private func notify(_ approval: LANApproval) {
        guard notified.insert(approval.fingerprint).inserted,
              UIApplication.shared.applicationState != .active else { return }
        let pillName = PillCatalog.definition(for: approval.pillId)?.name ?? approval.pillId
        let content = UNMutableNotificationContent()
        content.title = pillName
        content.body = approval.command.isEmpty
            ? "An agent is waiting for your OK"
            : "Waiting for your OK: \(approval.command.prefix(140))"
        content.sound = PhoneSettings.sound("approval")
        content.categoryIdentifier = NotificationActions.approvalCategory
        content.userInfo = ["fingerprint": approval.fingerprint, "pillId": approval.pillId]
        content.threadIdentifier = approval.pillId
        let request = UNNotificationRequest(identifier: PhoneLink.approvalNotificationID(approval.fingerprint),
                                            content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }
}

extension SessionItem {
    init(lan session: LANSession, approval: LANApproval?, question: LANQuestion?, computer: String) {
        id = session.pillId
        name = session.project
        let catalogColor = PillCatalog.definition(for: session.pillId)?.defaultColor ?? "#C0C4CC"
        color = session.color.hasPrefix("#") && session.color.count == 7 ? session.color : catalogColor
        var state = BotState(rawValue: session.state) ?? .idle
        if approval != nil { state = .approval } else if question != nil { state = .question }
        self.state = state
        var steps = session.steps
        if steps.isEmpty, !session.statusText.isEmpty { steps = [session.statusText] }
        self.steps = steps
        stepIndex = max(0, steps.count - 1)
        needsApproval = approval != nil
        if let approval {
            approvalCommand = approval.truncated
                ? approval.command + "…\n(Longer than shown: it can only be allowed on the computer.)"
                : approval.command
        } else {
            approvalCommand = ""
        }
        approvalFingerprint = approval?.fingerprint ?? ""
        acceptsInstructions = false
        self.question = question?.payload.items.first?.question ?? ""
        questionPayload = question?.payload
        questionFingerprint = question?.fingerprint ?? ""
        finalLine = session.finalLine
        cwd = session.project
        updatedAt = max(session.updatedAt, approval?.createdAt ?? .distantPast, question?.createdAt ?? .distantPast)
        macName = computer
    }
}
#endif
