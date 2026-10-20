import SwiftUI

// How a session reads on the iPhone: short status line, urgency for sorting,
// and the summary shown in the notch.

extension SessionItem {
    /// Lower = more urgent. Waiting on you first, then errors, then work, then rest.
    var urgency: Int {
        if needsApproval || state == .approval { return 0 }
        if !question.isEmpty || state == .question { return 1 }
        switch state {
        case .error, .ratelimit, .dizzy: return 2
        case .working, .thinking, .searching: return 3
        case .finished: return 4
        default: return 5
        }
    }

    var isWaitingForYou: Bool { urgency <= 1 }
    var isWorking: Bool { urgency == 3 }

    /// What the agent is doing right now ("Bash · npm test", "main.ts"), one short
    /// line, while it works; nil otherwise, so the usual text shows. Same rule as
    /// the overview pills on Windows (windows/src/core/pill-status.ts).
    var liveStep: String? {
        guard isWorking, let step = currentStep else { return nil }
        return SessionItem.oneLine(step)
    }

    static func oneLine(_ text: String, limit: Int = 80) -> String? {
        let line = text.split(whereSeparator: { $0.isWhitespace || $0.isNewline }).joined(separator: " ")
        guard !line.isEmpty else { return nil }
        return line.count > limit ? String(line.prefix(limit - 1)) + "…" : line
    }

    var statusText: String {
        if needsApproval || state == .approval { return "waiting for your OK" }
        if !question.isEmpty || state == .question { return "has a question" }
        switch state {
        case .working, .thinking, .searching:
            return steps.isEmpty ? "working" : String(localized: "working · \(min(stepIndex + 1, steps.count))/\(steps.count)")
        case .finished: return "✓ done"
        case .error: return "error"
        case .ratelimit: return "rate limited"
        case .sleeping: return "asleep"
        case .dizzy: return "dizzy"
        default: return "idle"
        }
    }

    var statusColor: Color {
        switch urgency {
        case 0: .orange
        case 1: .cyan
        case 2: .red
        case 4: .green
        default: .secondary
        }
    }

    var title: String { name.isEmpty ? pillName : name }

    var tone: SharedSession.Tone {
        switch urgency {
        case 0: .waiting
        case 1: .question
        case 2: .error
        case 3: .working
        case 4: .done
        default: .idle
        }
    }

    /// What the widgets get (SharedSessions).
    var shared: SharedSession {
        SharedSession(id: id, title: title, agent: pillName, color: color, state: state.rawValue,
                      statusText: statusText, tone: tone, urgency: urgency,
                      stepIndex: stepIndex, stepCount: steps.count,
                      currentStep: currentStep ?? "", updatedAt: updatedAt)
    }
}

extension Array where Element == SessionItem {
    func sortedByUrgency() -> [SessionItem] {
        sorted { ($0.urgency, $1.updatedAt) < ($1.urgency, $0.updatedAt) }
    }

    /// "2 working · 1 waiting", or nil when nothing is going on.
    var summary: String? {
        let waiting = filter(\.isWaitingForYou).count
        let working = filter(\.isWorking).count
        var parts: [String] = []
        if working > 0 { parts.append("\(working) working") }
        if waiting > 0 { parts.append("\(waiting) waiting") }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// The state Mochi shows in the notch: the most urgent session's.
    var leadState: BotState {
        guard let lead = sortedByUrgency().first else { return .sleeping }
        if lead.isWaitingForYou { return lead.needsApproval ? .approval : .question }
        return lead.state
    }
}
