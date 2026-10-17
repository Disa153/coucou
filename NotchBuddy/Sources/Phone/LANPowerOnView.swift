#if LAN_LINK
import SwiftUI

/// "Turn on computer", with what happened after: on the home screen while the computer is away.
struct PowerOnButton: View {
    var power = LANPowerOn.shared

    var body: some View {
        VStack(spacing: 6) {
            Button {
                Haptics.impact()
                power.powerOn()
            } label: {
                Label(power.phase == .sending ? "Sending…" : "Turn on computer", systemImage: "power")
            }
            .buttonStyle(.bordered)
            .disabled(power.phase == .sending)
            if let text = PowerOnSection.phaseText(power.phase) {
                Text(text)
                    .font(.caption)
                    .foregroundStyle(PowerOnSection.phaseColor(power.phase))
                    .multilineTextAlignment(.center)
            }
        }
    }
}

/// Settings → Your computer → Turn on computer: the card Coucou learned, the router, the button.
struct PowerOnSection: View {
    @Bindable var power = LANPowerOn.shared
    @State private var mac = ""
    @State private var badMac = false
    @State private var routerNote: String?
    @State private var testing = false

    var body: some View {
        Section {
            LabeledContent("Network card", value: power.target?.colonMac ?? "Not known yet")
                .font(.callout)
            Button {
                Haptics.impact()
                power.powerOn()
            } label: {
                Label(power.phase == .sending ? "Sending…" : "Turn on computer", systemImage: "power")
            }
            .disabled(power.phase == .sending || LANLink.shared.state == .connected)
            if let text = Self.phaseText(power.phase) {
                Text(text).font(.footnote).foregroundStyle(Self.phaseColor(power.phase))
            }
        } header: {
            Text("Turn on computer")
        } footer: {
            Text("Works when the computer is wired to the router and Wake-on-LAN is on in its BIOS and network card. Coucou learns the card while the computer is on.")
        }

        Section {
            TextField("Router address", text: $power.router.address)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            TextField("Login", text: $power.router.login)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            SecureField("Password", text: $power.router.password)
            Button(testing ? "Checking…" : "Save and check") {
                testing = true
                routerNote = nil
                Task {
                    routerNote = await power.testRouter()
                    testing = false
                }
            }
            .disabled(testing || !power.routerReady)
            if let routerNote {
                Text(routerNote).font(.footnote).foregroundStyle(.secondary)
            }
        } header: {
            Text("Keenetic router")
        } footer: {
            Text("iOS may keep Coucou from sending the signal itself: then the router sends it. Use a separate router user for Coucou. The password stays in this iPhone's Keychain and only goes to the router, on your home Wi-Fi.")
        }

        Section {
            TextField("aa:bb:cc:dd:ee:ff", text: $mac)
                .font(.body.monospaced())
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            Button("Use this card") { badMac = !power.setMac(mac); if !badMac { mac = "" } }
                .disabled(mac.isEmpty)
        } header: {
            Text("Network card by hand")
        } footer: {
            if badMac {
                Text("That isn't a MAC address.").foregroundStyle(.red)
            } else {
                Text("Only if Coucou couldn't read it: on the computer, run getmac /v and copy the wired card's physical address.")
            }
        }
    }

    static func phaseText(_ phase: LANPowerOn.Phase) -> String? {
        switch phase {
        case .idle, .sending: nil
        case .waiting(let how): "\(how) Waiting for the computer…"
        case .on: "The computer is on."
        case .failed(let why): why
        }
    }

    static func phaseColor(_ phase: LANPowerOn.Phase) -> Color {
        switch phase {
        case .on: .green
        case .failed: .orange
        default: .secondary
        }
    }
}
#endif
