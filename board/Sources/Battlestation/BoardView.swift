import SwiftUI

/// The Team Board: one lane per battlespace (bs order), a card per lead
/// session with its subagent squad and open asks, and the Switchboard's
/// links drawn between agents across lanes.
struct BoardView: View {
    @Environment(Store.self) private var store
    @Environment(\.isSnapshot) private var isSnapshot
    @State private var linking = false
    @State private var picked: String?
    @State private var hint: String?
    @State private var showEmpty = false

    private var world: World { store.world }

    /// Lanes worth a column: any with agents, a name, or the focus — unless
    /// "Show empty" is on. Ten mostly-empty columns would bury the teams.
    private var lanes: [Workspace] {
        world.lanes.filter { w in
            showEmpty || w.active == true || (w.name?.isEmpty == false)
                || world.allAgents.contains { $0.ws == w.ws }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            toolbar
            if world.workspaces == nil {
                ContentUnavailableView(
                    "No window manager", systemImage: "rectangle.3.group",
                    description: Text("bsctl can't see AeroSpace (or Hyprland). Agents still report; lanes appear when it's running."))
            } else if isSnapshot {
                lanesRow
            } else {
                ScrollView([.horizontal, .vertical]) { lanesRow }
            }
        }
    }

    private var toolbar: some View {
        HStack(spacing: 8) {
            Toggle(isOn: $linking) { Label("Link agents", systemImage: "point.3.connected.trianglepath.dotted") }
                .toggleStyle(BSPillToggle())
                .onChange(of: linking) { _, _ in picked = nil; hint = nil }
            Toggle("Show empty", isOn: $showEmpty).toggleStyle(BSCheckbox())
            Text(hint ?? (linking
                ? "Click a named agent, then another, to link or unlink them."
                : "Drag a lane title to reorder · double-click to rename · click an agent to jump to its window."))
                .font(.caption).foregroundStyle(Theme.muted).lineLimit(2)
            Spacer()
            legend
        }
    }

    private var legend: some View {
        HStack(spacing: 10) {
            ForEach([AgentStatus.thinking, .tooling, .waiting], id: \.self) { s in
                HStack(spacing: 4) {
                    Circle().fill(Theme.status(s)).frame(width: 7, height: 7)
                    Text(s.label).font(.caption).foregroundStyle(Theme.muted)
                }
            }
        }
    }

    private var lanesRow: some View {
        HStack(alignment: .top, spacing: 14) {
            ForEach(lanes) { w in
                LaneView(lane: w, linking: linking, picked: picked, onBot: tapBot, onReorder: reorder)
            }
        }
        .padding(.top, 22) // the lane over which skip-lane links are routed
        .padding(.bottom, 8)
        .overlayPreferenceValue(BotAnchorKey.self) { anchors in
            GeometryReader { geo in
                LinksLayer(links: world.sessionLinks, anchors: anchors, geo: geo)
            }
            .allowsHitTesting(false)
        }
    }

    private func tapBot(_ a: Agent) {
        guard linking else {
            store.run(["ws", "focus", "--session", a.session])
            return
        }
        guard let name = a.name else {
            hint = "That agent hasn't named itself yet (agents call set_name), so it can't be linked."
            return
        }
        guard let first = picked, let other = world.agent(session: first), let otherName = other.name else {
            picked = a.session
            hint = "Now pick the agent to link with \(name)."
            return
        }
        if other.session == a.session { picked = nil; hint = nil; return }
        let verb = world.linked(other, a) ? "unlink" : "link"
        store.run(["comms", verb,
                   "--a-workspace", world.label(ws: other.ws), "--a-name", otherName,
                   "--b-workspace", world.label(ws: a.ws), "--b-name", name])
        picked = nil
        hint = "\(verb == "link" ? "Linked" : "Unlinked") \(otherName) and \(name)."
    }

    /// Move `dragged` before `target` in the FULL battlespace order (hidden
    /// lanes included) — `ws map set` takes every ws-id in order.
    private func reorder(_ dragged: Int, before target: Int) {
        var order = world.lanes.map(\.ws).filter { $0 != dragged }
        guard let i = order.firstIndex(of: target) else { return }
        order.insert(dragged, at: i)
        store.run(["ws", "map", "set"] + order.map(String.init))
    }
}

/// One battlespace column.
private struct LaneView: View {
    @Environment(Store.self) private var store
    var lane: Workspace
    var linking: Bool
    var picked: String?
    var onBot: (Agent) -> Void
    var onReorder: (Int, Int) -> Void
    @State private var renaming = false
    @State private var draft = ""
    @State private var dropTarget = false

    var body: some View {
        let agents = store.world.allAgents.filter { $0.ws == lane.ws }
        VStack(alignment: .leading, spacing: 10) {
            header
            if agents.isEmpty {
                Text("No agents here").font(.caption).foregroundStyle(Theme.muted)
                    .frame(maxWidth: .infinity).padding(.vertical, 14)
                    .overlay(RoundedRectangle(cornerRadius: 8).stroke(Theme.line, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
            }
            ForEach(agents) { a in
                TeamCard(agent: a, linking: linking, selected: picked == a.session, onBot: onBot)
            }
        }
        .padding(10)
        .frame(width: 300, alignment: .topLeading)
        .background(RoundedRectangle(cornerRadius: 10).fill(Theme.surface))
        .overlay(RoundedRectangle(cornerRadius: 10)
            .stroke(dropTarget ? Theme.primary : (lane.active == true ? Theme.primary.opacity(0.7) : Theme.line),
                    lineWidth: dropTarget ? 2 : 1))
        .dropDestination(for: String.self) { items, _ in
            guard let s = items.first, let ws = Int(s), ws != lane.ws else { return false }
            onReorder(ws, lane.ws)
            return true
        } isTargeted: { dropTarget = $0 }
    }

    private var header: some View {
        HStack(spacing: 8) {
            Text("\(lane.bs)")
                .font(.system(.caption, design: .monospaced).weight(.semibold))
                .foregroundStyle(Theme.bg)
                .padding(.horizontal, 6).padding(.vertical, 1)
                .background(RoundedRectangle(cornerRadius: 5).fill(lane.active == true ? Theme.primary : Theme.fg))
            if renaming {
                TextField("name", text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { commitRename() }
                    .onExitCommand { renaming = false }
            } else {
                Text((lane.name ?? "ws \(lane.ws)").uppercased())
                    .font(.system(.caption).weight(.semibold)).kerning(0.8)
                    .lineLimit(1)
                    .onTapGesture(count: 2) { draft = lane.name ?? ""; renaming = true }
                    .help("Double-click to rename · drag to reorder")
            }
            Spacer()
            Text(lane.display ?? "").font(.caption2).foregroundStyle(Theme.muted).lineLimit(1)
        }
        .contentShape(Rectangle())
        .draggable(String(lane.ws))
    }

    private func commitRename() {
        renaming = false
        let name = draft.trimmingCharacters(in: .whitespaces)
        store.run(name.isEmpty ? ["ws", "name", "rm", "--ws-id", String(lane.ws)]
                               : ["ws", "name", "set", "--ws-id", String(lane.ws), "--name", name])
    }
}

/// A lead session, its squad, and its open asks.
private struct TeamCard: View {
    @Environment(Store.self) private var store
    var agent: Agent
    var linking: Bool
    var selected: Bool
    var onBot: (Agent) -> Void

    var body: some View {
        let unread = store.world.unread(agent)
        let asks = store.world.openAsks.filter { $0.session == agent.session }
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 10) {
                Button { onBot(agent) } label: {
                    BotIcon(kind: agent.harness, status: agent.state, size: 22)
                        .frame(width: 34, height: 34)
                        .background(RoundedRectangle(cornerRadius: 8).fill(Theme.surface))
                        .overlay(RoundedRectangle(cornerRadius: 8)
                            .stroke(selected ? Theme.primary : (linking && agent.name != nil ? Theme.primary.opacity(0.6) : Theme.line),
                                    lineWidth: selected ? 2 : 1))
                        .opacity(linking && agent.name == nil ? 0.45 : 1)
                }
                .buttonStyle(.plain)
                .help(linking ? "Select for linking" : "Jump to this agent's window")

                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        if let name = agent.name {
                            Text(name).font(.system(.callout, design: .monospaced).weight(.semibold))
                        } else {
                            Text("unnamed").font(.callout).italic().foregroundStyle(Theme.muted)
                        }
                        Chip(text: agent.harness)
                        if unread > 0 { Chip(text: "\(unread) unread", color: Theme.primary) }
                    }
                    if let t = agent.title, !t.isEmpty {
                        Text(t).font(.callout).foregroundStyle(Theme.fg).fixedSize(horizontal: false, vertical: true)
                    }
                    HStack(spacing: 5) {
                        Circle().fill(Theme.status(agent.state)).frame(width: 7, height: 7)
                        Text(agent.state.label).font(.caption.weight(.medium)).foregroundStyle(Theme.muted)
                    }
                }
            }
            if !agent.squad.isEmpty {
                Squad(agent: agent)
            }
            ForEach(asks) { k in AskChip(ask: k) }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 9).fill(Theme.card).shadow(color: .black.opacity(0.08), radius: 4, y: 2))
        .overlay(RoundedRectangle(cornerRadius: 9).stroke(Theme.line))
        .anchorPreference(key: BotAnchorKey.self, value: .bounds) { [agent.session: $0] }
    }
}

/// Running subagents, each with its elapsed time (ticks every second).
private struct Squad: View {
    var agent: Agent
    @Environment(\.isSnapshot) private var isSnapshot

    var body: some View {
        TimelineView(.periodic(from: .now, by: isSnapshot ? 3600 : 1)) { ctx in
            VStack(alignment: .leading, spacing: 4) {
                ForEach(agent.squad) { s in
                    HStack(spacing: 8) {
                        BotIcon(kind: agent.harness, status: agent.state, size: 14)
                        (Text(s.type ?? "agent").font(.system(.caption, design: .monospaced).weight(.semibold))
                            + Text(" " + (s.description ?? "")).font(.caption))
                            .lineLimit(1).truncationMode(.tail)
                            .help("\(s.type ?? "agent") — \(s.description ?? "")")
                        Spacer(minLength: 4)
                        if let started = s.started {
                            Text(fmtAge(ctx.date.timeIntervalSince1970 - started))
                                .font(Theme.mono).foregroundStyle(Theme.muted).monospacedDigit()
                        }
                    }
                }
            }
            .padding(.leading, 12)
            .overlay(alignment: .leading) { Rectangle().fill(Theme.line).frame(width: 2) }
            .padding(.leading, 16)
        }
    }
}

/// An open ask on its team card; click to open it in the Deck.
private struct AskChip: View {
    @Environment(Store.self) private var store
    var ask: Ask

    var body: some View {
        Button {
            store.expandedAsk = ask.id
            store.tab = .deck
        } label: {
            HStack(spacing: 8) {
                Rectangle().fill(Theme.urgency(ask.urgency)).frame(width: 3)
                Text(askGlyph(ask.type)).font(.system(.caption, design: .monospaced).weight(.semibold)).foregroundStyle(Theme.muted)
                Text(ask.title ?? "").font(.caption).lineLimit(1)
                Spacer(minLength: 4)
                if ask.isBlocking {
                    Text("▹ BLOCKING").font(.system(size: 9, weight: .semibold, design: .monospaced)).foregroundStyle(Theme.thinking)
                }
                Text("#\(ask.id)").font(Theme.mono).foregroundStyle(Theme.muted)
            }
            .padding(.trailing, 8)
            .frame(height: 24)
            .background(RoundedRectangle(cornerRadius: 6).fill(Theme.surface))
            .clipShape(RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(Theme.line))
        }
        .buttonStyle(.plain)
    }
}

func askGlyph(_ type: String?) -> String {
    switch type {
    case "question": return "?"
    case "review": return "⇄"
    default: return "i"
    }
}

/// session id -> the bounds of that agent's team card.
struct BotAnchorKey: PreferenceKey {
    static let defaultValue: [String: Anchor<CGRect>] = [:]
    static func reduce(value: inout [String: Anchor<CGRect>], nextValue: () -> [String: Anchor<CGRect>]) {
        value.merge(nextValue()) { $1 }
    }
}

/// Switchboard links as dashed curves between team cards, level with each
/// lead agent's icon. Across lanes the curve runs from the left card's right
/// edge to the right card's left edge (through the gap, never over a card's
/// own text); within a lane it bows out to the right.
private struct LinksLayer: View {
    var links: [(String, String)]
    var anchors: [String: Anchor<CGRect>]
    var geo: GeometryProxy

    var body: some View {
        Path { path in
            var skips = 0
            for (a, b) in links {
                guard let ra = anchors[a].map({ geo[$0] }), let rb = anchors[b].map({ geo[$0] }) else { continue }
                let (l, r) = ra.midX <= rb.midX ? (ra, rb) : (rb, ra)
                let iconY: CGFloat = 27 // card padding + half the 34pt icon
                let start = CGPoint(x: l.maxX, y: l.minY + iconY)
                let gap = r.minX - l.maxX
                if gap > 60 {
                    // Skips a lane: never cut through the cards between —
                    // rise into the margin above the lanes, cross, come down.
                    // Each skip-link gets its own height so they don't overlap.
                    let top = 4 + CGFloat(skips % 3) * 6
                    skips += 1
                    let end = CGPoint(x: r.minX, y: r.minY + iconY)
                    // Card edge -> middle of the inter-lane gap (10pt lane
                    // padding + half the 14pt spacing), so the vertical runs
                    // between lanes rather than on a lane's border.
                    let x0 = start.x + 17, x1 = end.x - 17
                    path.move(to: start)
                    path.addCurve(to: CGPoint(x: x0, y: top),
                                  control1: CGPoint(x: x0, y: start.y), control2: CGPoint(x: x0, y: top + 12))
                    path.addLine(to: CGPoint(x: x1, y: top))
                    path.addCurve(to: end,
                                  control1: CGPoint(x: x1, y: top + 12), control2: CGPoint(x: x1, y: end.y))
                } else if abs(l.midX - r.midX) < 40 {
                    let end = CGPoint(x: r.maxX, y: r.minY + iconY)
                    let bulge = max(l.maxX, r.maxX) + 40
                    path.move(to: start)
                    path.addCurve(to: end, control1: CGPoint(x: bulge, y: start.y), control2: CGPoint(x: bulge, y: end.y))
                } else {
                    let end = CGPoint(x: r.minX, y: r.minY + iconY)
                    let mid = (start.x + end.x) / 2
                    path.move(to: start)
                    path.addCurve(to: end, control1: CGPoint(x: mid, y: start.y), control2: CGPoint(x: mid, y: end.y))
                }
            }
        }
        .stroke(Theme.primary.opacity(0.85), style: StrokeStyle(lineWidth: 1.75, lineCap: .round, dash: [5, 4]))
    }
}
