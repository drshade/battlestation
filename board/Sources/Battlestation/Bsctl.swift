import Foundation
import Observation

/// The one bridge to bsctl. The app owns no state: it renders the stream and
/// turns every gesture into the same bsctl verb the Linux bar runs.
enum Bsctl {
    /// `$HOME/.local/bin/bsctl` — absolute, like every other caller (a GUI
    /// app's environment carries no useful PATH).
    static var path: String {
        ProcessInfo.processInfo.environment["BSCTL"]
            ?? (NSHomeDirectory() as NSString).appendingPathComponent(".local/bin/bsctl")
    }

    /// Fire a verb; stderr of a failure is reported to `onError` (main actor).
    static func run(_ args: [String], onError: (@MainActor (String) -> Void)? = nil) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: path)
        p.arguments = args
        let err = Pipe()
        p.standardError = err
        p.standardOutput = FileHandle.nullDevice
        p.terminationHandler = { proc in
            guard proc.terminationStatus != 0 else { return }
            let msg = String(data: err.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            let line = "bsctl \(args.joined(separator: " ")): " + msg.trimmingCharacters(in: .whitespacesAndNewlines)
            Task { @MainActor in onError?(line) }
        }
        do { try p.run() } catch {
            Task { @MainActor in onError?("cannot run \(path): \(error.localizedDescription)") }
        }
    }

    /// Run a verb and return its stdout (for one-shot reads like `asks deck`).
    static func read(_ args: [String]) async -> String? {
        await withCheckedContinuation { cont in
            let p = Process()
            p.executableURL = URL(fileURLWithPath: path)
            p.arguments = args
            let out = Pipe()
            p.standardOutput = out
            p.standardError = FileHandle.nullDevice
            p.terminationHandler = { proc in
                let data = out.fileHandleForReading.readDataToEndOfFile()
                cont.resume(returning: proc.terminationStatus == 0 ? String(data: data, encoding: .utf8) : nil)
            }
            do { try p.run() } catch { cont.resume(returning: nil) }
        }
    }
}

/// Live app state: the latest world plus the few UI facts bsctl doesn't own.
@MainActor @Observable
final class Store {
    var world = World()
    var connected = false
    var deckOn = true
    var lastError: String?
    var tab: Tab = .board
    var expandedAsk: Int?
    var onWorldChange: (() -> Void)?

    enum Tab: String, CaseIterable, Identifiable {
        case board = "Board", deck = "Deck", links = "Links"
        var id: String { rawValue }
    }

    func run(_ args: [String]) {
        Bsctl.run(args) { [weak self] msg in self?.lastError = msg }
    }

    /// Panel.qml's willWake: the asker is idle at its prompt and not parked
    /// in the blocking RPC, so answering should also type the wake line.
    func willWake(_ ask: Ask) -> Bool {
        !ask.isBlocking && world.agent(session: ask.session)?.state == .waiting
    }

    /// Panel.qml's submitAnswer: store the answer (answer, or complete when
    /// empty); when the asker is idle, also fire the wake trigger. One path
    /// for the Deck tab and the notification banner.
    func submit(_ ask: Ask, _ text: String) {
        let wake = willWake(ask)
        run(text.isEmpty ? ["asks", "complete", String(ask.id)] : ["asks", "answer", String(ask.id), text])
        if wake { run(["asks", "wake", String(ask.id)]) }
    }

    func refreshDeckSwitch() {
        Task {
            if let out = await Bsctl.read(["asks", "deck", "status"]) {
                deckOn = !out.lowercased().contains("off")
            }
        }
    }

    func setDeck(_ on: Bool) {
        deckOn = on
        run(["asks", "deck", on ? "on" : "off"])
    }

    /// Follow `bsctl status --format json --stream` forever: one NDJSON line
    /// per change; on exit, retry after 5s (the bar plugin's policy).
    func startStream() {
        let store = self // app-lifetime object: the loop runs as long as the app
        Task.detached {
            while true {
                await store.consumeOnce()
                await MainActor.run { store.connected = false }
                try? await Task.sleep(for: .seconds(5))
            }
        }
    }

    nonisolated private func consumeOnce() async {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: Bsctl.path)
        p.arguments = ["status", "--format", "json", "--stream"]
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        do { try p.run() } catch { return }
        let decoder = JSONDecoder()
        do {
            for try await line in out.fileHandleForReading.bytes.lines {
                guard let data = line.data(using: .utf8),
                      let w = try? decoder.decode(World.self, from: data) else { continue }
                await MainActor.run { [weak self] in
                    guard let self else { return }
                    self.connected = true
                    if self.world != w {
                        self.world = w
                        self.onWorldChange?()
                    }
                }
            }
        } catch {}
        p.terminate()
    }
}
