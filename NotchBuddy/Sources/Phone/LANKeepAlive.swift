#if LAN_LINK
import AVFoundation
import Foundation
import Observation

// Keeps the link to the Windows computer open while Coucou is in the background.
//
// The iCloud build is woken by Apple push; the sideloaded LAN build has no push (a
// free Apple ID cannot sign for it), so iOS suspends it a few seconds after it leaves
// the screen and the connection goes with it: no banner, no Allow / Deny on the lock
// screen. While this is on, the app plays silence (mixed with whatever else plays, so
// music is never interrupted), which iOS lets run in the background. The link stays
// open and approvals arrive as notifications with their Allow / Deny buttons.
// Costs some battery; Settings → Your computer turns it off.

@MainActor
@Observable
final class LANKeepAlive {
    static let shared = LANKeepAlive()

    private static let key = "lanKeepAlive"

    var enabled: Bool {
        didSet {
            UserDefaults.standard.set(enabled, forKey: Self.key)
            if !enabled { stop() }
        }
    }
    private(set) var running = false

    @ObservationIgnored private var engine: AVAudioEngine?
    @ObservationIgnored private var player: AVAudioPlayerNode?
    @ObservationIgnored private var observers: [NSObjectProtocol] = []

    private init() {
        enabled = UserDefaults.standard.object(forKey: Self.key) as? Bool ?? true
    }

    /// The app went to the background with the link open.
    func start() {
        guard enabled, !running else { return }
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.playback, mode: .default, options: [.mixWithOthers])
            try session.setActive(true)
        } catch {
            return
        }
        let engine = AVAudioEngine()
        let player = AVAudioPlayerNode()
        engine.attach(player)
        guard let format = AVAudioFormat(standardFormatWithSampleRate: 44_100, channels: 1),
              let silence = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 44_100) else { return }
        silence.frameLength = silence.frameCapacity   // zeroed: one second of nothing
        engine.connect(player, to: engine.mainMixerNode, format: format)
        do {
            try engine.start()
        } catch {
            return
        }
        player.scheduleBuffer(silence, at: nil, options: .loops)
        player.play()
        self.engine = engine
        self.player = player
        running = true
        watchInterruptions()
    }

    /// Back on screen, or turned off.
    func stop() {
        for o in observers { NotificationCenter.default.removeObserver(o) }
        observers = []
        player?.stop()
        engine?.stop()
        player = nil
        engine = nil
        if running {
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }
        running = false
    }

    /// A call or another app took the audio: start again once it gives it back.
    private func watchInterruptions() {
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: AVAudioSession.interruptionNotification,
                                            object: nil, queue: .main) { note in
            let ended = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt)
                .flatMap(AVAudioSession.InterruptionType.init(rawValue:)) == .ended
            MainActor.assumeIsolated {
                guard ended else { return }
                let keeper = LANKeepAlive.shared
                keeper.stop()
                keeper.start()
            }
        })
        observers.append(center.addObserver(forName: AVAudioSession.mediaServicesWereResetNotification,
                                            object: nil, queue: .main) { _ in
            MainActor.assumeIsolated {
                let keeper = LANKeepAlive.shared
                keeper.stop()
                keeper.start()
            }
        })
    }
}
#endif
