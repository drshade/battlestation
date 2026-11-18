import AppKit
import SwiftUI

/// The window: header (presence, usage, connection) + Board / Deck / Links.
struct MainView: View {
    @Environment(Store.self) private var store

    var body: some View {
        @Bindable var store = store
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 18) {
                Text("Team Board").font(.title2.weight(.semibold))
                presence
                usage
                Spacer()
                TabSwitcher(tab: $store.tab, deckCount: store.world.openAsks.count)
            }
            if !store.connected {
                Label("Not connected to bsctl (\(Bsctl.path)) — retrying every 5s.", systemImage: "bolt.horizontal.circle")
                    .font(.caption).foregroundStyle(Theme.urgency("high"))
            }
            switch store.tab {
            case .board: BoardView()
            case .deck: DeckView()
            case .links: LinksView()
            }
            if let err = store.lastError {
                HStack {
                    Text(err).font(Theme.mono).foregroundStyle(Theme.urgency("high")).lineLimit(2)
                    Spacer()
                    Button("Dismiss") { store.lastError = nil }.buttonStyle(.bs)
                }
            }
        }
        .padding(16)
        .frame(minWidth: 720, minHeight: 420, alignment: .topLeading)
        .background(Theme.bg)
        .foregroundStyle(Theme.fg)
    }

    private var presence: some View {
        let h = store.world.human
        let active = h?.state == "active"
        return HStack(spacing: 5) {
            Circle().fill(active ? Theme.thinking : Theme.muted).frame(width: 8, height: 8)
            Text(active ? "you're here" : (h?.state == "idle" ? "away \(fmtAge(h?.idle_secs ?? 0))" : "presence unknown"))
                .font(.caption).foregroundStyle(Theme.muted)
        }
    }

    @ViewBuilder private var usage: some View {
        if let u = store.world.usage?["claude"] {
            HStack(spacing: 10) {
                meter("session", u.sessionPct)
                meter("weekly", u.weeklyPct)
            }
        }
    }

    private func meter(_ label: String, _ pct: Double?) -> some View {
        HStack(spacing: 5) {
            Text(label).font(.caption).foregroundStyle(Theme.muted)
            Meter(value: pct ?? 0)
            Text("\(Int(pct ?? 0))%").font(Theme.mono).foregroundStyle(Theme.muted)
        }
        .help("Claude plan usage (\(label))")
    }
}

/// The menu-bar popover: who needs you, at a glance.
struct MenuPanel: View {
    @Environment(Store.self) private var store
    var open: (Store.Tab) -> Void

    var body: some View {
        let w = store.world
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Battlestation").font(.headline)
                Spacer()
                Text("\(w.openAsks.count) on the Deck").font(.caption).foregroundStyle(Theme.muted)
            }
            if w.allAgents.isEmpty {
                Text("No live agent sessions.").font(.callout).foregroundStyle(Theme.muted)
            }
            ForEach(w.lanes.filter { l in w.allAgents.contains { $0.ws == l.ws } }) { lane in
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(lane.bs) · \((lane.name ?? "ws \(lane.ws)").uppercased())")
                        .font(.caption.weight(.semibold)).foregroundStyle(Theme.muted)
                    ForEach(w.allAgents.filter { $0.ws == lane.ws }) { a in
                        Button { store.run(["ws", "focus", "--session", a.session]) } label: {
                            HStack(spacing: 8) {
                                BotIcon(kind: a.harness, status: a.state, size: 16)
                                Text(a.name ?? a.title ?? a.harness).lineLimit(1)
                                Spacer()
                                Text(a.state.label).font(.caption).foregroundStyle(Theme.status(a.state))
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            Divider()
            HStack {
                Button("Board") { open(.board) }
                Button("Deck") { open(.deck) }
                Button("Links") { open(.links) }
                Spacer()
                Button("Quit") { NSApp.terminate(nil) }
            }
            .buttonStyle(.bs)
        }
        .padding(12)
        .frame(width: 320)
    }
}

/// The status item's face: one dot per live agent (status colour) and the
/// open-asks count (green if any asker is blocking on you, else red).
struct StatusItemFace: View {
    var world: World
    var body: some View {
        HStack(spacing: 3) {
            ForEach(world.allAgents.prefix(8)) { a in
                Circle().fill(Theme.status(a.state)).frame(width: 7, height: 7)
            }
            if world.allAgents.isEmpty {
                Image(systemName: "circle.grid.2x1").font(.system(size: 12))
            }
            let open = world.openAsks
            if !open.isEmpty {
                Text("\(open.count)")
                    .font(.system(size: 10, weight: .bold, design: .monospaced))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 4)
                    .background(Capsule().fill(open.contains(where: \.isBlocking) ? Theme.thinking : Theme.urgency("high")))
            }
        }
        .padding(.horizontal, 2)
        .frame(height: 18)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let store = Store()
    private var statusItem: NSStatusItem!
    private let popover = NSPopover()
    private var window: NSWindow?

    func applicationWillFinishLaunching(_ notification: Notification) {
        NSAppleEventManager.shared().setEventHandler(
            self, andSelector: #selector(handleURL(_:reply:)),
            forEventClass: AEEventClass(kInternetEventClass), andEventID: AEEventID(kAEGetURL))
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.target = self
        statusItem.button?.action = #selector(togglePopover)
        popover.behavior = .transient
        popover.contentViewController = NSHostingController(
            rootView: MenuPanel(open: { [weak self] tab in self?.show(tab) }).environment(store))
        store.onWorldChange = { [weak self] in self?.renderStatusItem() }
        renderStatusItem()
        store.refreshDeckSwitch()
        store.startStream()
    }

    private func renderStatusItem() {
        let r = ImageRenderer(content: StatusItemFace(world: store.world).foregroundStyle(.primary))
        r.scale = NSScreen.main?.backingScaleFactor ?? 2
        statusItem.button?.image = r.nsImage
        statusItem.button?.toolTip = "\(store.world.allAgents.count) agents · \(store.world.openAsks.count) on the Deck"
    }

    @objc private func togglePopover() {
        guard let button = statusItem.button else { return }
        if popover.isShown { popover.performClose(nil) } else {
            popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
            popover.contentViewController?.view.window?.makeKey()
        }
    }

    func show(_ tab: Store.Tab) {
        popover.performClose(nil)
        store.tab = tab
        if window == nil {
            let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1240, height: 760),
                             styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                             backing: .buffered, defer: false)
            w.title = "Team Board"
            w.isReleasedWhenClosed = false
            w.contentView = NSHostingView(rootView: MainView().environment(store))
            w.setFrameAutosaveName("TeamBoard")
            if !w.setFrameUsingName("TeamBoard") { w.center() }
            window = w
        }
        store.refreshDeckSwitch()
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
    }

    /// battlestation://board | deck | links — what keybinds open.
    @objc private func handleURL(_ event: NSAppleEventDescriptor, reply: NSAppleEventDescriptor) {
        guard let s = event.paramDescriptor(forKeyword: keyDirectObject)?.stringValue,
              let host = URL(string: s)?.host else { return }
        show(Store.Tab(rawValue: host.capitalized) ?? .board)
    }
}

/// `Battlestation --snapshot <world.json> <out.png> [board|deck|links|menu]`:
/// render a view offscreen from a captured world — a development aid for
/// checking layout without a display or a running bsctl.
@MainActor
func snapshot(_ args: [String]) -> Int32 {
    guard args.count >= 2, let data = FileManager.default.contents(atPath: args[0]),
          let world = try? JSONDecoder().decode(World.self, from: data) else {
        FileHandle.standardError.write("usage: --snapshot <world.json> <out.png> [board|deck|links|menu] [dark]\n".data(using: .utf8)!)
        return 2
    }
    let store = Store()
    store.world = world
    store.connected = true
    let which = args.count > 2 ? args[2] : "board"
    store.tab = Store.Tab(rawValue: which.capitalized) ?? .board
    if which == "deck" { store.expandedAsk = world.openAsks.first?.id }
    let dark = args.contains("dark")
    let root: AnyView = which == "menu"
        ? AnyView(MenuPanel(open: { _ in }).background(Theme.bg))
        : AnyView(MainView().frame(width: 1240))
    let r = ImageRenderer(content: root.environment(store).environment(\.isSnapshot, true)
        .environment(\.colorScheme, dark ? .dark : .light))
    r.scale = 2
    NSApp.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
    guard let tiff = r.nsImage?.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
          let png = rep.representation(using: .png, properties: [:]) else { return 1 }
    FileManager.default.createFile(atPath: args[1], contents: png)
    return 0
}

@main
enum Main {
    @MainActor static func main() {
        let args = Array(CommandLine.arguments.dropFirst())
        if let i = args.firstIndex(of: "--snapshot") {
            _ = NSApplication.shared
            exit(snapshot(Array(args[(i + 1)...])))
        }
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let delegate = AppDelegate()
        app.delegate = delegate
        app.run()
    }
}
