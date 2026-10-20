#if LAN_LINK
import SwiftUI

/// A card after an update, the same as on the computer: once per new build, a friend's project
/// (Custom UI, a macOS-style shell for Windows 11 by sergey1029). Nothing is fetched: the words and
/// pictures are in the app, and the link opens only on a tap. The first start after installing shows nothing.
struct LANPromoSeen: Equatable {
    var lastBuild = 0
    var due = false
    var never = false

    /// What a start of `build` does to what was kept.
    func started(_ build: Int) -> LANPromoSeen {
        guard build > 0 else { return self }
        var s = self
        if lastBuild != 0 && build != lastBuild { s.due = true }
        s.lastBuild = build
        return s
    }

    var shown: Bool { due && !never }
}

@MainActor @Observable
final class LANPromo {
    static let shared = LANPromo()
    static let url = URL(string: "https://github.com/sergey1029/custom-ui")!
    private let defaults = UserDefaults.standard
    var due = false

    private init() {
        // The fork's build number (set by the all-in-one workflow); CFBundleVersion only moves with App Store builds.
        let info = Bundle.main.infoDictionary
        let build = (info?["CoucouForkBuild"] as? Int) ?? Int(info?["CFBundleVersion"] as? String ?? "") ?? 0
        let s = load().started(build)
        save(s)
        due = s.shown
    }

    private func load() -> LANPromoSeen {
        LANPromoSeen(lastBuild: defaults.integer(forKey: "promo.build"), due: defaults.bool(forKey: "promo.due"), never: defaults.bool(forKey: "promo.never"))
    }

    private func save(_ s: LANPromoSeen) {
        defaults.set(s.lastBuild, forKey: "promo.build")
        defaults.set(s.due, forKey: "promo.due")
        defaults.set(s.never, forKey: "promo.never")
    }

    /// Shown (or put away); `never`: not after any update again.
    func done(never: Bool) {
        var s = load()
        s.due = false
        if never { s.never = true }
        save(s)
        due = false
    }
}

struct LANPromoSheet: View {
    @Environment(\.openURL) private var openURL
    @State private var more = false
    @State private var big = false
    let onDone: (Bool) -> Void

    private static let details: [LocalizedStringKey] = [
        "A menu bar with the app's menus, tray icons and a clock",
        "A Dock with your apps, growing under the pointer",
        "Spotlight: apps, files, the web and a calculator (Win or Alt+Space)",
        "Launchpad, a Control Center, a calendar and widgets",
        "Your windows stay as they are: only what is around them changes",
        "Free and open source (MIT), no admin rights to install. Not made by or linked to Apple.",
    ]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 12) {
                    Image("PromoCustomUI").resizable().frame(width: 44, height: 44).clipShape(RoundedRectangle(cornerRadius: 10))
                    VStack(alignment: .leading, spacing: 2) {
                        Text("A friend's project", tableName: "Promo").font(.caption).foregroundStyle(.secondary).textCase(.uppercase)
                        Text("Custom UI, by sergey1029", tableName: "Promo").font(.headline)
                    }
                }
                Image("PromoCustomUIShot").resizable().aspectRatio(contentMode: .fit)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(.white.opacity(0.12)))
                    .onTapGesture { withAnimation { big.toggle() } }
                    .accessibilityLabel(Text("What Custom UI looks like", tableName: "Promo"))
                if !big {
                    if more {
                        VStack(alignment: .leading, spacing: 6) {
                            ForEach(Self.details.indices, id: \.self) { i in
                                Label { Text(Self.details[i], tableName: "Promo") } icon: { Text("•") }
                                    .font(.subheadline).foregroundStyle(.secondary)
                            }
                        }
                    } else {
                        Text("Windows 11 in the style of macOS: a menu bar, a Dock, Spotlight and Launchpad. For your computer.", tableName: "Promo")
                            .font(.subheadline).foregroundStyle(.secondary)
                    }
                }
                HStack(spacing: 8) {
                    if !more {
                        Button { withAnimation { more = true } } label: { Text("More", tableName: "Promo").frame(maxWidth: .infinity) }
                            .buttonStyle(.bordered)
                    }
                    Button { onDone(false) } label: { Text("Later", tableName: "Promo").frame(maxWidth: .infinity) }
                        .buttonStyle(.bordered)
                    Button { openURL(LANPromo.url); onDone(false) } label: { Text("Open on GitHub", tableName: "Promo").frame(maxWidth: .infinity) }
                        .buttonStyle(.borderedProminent)
                }
                .controlSize(.large)
                Button { onDone(true) } label: { Text("Don't show", tableName: "Promo").font(.subheadline) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).frame(maxWidth: .infinity)
            }
            .padding(20)
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }
}
#endif
