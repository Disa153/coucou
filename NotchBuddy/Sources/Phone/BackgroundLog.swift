#if LAN_LINK
import Foundation

// What happened while Coucou was in the background: the keep-alive starting and
// stopping, the link to the computer closing and coming back, Mochi's updates.
// The sideloaded build has no push, so this is how to tell why Mochi stopped
// moving. Shown at the bottom of Settings → Your computer; kept on this iPhone only.

enum BackgroundLog {
    private static let key = "lanBackgroundLog"
    private static let keep = 30

    static func add(_ event: String) {
        let time = Date.now.formatted(date: .omitted, time: .standard)
        var lines = entries
        lines.insert("\(time)  \(event)", at: 0)
        UserDefaults.standard.set(Array(lines.prefix(keep)), forKey: key)
    }

    static var entries: [String] {
        UserDefaults.standard.stringArray(forKey: key) ?? []
    }
}
#endif
