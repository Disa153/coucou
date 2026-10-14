#if LAN_LINK
import SwiftUI
import UIKit

/// Pairing this iPhone with Coucou for Windows/Linux: the computer shows a QR
/// code (Settings → Android phone → Show pairing code). Scanning it with the
/// Camera opens Coucou on this link; it can also be pasted here.
struct LANPairingView: View {
    @Environment(\.dismiss) private var dismiss
    var lan = LANLink.shared
    @State private var pasted = ""
    @State private var invalid = false
    @State private var away = LANLink.shared.pairing?.awayHost ?? ""

    var body: some View {
        NavigationStack {
            List {
                Section {
                    HStack(spacing: 14) {
                        MochiLive(state: lan.state == .connected ? .finished : .idle)
                            .padding(6)
                            .frame(width: 64, height: 64)
                            .background(Color(white: 0.16), in: RoundedRectangle(cornerRadius: 16))
                        VStack(alignment: .leading, spacing: 4) {
                            Text("Your computer").font(.title3.weight(.semibold))
                            Text(statusText).font(.footnote).foregroundStyle(statusColor)
                        }
                    }
                    .padding(.vertical, 4)
                }

                if let proposed = lan.proposedPairing {
                    Section("Pair with this computer?") {
                        LabeledContent("Name", value: proposed.name)
                        LabeledContent("Address", value: "\(proposed.host):\(proposed.port)")
                        Button("Pair") { lan.pair(proposed) }
                            .font(.headline)
                        Button("Cancel", role: .cancel) { lan.proposedPairing = nil }
                    }
                }

                if let pairing = lan.pairing, lan.proposedPairing == nil {
                    Section("Paired") {
                        LabeledContent("Computer", value: lan.desktopName.isEmpty ? pairing.name : lan.desktopName)
                        LabeledContent("Address", value: "\(pairing.host):\(pairing.port)")
                        if lan.state != .connected {
                            Button("Try again") { lan.wake() }
                        }
                        Button("Unpair", role: .destructive) { lan.unpair() }
                    }
                    Section {
                        TextField("100.x.y.z", text: $away)
                            .font(.body.monospaced())
                            .keyboardType(.numbersAndPunctuation)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .onSubmit { lan.setAwayHost(away) }
                        Button("Save") { lan.setAwayHost(away) }
                            .disabled(away == (lan.pairing?.awayHost ?? ""))
                    } header: {
                        Text("Away from home")
                    } footer: {
                        Text("Install Tailscale on the computer and on this iPhone, signed in to the same account, and enter the computer's Tailscale address (100.…). When the home address does not answer, Coucou tries this one.")
                    }
                    Section {
                        Toggle("Stay connected in the background",
                               isOn: Binding(get: { LANKeepAlive.shared.enabled },
                                             set: { LANKeepAlive.shared.enabled = $0 }))
                    } footer: {
                        Text("Approvals then arrive as notifications with Allow and Deny, without opening Coucou (press and hold the notification). Uses some battery.")
                    }
                }

                Section {
                    step(1, "On the computer, open Coucou's Settings → Android phone and turn the phone link on.")
                    step(2, "Click \"Show pairing code\".")
                    step(3, "Scan the QR code with the iPhone's Camera and tap \"Open in Coucou\". Or copy the link and paste it below.")
                    step(4, "The iPhone and the computer must be on the same Wi-Fi.")
                } header: {
                    Text(lan.pairing == nil ? "How to pair" : "Pair again")
                }

                Section {
                    TextField("coucou://pair?…", text: $pasted, axis: .vertical)
                        .font(.footnote.monospaced())
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .lineLimit(1...4)
                    PasteButton(payloadType: String.self) { strings in
                        if let text = strings.first { pasted = text; use(text) }
                    }
                    Button("Use this link") { use(pasted) }
                        .disabled(pasted.isEmpty)
                } footer: {
                    if invalid {
                        Text("That isn't a Coucou pairing link.").foregroundStyle(.red)
                    } else {
                        Text("The link holds a secret code: it is kept in this iPhone's Keychain and only sent to that computer.")
                    }
                }
            }
            .navigationTitle("Computer")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .preferredColorScheme(.dark)
    }

    private func use(_ text: String) {
        if let pairing = LANPairing.parse(text) {
            invalid = false
            pasted = ""
            lan.proposedPairing = pairing
        } else {
            invalid = true
        }
    }

    private var statusText: String {
        switch lan.state {
        case .unpaired: "Not paired yet"
        case .connecting: "Connecting…"
        case .connected: "Connected to \(lan.desktopName.isEmpty ? (lan.pairing?.name ?? "your computer") : lan.desktopName)"
        case .offline(let reason): reason
        case .rejected: "The computer refused this iPhone's code. Pair again."
        }
    }

    private var statusColor: Color {
        switch lan.state {
        case .connected: .green
        case .connecting, .unpaired: .secondary
        case .offline, .rejected: .orange
        }
    }

    private func step(_ number: Int, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Text("\(number)")
                .font(.footnote.weight(.bold))
                .frame(width: 22, height: 22)
                .background(Color(white: 0.2), in: Circle())
            Text(text).font(.callout)
        }
    }
}
#endif
