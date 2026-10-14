#if LAN_LINK
import ActivityKit
import Foundation
import UIKit

// Mochi on the Lock Screen and in the Dynamic Island, linked to Coucou for Windows.
//
// The iCloud build has the Mac start and update the Live Activity through Apple push.
// The LAN build has no push, so the app runs it itself from what the link reports:
// it is started while Coucou is on screen (iOS only lets an app start one from the
// foreground), then updated from the background, where LANKeepAlive keeps the link
// open. The Allow / Deny buttons (AllowApprovalIntent, DenyApprovalIntent) answer
// through PhoneLink.decide, which goes over the link: no need to open Coucou.

@MainActor
final class LANLiveActivity {
    static let shared = LANLiveActivity()

    private var activity: Activity<MochiActivityAttributes>?
    private var shown: MochiActivityState?
    private var since = Int(Date().timeIntervalSince1970)
    private var latest: [SessionItem] = []

    private init() {
        activity = Activity<MochiActivityAttributes>.activities.first
    }

    var enabled: Bool { ActivityAuthorizationInfo().areActivitiesEnabled }

    /// The sessions changed (LANLink.rebuild).
    func update(_ items: [SessionItem]) {
        latest = items
        apply()
    }

    /// Coucou is on screen: the only moment iOS lets it start an activity.
    func foreground() {
        apply(mayStart: true)
    }

    /// Unpaired or no computer: nothing to show.
    func end() {
        latest = []
        apply()
    }

    private func apply(mayStart: Bool = false) {
        guard enabled else { return }
        guard let state = Self.lead(latest, since: since) else {
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
        if let current = activity, current.activityState == .active || current.activityState == .stale {
            guard state != shown else { return }
            shown = state
            let id = current.id
            Task.detached {
                for a in Activity<MochiActivityAttributes>.activities where a.id == id {
                    await a.update(ActivityContent(state: state, staleDate: nil))
                }
            }
            return
        }
        activity = nil
        guard mayStart || UIApplication.shared.applicationState == .active else { return }
        since = Int(Date().timeIntervalSince1970)
        let fresh = Self.lead(latest, since: since) ?? state
        do {
            activity = try Activity.request(attributes: MochiActivityAttributes(),
                                            content: ActivityContent(state: fresh, staleDate: nil),
                                            pushType: nil)
            shown = fresh
        } catch {
            activity = nil
        }
    }

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
