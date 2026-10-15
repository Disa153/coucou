#if LAN_LINK
import ActivityKit
import Foundation
import Observation
import UIKit

// Mochi on the Lock Screen and in the Dynamic Island, linked to Coucou for Windows.
//
// The iCloud build has the Mac start and update the Live Activity through Apple push.
// The LAN build has no push, so the app runs it itself from what the link reports:
// it is started while Coucou is on screen (iOS only lets an app start one from the
// foreground), then updated from the background, where LANKeepAlive keeps the link
// open. The Allow / Deny buttons (AllowApprovalIntent, DenyApprovalIntent) answer
// through PhoneLink.decide, which goes over the link: no need to open Coucou.
//
// Because iOS never lets it start one from the background, Mochi stays on the Lock
// Screen and in the Dynamic Island the whole time the computer is linked, resting
// when nothing happens. Otherwise an approval that arrives while Coucou is closed
// would find no activity to put its Allow / Deny buttons on.

@MainActor
@Observable
final class LANLiveActivity {
    static let shared = LANLiveActivity()

    @ObservationIgnored private var activity: Activity<MochiActivityAttributes>?
    /// What happened to Mochi's activity last, shown under Live Activities in Settings.
    var diagnosis = "Not started yet: open Coucou with the computer linked"
    private var shown: MochiActivityState?
    private var since = Int(Date().timeIntervalSince1970)
    private var latest: [SessionItem] = []
    /// The computer reported its sessions since the last unpairing.
    private var linked = false

    private init() {
        activity = Activity<MochiActivityAttributes>.activities.first
    }

    var enabled: Bool { ActivityAuthorizationInfo().areActivitiesEnabled }

    /// The widgets extension draws the Live Activity; a signing tool may have left it out.
    static var hasWidgets: Bool {
        guard let dir = Bundle.main.builtInPlugInsURL,
              let names = try? FileManager.default.contentsOfDirectory(atPath: dir.path) else { return false }
        return names.contains { $0.hasSuffix(".appex") && $0.localizedCaseInsensitiveContains("widget") }
    }

    /// The sessions changed (LANLink.rebuild).
    func update(_ items: [SessionItem]) {
        latest = items
        linked = true
        apply()
    }

    /// Coucou is on screen: the only moment iOS lets it start an activity.
    func foreground() {
        apply(mayStart: true)
    }

    /// Unpaired or no computer: nothing to show.
    func end() {
        latest = []
        linked = false
        apply()
    }

    private func apply(mayStart: Bool = false) {
        guard enabled else {
            diagnosis = "Off: turn Live Activities on for Coucou in iPhone Settings"
            return
        }
        guard Self.hasWidgets else {
            diagnosis = "Not shown: this install has no widgets extension (lite .ipa, or the signing tool removed it)"
            return
        }
        guard let state = Self.lead(latest, since: since) ?? (linked ? Self.resting : nil) else {
            if let current = activity {
                activity = nil
                shown = nil
                let id = current.id
                Task.detached {
                    for a in Activity<MochiActivityAttributes>.activities where a.id == id {
                        await a.end(nil, dismissalPolicy: .immediate)
                    }
                }
            }
            return
        }
        // The activity on screen, even if this object lost track of it (the app was relaunched).
        let live = [activity].compactMap { $0 }.first { Self.isLive($0) }
            ?? Activity<MochiActivityAttributes>.activities.first { Self.isLive($0) }
        if let current = live {
            activity = current
            guard state != shown else { return }
            // A new command waiting for your OK lights the screen and opens the Dynamic Island.
            let alert = state.tone == "waiting" && state.approval != nil && state.approval != shown?.approval
            shown = state
            let id = current.id
            // In the background iOS may suspend the app before an update lands: hold it until done.
            let hold = UIApplication.shared.beginBackgroundTask(withName: "Mochi update")
            Task.detached {
                let config = alert
                    ? AlertConfiguration(title: "\(state.agent)", body: "waiting for your OK", sound: .default)
                    : nil
                for a in Activity<MochiActivityAttributes>.activities where a.id == id {
                    await a.update(ActivityContent(state: state, staleDate: nil), alertConfiguration: config)
                }
                await MainActor.run {
                    let time = Date.now.formatted(date: .omitted, time: .standard)
                    LANLiveActivity.shared.diagnosis = "Shown · updated \(time): \(state.agent), \(state.statusText)"
                    if hold != .invalid { UIApplication.shared.endBackgroundTask(hold) }
                }
            }
            return
        }
        activity = nil
        guard mayStart || UIApplication.shared.applicationState == .active else {
            diagnosis = "Waiting: open Coucou once to show Mochi again"
            return
        }
        since = Int(Date().timeIntervalSince1970)
        let fresh = Self.lead(latest, since: since) ?? Self.resting
        do {
            activity = try Activity.request(attributes: MochiActivityAttributes(),
                                            content: ActivityContent(state: fresh, staleDate: nil),
                                            pushType: nil)
            shown = fresh
            diagnosis = "Shown (\(Activity<MochiActivityAttributes>.activities.count) on screen)"
        } catch {
            activity = nil
            diagnosis = "iOS refused to show it: \(error.localizedDescription)"
        }
    }

    private static func isLive(_ a: Activity<MochiActivityAttributes>) -> Bool {
        a.activityState == .active || a.activityState == .stale
    }

    /// Mochi with nothing to do: keeps the activity alive for the next approval.
    static let resting = MochiActivityState(
        pillId: "", agent: "Coucou", color: "#C0C4CC", state: BotState.idle.rawValue,
        statusText: "All quiet", tone: "idle", stepIndex: 0, stepCount: 0, others: 0,
        since: nil, approval: nil)

    /// The most urgent session, as the Mac's LiveActivityRelay.leadState picks it.
    static func lead(_ items: [SessionItem], since: Int) -> MochiActivityState? {
        let ranked = items.map { item -> (SessionItem, Int) in
            (item, MochiActivityState.urgency(state: item.state, waitingForOK: item.needsApproval,
                                              hasQuestion: item.questionPayload != nil || item.state == .question))
        }
        guard let (task, urgency) = ranked.min(by: { $0.1 < $1.1 }) else { return nil }
        let others = ranked.filter { $0.0.id != task.id && $0.1 <= 3 }.count
        return MochiActivityState(
            pillId: task.id,
            agent: PillCatalog.definition(for: task.id)?.name ?? (task.name.isEmpty ? "Agent" : task.name),
            color: task.color,
            state: (urgency == 0 ? BotState.approval : task.state).rawValue,
            statusText: MochiActivityState.statusText(state: task.state, urgency: urgency,
                                                      stepIndex: task.stepIndex, stepCount: task.steps.count),
            tone: MochiActivityState.tone(urgency: urgency),
            stepIndex: max(0, task.stepIndex),
            stepCount: task.steps.count,
            others: others,
            since: since,
            approval: task.needsApproval && !task.approvalFingerprint.isEmpty ? task.approvalFingerprint : nil)
    }
}

#endif
