import SwiftUI

/// The Deck: the agents' attention queue, in the human's order (the stream
/// already resolves it: ordered open asks, then FIFO, then answered). Every
/// control is the bsctl verb the Linux panel runs (Panel.qml, asks mode).
struct DeckView: View {
    @Environment(Store.self) private var store
    @Environment(\.isSnapshot) private var isSnapshot
    @State private var project: Int?
    @AppStorage("hideDelivered") private var hideDelivered = true

    private var world: World { store.world }

    private var shown: [Ask] {
        world.allAsks.filter { k in
            (project == nil || k.ws == project)
                && !(hideDelivered && !k.isOpen && k.delivered_at != nil)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            header
            chips
            if shown.isEmpty {
                ContentUnavailableView("Nothing on the Deck", systemImage: "tray",
                                       description: Text("Agents post here when they need you."))
            } else if isSnapshot {
                list
            } else {
                ScrollView { list }
            }
        }
    }

    private var header: some View {
        let open = world.openAsks
        return HStack(spacing: 12) {
            Text("\(open.count) open").font(.headline)
            let blocking = open.filter(\.isBlocking).count
            if blocking > 0 {
                Text("\(blocking) blocking").font(.subheadline).foregroundStyle(Theme.thinking)
            }
            Spacer()
            Toggle("Deck", isOn: Binding(get: { store.deckOn }, set: { store.setDeck($0) }))
                .toggleStyle(BSCheckbox())
                .help("On: agents are steered to post here. Off: they ask inline in their terminal.")
            Toggle("Hide delivered", isOn: $hideDelivered).toggleStyle(BSCheckbox())
        }
    }

    private var chips: some View {
        let lanes = Array(Set(world.allAsks.compactMap(\.ws))).sorted()
        return HStack(spacing: 6) {
            chip("All projects", selected: project == nil) { project = nil }
            ForEach(lanes, id: \.self) { ws in
                chip(world.label(ws: ws), selected: project == ws) { project = ws }
            }
        }
    }

    private func chip(_ text: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(text).font(.caption)
                .padding(.horizontal, 10).padding(.vertical, 3)
                .foregroundStyle(selected ? Theme.primary : Theme.fg)
                .background(Capsule().fill(selected ? Theme.primary.opacity(0.15) : Theme.surface))
                .overlay(Capsule().stroke(selected ? Theme.primary : Theme.line))
        }
        .buttonStyle(.plain)
    }

    private var list: some View {
        let open = world.openAsks.map(\.id)
        return VStack(spacing: 8) {
            ForEach(shown) { k in
                AskCard(ask: k, expanded: store.expandedAsk == k.id,
                        canUp: k.isOpen && open.first != k.id,
                        canDown: k.isOpen && open.last != k.id,
                        onMove: { move(k.id, by: $0) })
            }
        }
        .frame(maxWidth: 920, alignment: .leading)
    }

    /// Reorder within the open asks and persist the full order (`asks order
    /// set` — the human owns the order; agents never reorder).
    private func move(_ id: Int, by delta: Int) {
        var ids = world.openAsks.map(\.id)
        guard let i = ids.firstIndex(of: id) else { return }
        let j = min(max(i + delta, 0), ids.count - 1)
        ids.swapAt(i, j)
        store.run(["asks", "order", "set"] + ids.map(String.init))
    }
}

private struct AskCard: View {
    @Environment(Store.self) private var store
    var ask: Ask
    var expanded: Bool
    var canUp: Bool
    var canDown: Bool
    var onMove: (Int) -> Void
    @State private var draft = ""
    @State private var saveTask: Task<Void, Never>?
    @Environment(\.isSnapshot) private var isSnapshot

    private var asker: Agent? { store.world.agent(session: ask.session) }

    /// Panel.qml's willWake: the asker is idle at its prompt and not parked in
    /// the blocking RPC, so answering should also type the wake line.
    private var willWake: Bool { !ask.isBlocking && asker?.state == .waiting }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            head
            meta
            if expanded { detail }
        }
        .padding(10)
        .padding(.leading, 3)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 9).fill(Theme.card))
        .overlay(alignment: .leading) { Rectangle().fill(Theme.urgency(ask.urgency)).frame(width: 3) }
        .clipShape(RoundedRectangle(cornerRadius: 9))
        .overlay(RoundedRectangle(cornerRadius: 9).stroke(Theme.line))
        .opacity(ask.isOpen ? 1 : 0.7)
        .onAppear { draft = ask.isOpen ? (ask.answer ?? "") : "" }
    }

    private var head: some View {
        HStack(spacing: 10) {
            Text(askGlyph(ask.type)).font(.system(.body, design: .monospaced).weight(.semibold))
                .foregroundStyle(Theme.muted).frame(width: 16)
            Text(ask.title ?? "(untitled)").font(.body.weight(.medium)).lineLimit(expanded ? nil : 1)
            Spacer()
            if ask.isOpen {
                Button { onMove(-1) } label: { Image(systemName: "chevron.up") }
                    .buttonStyle(.bsIcon).disabled(!canUp).help("Move up")
                Button { onMove(1) } label: { Image(systemName: "chevron.down") }
                    .buttonStyle(.bsIcon).disabled(!canDown).help("Move down")
            }
            // Jump needs a LIVE tracked asker: an ask can outlive its session,
            // or carry an empty session when the poster's identity didn't
            // resolve (bsctl still accepts it) — there is no window to go to.
            Button("Jump ↵") { if let s = asker?.session { store.run(["ws", "focus", "--session", s]) } }
                .buttonStyle(.bs).disabled(asker == nil)
                .help(asker == nil ? "The asking session has ended or was never tracked, so there is no window to jump to."
                                   : "Jump to the asking agent's window")
            Button(expanded ? "Collapse" : "Open") { store.expandedAsk = expanded ? nil : ask.id }
                .buttonStyle(.bs)
            Button { store.run(["asks", "dismiss", String(ask.id)]) } label: { Image(systemName: "xmark") }
                .buttonStyle(.bsIcon).help("Dismiss")
        }
    }

    private var meta: some View {
        HStack(spacing: 6) {
            Chip(text: store.world.label(ws: ask.ws))
            HStack(spacing: 4) {
                BotIcon(kind: ask.kind ?? "claude", status: asker?.state ?? .waiting, size: 12)
                Text(asker?.name ?? ask.kind ?? "agent").font(Theme.mono).foregroundStyle(Theme.muted)
            }
            .padding(.horizontal, 5).padding(.vertical, 1)
            .overlay(RoundedRectangle(cornerRadius: 4).stroke(Theme.line))
            Chip(text: "#\(ask.id)")
            if let c = ask.created { Chip(text: fmtAge(Date().timeIntervalSince1970 - c) + " ago") }
            if let u = ask.urgency { Chip(text: u, color: Theme.urgency(u)) }
            if let m = ask.estimate_min, m > 0 { Chip(text: "~\(Int(m))m of you") }
            if ask.isBlocking { Chip(text: "▹ BLOCKING", color: Theme.thinking) }
            if let n = ask.note, !n.isEmpty { Chip(text: "note: \(n)", color: Theme.primary) }
            if !ask.isOpen {
                Chip(text: ask.delivered_at != nil ? "delivered ✓" : "answered — awaiting pickup",
                     color: ask.delivered_at != nil ? Theme.thinking : Theme.primary)
            }
        }
    }

    @ViewBuilder private var detail: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                BotIcon(kind: ask.kind ?? "claude", status: asker?.state ?? .waiting, size: 12)
                Text("\(ask.kind ?? "agent") · \(store.world.label(ws: ask.ws)) · "
                     + (ask.isBlocking ? "waiting on you" : (asker?.state.label ?? "session ended")))
                    .font(Theme.mono).foregroundStyle(Theme.muted)
            }
            .padding(.horizontal, 10).padding(.vertical, 5)
            Divider()
            Text(ask.body ?? "").font(.system(.callout, design: .monospaced))
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(10)
        }
        .background(RoundedRectangle(cornerRadius: 7).fill(Theme.term))
        .overlay(RoundedRectangle(cornerRadius: 7).stroke(Theme.line))

        if ask.isOpen {
            if ask.type == "notify" {
                Button("Got it") { store.run(["asks", "dismiss", String(ask.id)]) }
                    .buttonStyle(.bsPrimary)
            } else {
                if let opts = ask.options, !opts.isEmpty {
                    HStack { ForEach(opts, id: \.self) { o in Button(o) { submit(o) }.buttonStyle(.bs) } }
                }
                replyBox
                HStack(spacing: 6) {
                    Button(willWake ? "Trigger ↵" : "Enqueue ↵") { submit(draft) }
                        .buttonStyle(.bsPrimary)
                        .keyboardShortcut(.return, modifiers: .command)
                        .help(willWake ? "The agent is idle: answer and wake it."
                                       : "The agent collects this at its next turn (or its blocking call returns it).")
                    ForEach(["working on it", "later", "your call", "need detail"], id: \.self) { tag in
                        Button(tag) { store.run(["asks", "note", String(ask.id), tag]) }.buttonStyle(.bs)
                    }
                }
            }
        } else {
            HStack {
                Text("Answer: " + ((ask.answer ?? "").isEmpty ? "(acknowledged)" : ask.answer!))
                    .font(.callout).foregroundStyle(Theme.muted)
                Spacer()
                Button("Reopen") { store.run(["asks", "reopen", String(ask.id)]) }.buttonStyle(.bs)
            }
        }
    }

    /// The reply box. Offscreen (--snapshot) the native editor can't draw, so
    /// a static stand-in shows the draft instead.
    @ViewBuilder private var replyBox: some View {
        Group {
            if isSnapshot {
                Text(draft.isEmpty ? "Type a reply. Drafts save as you type." : draft)
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(draft.isEmpty ? Theme.muted : Theme.fg)
                    .frame(maxWidth: .infinity, minHeight: 64, alignment: .topLeading)
                    .padding(8)
            } else {
                TextEditor(text: $draft)
                    .font(.system(.callout, design: .monospaced))
                    .frame(minHeight: 64)
                    .scrollContentBackground(.hidden)
                    .padding(4)
                    .onChange(of: draft) { _, text in autosave(text) }
            }
        }
        .background(RoundedRectangle(cornerRadius: 7).fill(Theme.term))
        .overlay(RoundedRectangle(cornerRadius: 7).stroke(Theme.line))
    }

    /// Draft auto-save: `asks reply` WITHOUT completing (a blocked asker stays
    /// parked; a peeking agent sees the draft). Debounced like the panel.
    private func autosave(_ text: String) {
        saveTask?.cancel()
        let id = ask.id
        saveTask = Task {
            try? await Task.sleep(for: .milliseconds(1500))
            guard !Task.isCancelled else { return }
            store.run(["asks", "reply", String(id), text])
        }
    }

    /// Panel.qml's submitAnswer: store the answer (answer, or complete when
    /// empty); when the asker is idle, also fire the wake trigger.
    private func submit(_ text: String) {
        saveTask?.cancel()
        let wake = willWake
        store.run(text.isEmpty ? ["asks", "complete", String(ask.id)] : ["asks", "answer", String(ask.id), text])
        if wake { store.run(["asks", "wake", String(ask.id)]) }
        store.expandedAsk = nil
    }
}

/// The Switchboard as a matrix: a precise alternative to drawing links on
/// the Board. Links are two-way and never transitive; only named agents can
/// be linked.
struct LinksView: View {
    @Environment(Store.self) private var store

    var body: some View {
        let w = store.world
        let named = w.allAgents.filter { $0.name != nil }
        let unnamed = w.allAgents.count - named.count
        VStack(alignment: .leading, spacing: 10) {
            Text("Links are private, two-way and not transitive: if A↔B and B↔C, A and C still can't talk.")
                .font(.callout).foregroundStyle(Theme.muted)
            if unnamed > 0 {
                Text("\(unnamed) live agent\(unnamed == 1 ? " hasn't" : "s haven't") picked a name yet (agents call set_name), so \(unnamed == 1 ? "it" : "they") can't be linked.")
                    .font(.caption).foregroundStyle(Theme.primary)
            }
            if named.count < 2 {
                ContentUnavailableView("Two named agents are needed to make a link", systemImage: "point.3.connected.trianglepath.dotted")
            } else {
                Grid(alignment: .center, horizontalSpacing: 0, verticalSpacing: 0) {
                    GridRow {
                        Color.clear.gridCellUnsizedAxes([.horizontal, .vertical])
                        ForEach(named) { b in header(b, w) }
                    }
                    ForEach(named) { a in
                        GridRow {
                            rowHeader(a, w)
                            ForEach(named) { b in cell(a, b, w) }
                        }
                    }
                }
                .overlay(Rectangle().stroke(Theme.line))
            }
            Spacer()
        }
    }

    private func header(_ a: Agent, _ w: World) -> some View {
        VStack(spacing: 1) {
            Text(a.name ?? "").font(.caption.weight(.semibold))
            Text(w.label(ws: a.ws)).font(Theme.mono).foregroundStyle(Theme.muted)
        }
        .padding(6).frame(minWidth: 90)
        .background(Theme.surface).border(Theme.line)
    }

    private func rowHeader(_ a: Agent, _ w: World) -> some View {
        HStack(spacing: 6) {
            Circle().fill(Theme.status(a.state)).frame(width: 7, height: 7)
            VStack(alignment: .leading, spacing: 1) {
                Text(a.name ?? "").font(.caption.weight(.semibold))
                Text("\(w.label(ws: a.ws)) · \(w.unread(a)) unread").font(Theme.mono).foregroundStyle(Theme.muted)
            }
        }
        .padding(6).frame(minWidth: 140, alignment: .leading)
        .background(Theme.surface).border(Theme.line)
    }

    @ViewBuilder private func cell(_ a: Agent, _ b: Agent, _ w: World) -> some View {
        if a.session == b.session {
            Text("—").foregroundStyle(Theme.muted).frame(minWidth: 90, minHeight: 38).border(Theme.line)
        } else {
            Toggle("", isOn: Binding(
                get: { w.linked(a, b) },
                set: { on in
                    store.run(["comms", on ? "link" : "unlink",
                               "--a-workspace", w.label(ws: a.ws), "--a-name", a.name ?? "",
                               "--b-workspace", w.label(ws: b.ws), "--b-name", b.name ?? ""])
                }))
            .toggleStyle(BSCheckbox()).labelsHidden()
            .frame(minWidth: 90, minHeight: 38).border(Theme.line)
            .help("Link \(a.name ?? "") and \(b.name ?? "")")
        }
    }
}
