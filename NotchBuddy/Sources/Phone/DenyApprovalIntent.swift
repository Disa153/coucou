import AppIntents
import Foundation

/// Deny on the Live Activity: answers the command waiting for your OK without
/// opening Coucou. Compiled into the app and the widgets extension; iOS runs
/// it in the app.
struct DenyApprovalIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "Deny the command"
    static var isDiscoverable: Bool { false }
    static var authenticationPolicy: IntentAuthenticationPolicy { .requiresAuthentication }

    @Parameter(title: "Request") var fingerprint: String
    @Parameter(title: "Agent") var pillId: String

    init() {}

    init(fingerprint: String, pillId: String) {
        self.fingerprint = fingerprint
        self.pillId = pillId
    }

    func perform() async throws -> some IntentResult {
        #if !WIDGET_EXTENSION
        let link = await PhoneLink.shared
        _ = await link.decide(.deny, fingerprint: fingerprint, pillId: pillId, summary: "Denied from the Lock Screen")
        #endif
        return .result()
    }
}

/// Allow on the Live Activity or the Dynamic Island: Coucou comes forward and
/// asks for Face ID every time, unlocked or not, as Allow does in the app;
/// only then the OK goes out. The computer applies it only to this exact command.
struct AllowApprovalIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "Allow the command"
    static var isDiscoverable: Bool { false }
    static var authenticationPolicy: IntentAuthenticationPolicy { .requiresAuthentication }
    /// Face ID needs Coucou on screen: from the background it cannot ask.
    static var openAppWhenRun: Bool { true }

    @Parameter(title: "Request") var fingerprint: String
    @Parameter(title: "Agent") var pillId: String

    init() {}

    init(fingerprint: String, pillId: String) {
        self.fingerprint = fingerprint
        self.pillId = pillId
    }

    func perform() async throws -> some IntentResult {
        #if !WIDGET_EXTENSION
        guard await OwnerCheck.confirm(reason: "Allow this command") else {
            await MainActor.run { Haptics.warning() }
            return .result()
        }
        let link = await PhoneLink.shared
        _ = await link.allowFromOutside(fingerprint: fingerprint, pillId: pillId, from: "the Lock Screen")
        #endif
        return .result()
    }
}
