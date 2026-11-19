import AppKit
import SwiftUI

/// The harness glyphs, shipped in the bundle from the bar plugin's asset set
/// (copied by bin/build — that directory stays the one source of truth).
/// Files are `<kind>/<stem>.svg` with emote faces `<kind>/<stem>-<face>.svg`;
/// status is baked into each file's fill, so no tinting happens here.
@MainActor
enum Assets {
    static var root: URL = {
        if let env = ProcessInfo.processInfo.environment["BATTLESTATION_ASSETS"] {
            return URL(fileURLWithPath: env)
        }
        return Bundle.main.resourceURL!.appendingPathComponent("assets")
    }()

    private static var cache: [String: NSImage] = [:]
    private static var faceCache: [String: [String]] = [:]

    static func image(_ kind: String, _ file: String) -> NSImage? {
        let key = "\(kind)/\(file)"
        if let img = cache[key] { return img }
        var url = root.appendingPathComponent(kind).appendingPathComponent(file + ".svg")
        if !FileManager.default.fileExists(atPath: url.path) {
            // Unknown harness: render as claude, like the bar plugin does.
            url = root.appendingPathComponent("claude").appendingPathComponent(file + ".svg")
        }
        let img = NSImage(contentsOf: url)
        cache[key] = img
        return img
    }

    /// The emote faces available for a kind+stem (e.g. thinking-blink).
    static func faces(_ kind: String, _ stem: String) -> [String] {
        let key = "\(kind)/\(stem)"
        if let f = faceCache[key] { return f }
        let dir = root.appendingPathComponent(kind)
        let names = (try? FileManager.default.contentsOfDirectory(atPath: dir.path)) ?? []
        let f = names.filter { $0.hasPrefix(stem + "-") && $0.hasSuffix(".svg") }
            .map { String($0.dropLast(4)) }.sorted()
        faceCache[key] = f
        return f
    }
}

/// One agent glyph. Shape = harness, colour = status. Busy bots emote often,
/// waiting ones rarely (the bar plugin's rhythm); a face shows briefly and
/// returns to the plain status glyph. Reduce Motion: no emotes, no pulse.
struct BotIcon: View {
    var kind: String
    var status: AgentStatus
    var size: CGFloat = 22
    @State private var face: String?
    @State private var pulse = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.isSnapshot) private var isSnapshot

    var body: some View {
        Group {
            if let img = Assets.image(kind, face ?? status.stem) {
                Image(nsImage: img).resizable().interpolation(.high).aspectRatio(contentMode: .fit)
            } else {
                Circle().fill(Theme.status(status))
            }
        }
        .frame(width: size, height: size)
        .scaleEffect(pulse ? 1.08 : 1)
        .animation(status.busy && !reduceMotion ? .easeInOut(duration: 0.9).repeatForever() : .default, value: pulse)
        .onAppear { pulse = status.busy && !reduceMotion && !isSnapshot }
        .onChange(of: status) { _, s in face = nil; pulse = s.busy && !reduceMotion && !isSnapshot }
        .task(id: status) { await emote() }
        .accessibilityLabel("\(kind), \(status.label)")
    }

    private func emote() async {
        guard !reduceMotion, !isSnapshot else { return }
        let faces = Assets.faces(kind, status.stem)
        guard !faces.isEmpty else { return }
        while !Task.isCancelled {
            let base: Double = status.busy ? 2.5 : 30
            try? await Task.sleep(for: .seconds(base * Double.random(in: 0.65...1.35)))
            face = faces.randomElement()
            try? await Task.sleep(for: .milliseconds(700))
            face = nil
        }
    }
}

private struct SnapshotKey: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    /// Rendering offscreen (--snapshot): no animation, no scroll containers.
    var isSnapshot: Bool {
        get { self[SnapshotKey.self] }
        set { self[SnapshotKey.self] = newValue }
    }
}
