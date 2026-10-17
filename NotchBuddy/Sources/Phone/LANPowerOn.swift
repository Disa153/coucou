#if LAN_LINK
import CryptoKit
import Darwin
import Foundation
import Observation
import Security

// Switching the computer on from the iPhone (Wake-on-LAN). While the computer
// is on, Coucou for Windows/Linux sends its wired card's MAC address in the
// welcome (cap "wake"); the iPhone keeps it. When the computer is off, nothing
// on it can answer, so the iPhone sends the "magic packet" itself, and also
// asks the router to send it when a Keenetic router is set up here: iOS may
// keep a sideloaded app from broadcasting on the network, the router never
// does. The router's password lives in this iPhone's Keychain only.

/// The computer's card and network, as the computer last told us.
struct LANWakeTarget: Codable, Equatable, Sendable {
    var mac: String
    var ip: String
    var broadcast: String

    /// From the welcome's "wake" object.
    static func from(_ object: Any?) -> LANWakeTarget? {
        guard let o = object as? [String: Any],
              let mac = o["mac"] as? String, LANWakeTarget.bytes(of: mac) != nil else { return nil }
        return LANWakeTarget(mac: mac.lowercased(), ip: o["ip"] as? String ?? "", broadcast: o["broadcast"] as? String ?? "")
    }

    /// "aa:bb:cc:dd:ee:ff" (or with dashes) → its six bytes.
    static func bytes(of mac: String) -> [UInt8]? {
        let hex = mac.filter { $0 != ":" && $0 != "-" && $0 != "." && !$0.isWhitespace }
        guard hex.count == 12 else { return nil }
        var out: [UInt8] = []
        var index = hex.startIndex
        for _ in 0..<6 {
            let next = hex.index(index, offsetBy: 2)
            guard let byte = UInt8(hex[index..<next], radix: 16) else { return nil }
            out.append(byte)
            index = next
        }
        return out.allSatisfy({ $0 == 0 }) ? nil : out
    }

    /// Written the router's way.
    var colonMac: String {
        (LANWakeTarget.bytes(of: mac) ?? []).map { String(format: "%02x", $0) }.joined(separator: ":")
    }
}

enum LANWakeStore {
    private static let key = "lanWakeTarget"

    static func load() -> LANWakeTarget? {
        guard let data = UserDefaults.standard.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(LANWakeTarget.self, from: data)
    }

    static func save(_ target: LANWakeTarget?) {
        if let target, let data = try? JSONEncoder().encode(target) {
            UserDefaults.standard.set(data, forKey: key)
        } else {
            UserDefaults.standard.removeObject(forKey: key)
        }
    }
}

/// The Keenetic router: its address and login in the preferences, its password in the Keychain.
struct RouterLogin: Equatable, Sendable {
    var address: String
    /// Away from home: the router's KeenDNS name (https://….keenetic.link), empty if none.
    var awayAddress: String = ""
    var login: String
    var password: String

    var isComplete: Bool { (!address.isEmpty || !awayAddress.isEmpty) && !login.isEmpty && !password.isEmpty }

    /// The addresses to try, home first.
    var addresses: [String] {
        [address, awayAddress].map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
    }
}

enum RouterStore {
    private static let service = "fr.louisraille.Coucou.router"
    private static let account = "keenetic"

    private static var baseQuery: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    static func load() -> RouterLogin {
        let d = UserDefaults.standard
        var query = baseQuery
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        var password = ""
        if SecItemCopyMatching(query as CFDictionary, &out) == errSecSuccess, let data = out as? Data {
            password = String(decoding: data, as: UTF8.self)
        }
        return RouterLogin(address: d.string(forKey: "routerAddress") ?? "192.168.1.1",
                           awayAddress: d.string(forKey: "routerAwayAddress") ?? "",
                           login: d.string(forKey: "routerLogin") ?? "",
                           password: password)
    }

    static func save(_ login: RouterLogin) {
        let d = UserDefaults.standard
        d.set(login.address, forKey: "routerAddress")
        d.set(login.awayAddress, forKey: "routerAwayAddress")
        d.set(login.login, forKey: "routerLogin")
        SecItemDelete(baseQuery as CFDictionary)
        guard !login.password.isEmpty else { return }
        var item = baseQuery
        item[kSecValueData as String] = Data(login.password.utf8)
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }
}

enum RouterError: LocalizedError {
    case unreachable(String)
    case notKeenetic
    case badLogin
    case refused(String)

    var errorDescription: String? {
        switch self {
        case .unreachable(let why): "The router did not answer (\(why))."
        case .notKeenetic: "That address doesn't answer like a Keenetic router."
        case .badLogin: "The router refused this login or password."
        case .refused(let why): "The router refused: \(why)"
        }
    }
}

/// Talks to a Keenetic router's own interface (RCI) on the local network.
enum Keenetic {
    private static func base(_ address: String) -> String {
        var a = address.trimmingCharacters(in: .whitespacesAndNewlines)
        if !a.lowercased().hasPrefix("http://") && !a.lowercased().hasPrefix("https://") {
            // The router at home answers plain HTTP on its local address; a KeenDNS name is HTTPS.
            let host = a.split(separator: "/").first.map(String.init) ?? a
            let local = host.allSatisfy { $0.isNumber || $0 == "." || $0 == ":" } || host.hasSuffix(".local") || !host.contains(".")
            a = (local ? "http://" : "https://") + a
        }
        while a.hasSuffix("/") { a.removeLast() }
        return a
    }

    private static func hex<D: Sequence>(_ digest: D) -> String where D.Element == UInt8 {
        digest.map { String(format: "%02x", $0) }.joined()
    }

    private static func session() -> URLSession {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 8
        config.httpCookieAcceptPolicy = .always
        return URLSession(configuration: config)
    }

    private static func send(_ session: URLSession, _ request: URLRequest) async throws -> (Data, Int, HTTPURLResponse?) {
        do {
            let (data, response) = try await session.data(for: request)
            let http = response as? HTTPURLResponse
            return (data, http?.statusCode ?? 0, http)
        } catch {
            throw RouterError.unreachable(error.localizedDescription)
        }
    }

    /// Signs in with the router's challenge: SHA-256(challenge + MD5(login:realm:password)).
    static func signIn(_ login: RouterLogin, at address: String, session: URLSession) async throws {
        guard let url = URL(string: base(address) + "/auth") else { throw RouterError.notKeenetic }
        let (_, status, response) = try await send(session, URLRequest(url: url))
        if status == 200 { return }
        guard status == 401,
              let realm = response?.value(forHTTPHeaderField: "X-NDM-Realm"),
              let challenge = response?.value(forHTTPHeaderField: "X-NDM-Challenge") else { throw RouterError.notKeenetic }
        let md5 = hex(Insecure.MD5.hash(data: Data("\(login.login):\(realm):\(login.password)".utf8)))
        let sha = hex(SHA256.hash(data: Data((challenge + md5).utf8)))
        var post = URLRequest(url: url)
        post.httpMethod = "POST"
        post.setValue("application/json", forHTTPHeaderField: "Content-Type")
        post.httpBody = try JSONSerialization.data(withJSONObject: ["login": login.login, "password": sha])
        let (_, signed, _) = try await send(session, post)
        guard signed == 200 else { throw RouterError.badLogin }
    }

    /// Tries each address, home first; the first that answers is used. Returns which one.
    private static func firstAnswering<T>(_ login: RouterLogin, _ body: (String) async throws -> T) async throws -> T {
        var last: Error = RouterError.unreachable("no address")
        for address in login.addresses {
            do {
                return try await body(address)
            } catch RouterError.unreachable(let why) {
                last = RouterError.unreachable(why)
            }
        }
        throw last
    }

    /// Checks the addresses, login and password. Returns the address that answered.
    static func test(_ login: RouterLogin) async throws -> String {
        try await firstAnswering(login) { address in
            try await signIn(login, at: address, session: session())
            return address
        }
    }

    /// The computer's wired card, from the router's list of devices (`show ip hotspot`), found by its name.
    static func findMac(_ login: RouterLogin, computer: String) async throws -> String? {
        try await firstAnswering(login) { address in
            let session = session()
            try await signIn(login, at: address, session: session)
            guard let url = URL(string: base(address) + "/rci/show/ip/hotspot") else { throw RouterError.notKeenetic }
            let (data, status, _) = try await send(session, URLRequest(url: url))
            guard status == 200 else { throw RouterError.refused("HTTP \(status)") }
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            let hosts = object?["host"] as? [[String: Any]] ?? []
            return pickMac(hosts, computer: computer)
        }
    }

    /// The host named like the computer; its wired entry (no Wi-Fi network) first, then an active one.
    static func pickMac(_ hosts: [[String: Any]], computer: String) -> String? {
        let name = computer.lowercased()
        guard !name.isEmpty else { return nil }
        let named = hosts.filter { host in
            [host["hostname"], host["name"]].contains { ($0 as? String)?.lowercased() == name }
        }
        let ranked = named.sorted { a, b in
            let wiredA = a["ssid"] == nil, wiredB = b["ssid"] == nil
            if wiredA != wiredB { return wiredA }
            return (a["active"] as? Bool ?? false) && !(b["active"] as? Bool ?? false)
        }
        return ranked.compactMap { $0["mac"] as? String }.first { LANWakeTarget.bytes(of: $0) != nil }
    }

    /// The router sends the magic packet itself (`ip hotspot wake <mac>`), reached at home or through KeenDNS.
    static func wake(_ login: RouterLogin, mac: String) async throws {
        try await firstAnswering(login) { address in
            try await wake(login, at: address, mac: mac)
        }
    }

    private static func wake(_ login: RouterLogin, at address: String, mac: String) async throws {
        let session = session()
        try await signIn(login, at: address, session: session)
        guard let url = URL(string: base(address) + "/rci/") else { throw RouterError.notKeenetic }
        var post = URLRequest(url: url)
        post.httpMethod = "POST"
        post.setValue("application/json", forHTTPHeaderField: "Content-Type")
        post.httpBody = try JSONSerialization.data(withJSONObject: ["ip": ["hotspot": ["wake": ["mac": mac]]]])
        let (data, status, _) = try await send(session, post)
        guard status == 200 else { throw RouterError.refused("HTTP \(status)") }
        // A refused command still answers 200, with a status of type "error" inside.
        let text = String(decoding: data, as: UTF8.self)
        if text.contains("\"error\"") {
            let message = (try? JSONSerialization.jsonObject(with: data)).flatMap(firstMessage) ?? text
            throw RouterError.refused(String(message.prefix(160)))
        }
    }

    private static func firstMessage(_ object: Any) -> String? {
        if let d = object as? [String: Any] {
            if d["status"] as? String == "error", let m = d["message"] as? String { return m }
            for value in d.values { if let m = firstMessage(value) { return m } }
        } else if let a = object as? [Any] {
            for value in a { if let m = firstMessage(value) { return m } }
        }
        return nil
    }
}

/// The magic packet: six 0xFF, then the MAC address sixteen times.
enum MagicPacket {
    static func packet(_ mac: [UInt8]) -> [UInt8] {
        [UInt8](repeating: 0xFF, count: 6) + Array([[UInt8]](repeating: mac, count: 16).joined())
    }

    /// Sends it to each address on the usual ports. Returns how many went out, and the last error.
    static func send(_ mac: [UInt8], to hosts: [String]) -> (sent: Int, error: String?) {
        let fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)
        guard fd >= 0 else { return (0, String(cString: strerror(errno))) }
        defer { close(fd) }
        var on: Int32 = 1
        _ = setsockopt(fd, SOL_SOCKET, SO_BROADCAST, &on, socklen_t(MemoryLayout<Int32>.size))
        let bytes = packet(mac)
        var sent = 0
        var lastError: String?
        for host in hosts where !host.isEmpty {
            for port in [9, 7] as [UInt16] {
                var addr = sockaddr_in()
                addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
                addr.sin_family = sa_family_t(AF_INET)
                addr.sin_port = port.bigEndian
                guard inet_pton(AF_INET, host, &addr.sin_addr) == 1 else { continue }
                let result = bytes.withUnsafeBytes { buffer in
                    withUnsafePointer(to: &addr) { pointer in
                        pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                            sendto(fd, buffer.baseAddress, buffer.count, 0, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
                        }
                    }
                }
                if result >= 0 { sent += 1 } else { lastError = String(cString: strerror(errno)) }
            }
        }
        return (sent, lastError)
    }
}

@MainActor
@Observable
final class LANPowerOn {
    static let shared = LANPowerOn()

    enum Phase: Equatable {
        case idle
        case sending
        /// Sent: waiting for the computer to come back on the link.
        case waiting(String)
        case on
        case failed(String)
    }

    private(set) var phase: Phase = .idle
    private(set) var target: LANWakeTarget? = LANWakeStore.load()
    var router: RouterLogin = RouterStore.load()

    @ObservationIgnored private var watch: Task<Void, Never>?

    /// The computer told us its card (welcome, cap "wake").
    func learned(_ new: LANWakeTarget) {
        guard new != target else { return }
        target = new
        LANWakeStore.save(new)
    }

    /// Typed in by hand (a computer whose Coucou can't read its card).
    func setMac(_ text: String) -> Bool {
        guard LANWakeTarget.bytes(of: text) != nil else { return false }
        var t = target ?? LANWakeTarget(mac: "", ip: "", broadcast: "")
        t.mac = text.lowercased()
        target = t
        LANWakeStore.save(t)
        return true
    }

    var routerReady: Bool { router.isComplete }

    func saveRouter() { RouterStore.save(router) }

    func testRouter() async -> String {
        RouterStore.save(router)
        do {
            let address = try await Keenetic.test(router)
            var note = "The router accepted this login (\(address))."
            if target == nil {
                note += await learnFromRouter() ? " It also told Coucou the computer's network card." : ""
            }
            return note
        } catch {
            return error.localizedDescription
        }
    }

    /// Asks the router for the computer's card, by the computer's name. True when found.
    @discardableResult
    func learnFromRouter() async -> Bool {
        let computer = LANLink.shared.desktopName.isEmpty ? (LANLink.shared.pairing?.name ?? "") : LANLink.shared.desktopName
        guard router.isComplete, !computer.isEmpty,
              let mac = try? await Keenetic.findMac(router, computer: computer) else { return false }
        var t = target ?? LANWakeTarget(mac: "", ip: "", broadcast: "")
        t.mac = mac.lowercased()
        target = t
        LANWakeStore.save(t)
        BackgroundLog.add("Wake: network card from the router")
        return true
    }

    /// The button: the packet from here, and from the router when it is set up.
    func powerOn() {
        guard let target, LANWakeTarget.bytes(of: target.mac) != nil else {
            // Not told by the computer yet: the router may know it.
            guard router.isComplete else {
                phase = .failed("Connect once while the computer is on, so Coucou learns its network card.")
                return
            }
            phase = .sending
            Task {
                if await learnFromRouter() {
                    powerOn()
                } else {
                    phase = .failed("Neither the computer nor the router told Coucou the network card yet. Enter it by hand below.")
                }
            }
            return
        }
        guard let mac = LANWakeTarget.bytes(of: target.mac) else { return }
        phase = .sending
        let router = self.router
        let hosts = [target.broadcast, "255.255.255.255", target.ip]
        Task {
            let direct = await Task.detached { MagicPacket.send(mac, to: hosts) }.value
            var routerResult: String?
            var routerOK = false
            if router.isComplete {
                do {
                    try await Keenetic.wake(router, mac: target.colonMac)
                    routerOK = true
                } catch {
                    routerResult = error.localizedDescription
                }
            }
            if routerOK || direct.sent > 0 {
                let how = routerOK ? "The router sent the signal." : "Signal sent from this iPhone."
                BackgroundLog.add("Wake: \(how)")
                phase = .waiting(how + (routerResult.map { " \($0)" } ?? ""))
                waitForComputer()
            } else {
                phase = .failed(routerResult ?? "iOS did not let the signal out (\(direct.error ?? "no network")). Set up the router below.")
            }
        }
    }

    /// Tries the link every 10 s for 3 minutes, until the computer answers.
    private func waitForComputer() {
        watch?.cancel()
        watch = Task { [weak self] in
            for _ in 0..<18 {
                try? await Task.sleep(for: .seconds(10))
                guard let self, !Task.isCancelled else { return }
                if LANLink.shared.state == .connected {
                    self.phase = .on
                    return
                }
                LANLink.shared.retryNow()
            }
            guard let self, !Task.isCancelled else { return }
            if case .waiting = self.phase {
                self.phase = .failed("The computer didn't come back within 3 minutes. Check Wake-on-LAN in its BIOS and network card.")
            }
        }
    }

    func linkConnected() {
        if case .waiting = phase { phase = .on }
    }
}
#endif
