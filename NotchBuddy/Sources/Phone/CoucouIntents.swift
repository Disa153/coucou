import AppIntents
import Foundation

// Siri and the Shortcuts app: ask where your agents are, or send Claude the
// next instruction. Sending needs the iPhone unlocked by its owner, like the
// instruction field in the app needs Face ID.

struct AgentsStatusIntent: AppIntent {
    static let title: LocalizedStringResource = "Agent status"
    static var description: IntentDescription { IntentDescription("What your agents are doing on your Mac.") }

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let link = PhoneLink.shared
        await link.refresh()
        let sessions = link.sessions.sortedByUrgency()
        guard let lead = sessions.first else {
            return .result(dialog: "All quiet: no agent session on your Mac.")
        }
        let summary = sessions.summary ?? "All quiet"
        return .result(dialog: "\(summary). \(lead.pillName), \(lead.title): \(lead.statusText).")
    }
}

struct AskClaudeIntent: AppIntent {
    static let title: LocalizedStringResource = "Ask Claude"
    static var description: IntentDescription {
        IntentDescription("Sends the next instruction to your Claude Code session on your Mac.")
    }
    static var authenticationPolicy: IntentAuthenticationPolicy { .requiresAuthentication }

    @Parameter(title: "Instruction", requestValueDialog: "What should Claude do?")
    var instruction: String

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let link = PhoneLink.shared
        await link.refresh()
        let text = instruction.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return .result(dialog: "Nothing to send.") }
        // The Claude Code session that last moved and takes instructions.
        guard let session = link.sessions
            .filter({ $0.acceptsInstructions })
            .max(by: { $0.updatedAt < $1.updatedAt }) else {
            return .result(dialog: "No Claude Code session takes instructions. Turn it on in Coucou's Settings on your Mac.")
        }
        guard await link.sendInstruction(text, pillId: session.id) else {
            return .result(dialog: "Couldn't reach iCloud. Nothing was sent.")
        }
        return .result(dialog: "Sent to Claude in \(session.title). Your Mac picks it up within 15 seconds.")
    }
}

/// Siri: "Allow in Coucou". Approves the command waiting for your OK, after
/// reading it out and asking you to confirm. Needs the iPhone unlocked by its
/// owner, like Allow on the Live Activity. Applies only to that exact command.
struct ApproveWaitingIntent: AppIntent {
    static let title: LocalizedStringResource = "Allow the waiting command"
    static var description: IntentDescription {
        IntentDescription("Allows the command an agent is waiting for your OK on, after you confirm it.")
    }
    static var authenticationPolicy: IntentAuthenticationPolicy { .requiresAuthentication }

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let link = PhoneLink.shared
        guard let waiting = await WaitingApproval.find(link) else {
            return .result(dialog: "Nothing is waiting for your OK.")
        }
        let command = WaitingApproval.spoken(waiting.approvalCommand)
        try await requestConfirmation(
            result: .result(dialog: "\(waiting.pillName) wants to run \(command). Allow it?"),
            confirmationActionName: .run)
        guard await link.allowFromOutside(fingerprint: waiting.approvalFingerprint,
                                          pillId: waiting.id, from: "Siri") else {
            return .result(dialog: "Not sent: the computer isn't connected.")
        }
        return .result(dialog: "Allowed.")
    }
}

/// Siri: "Deny in Coucou". Refuses the command waiting for your OK.
struct DenyWaitingIntent: AppIntent {
    static let title: LocalizedStringResource = "Deny the waiting command"
    static var description: IntentDescription {
        IntentDescription("Denies the command an agent is waiting for your OK on.")
    }
    static var authenticationPolicy: IntentAuthenticationPolicy { .requiresAuthentication }

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let link = PhoneLink.shared
        guard let waiting = await WaitingApproval.find(link) else {
            return .result(dialog: "Nothing is waiting for your OK.")
        }
        let command = WaitingApproval.spoken(waiting.approvalCommand)
        let summary = command.isEmpty ? "Denied with Siri" : command
        guard await link.decide(.deny, fingerprint: waiting.approvalFingerprint,
                                pillId: waiting.id, summary: summary) else {
            return .result(dialog: "Not sent: the computer isn't connected.")
        }
        return .result(dialog: "Denied.")
    }
}

/// The approval Siri talks about: the most urgent session waiting for an OK.
enum WaitingApproval {
    /// Wakes the link and, when Siri started Coucou cold, gives the computer a
    /// few seconds to report its sessions before saying nothing waits.
    @MainActor
    static func find(_ link: PhoneLink) async -> SessionItem? {
        await link.refresh()
        for _ in 0..<12 {
            if let found = lead(in: link.sessions) { return found }
            try? await Task.sleep(for: .milliseconds(500))
        }
        return lead(in: link.sessions)
    }

    static func lead(in sessions: [SessionItem]) -> SessionItem? {
        sessions.sortedByUrgency().first { $0.needsApproval && !$0.approvalFingerprint.isEmpty }
    }

    /// The command, short enough to be read out.
    static func spoken(_ command: String) -> String {
        let line = command.split(whereSeparator: \.isNewline).first.map(String.init) ?? ""
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        return trimmed.count > 120 ? String(trimmed.prefix(120)) + "…" : trimmed
    }
}

struct CoucouShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(intent: ApproveWaitingIntent(),
                    phrases: ["Allow in \(.applicationName)",
                              "Approve in \(.applicationName)",
                              "Allow the command in \(.applicationName)"],
                    shortTitle: "Allow",
                    systemImageName: "checkmark.circle")
        AppShortcut(intent: DenyWaitingIntent(),
                    phrases: ["Deny in \(.applicationName)",
                              "Refuse in \(.applicationName)",
                              "Deny the command in \(.applicationName)"],
                    shortTitle: "Deny",
                    systemImageName: "xmark.circle")
        AppShortcut(intent: AskClaudeIntent(),
                    phrases: ["Ask Claude in \(.applicationName)",
                              "Tell Claude with \(.applicationName)",
                              "Send an instruction with \(.applicationName)"],
                    shortTitle: "Ask Claude",
                    systemImageName: "text.bubble")
        AppShortcut(intent: AgentsStatusIntent(),
                    phrases: ["What are my agents doing in \(.applicationName)",
                              "\(.applicationName) status",
                              "Agent status in \(.applicationName)"],
                    shortTitle: "Agent status",
                    systemImageName: "sparkles")
    }
}

/// A Focus filter: in a Focus (Sleep, Work…), Coucou can keep quiet about
/// finished and failed agents and only notify what waits on you.
struct CoucouFocusFilter: SetFocusFilterIntent {
    static let title: LocalizedStringResource = "Coucou"
    static var description: IntentDescription {
        IntentDescription("Choose what Coucou notifies during this Focus.")
    }

    @Parameter(title: "Only approvals and questions", default: false)
    var onlyWaiting: Bool

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: onlyWaiting ? "Only approvals and questions" : "Everything")
    }

    func perform() async throws -> some IntentResult {
        UserDefaults.standard.set(onlyWaiting, forKey: PhoneSettings.focusOnlyWaitingKey)
        return .result()
    }
}
