import Foundation

// The world as `bsctl status --format json` publishes it (contract:
// ctl/src/lib.rs, "The world"). Every field is optional on decode: the app
// renders whatever arrives and never rejects a line over a missing key.

struct World: Decodable, Equatable {
    var asks: [Ask]? = []
    var displays: [Display]?
    var workspaces: [Workspace]?
    var agents: [Agent]? = []
    var comms: Comms?
    var usage: [String: Usage]?
    var human: Human?

    var allAsks: [Ask] { asks ?? [] }
    var allAgents: [Agent] { agents ?? [] }
    var openAsks: [Ask] { allAsks.filter { $0.isOpen } }

    /// Battlespaces in bs order (the stream already sorts; sorting again is free).
    var lanes: [Workspace] { (workspaces ?? []).sorted { $0.bs < $1.bs } }

    /// The Switchboard's public workspace label for a ws-id: its name, else
    /// "ws N" — the same rule bsctl's comms uses, so link endpoints match.
    func label(ws: Int?) -> String {
        guard let ws else { return "unknown workspace" }
        if let name = workspaces?.first(where: { $0.ws == ws })?.name, !name.isEmpty { return name }
        return "ws \(ws)"
    }

    func agent(session: String?) -> Agent? {
        guard let session else { return nil }
        return allAgents.first { $0.session == session }
    }

    /// The agent behind a public Switchboard endpoint (name + workspace label).
    func agent(for e: Endpoint) -> Agent? {
        allAgents.first { $0.name == e.name && label(ws: $0.ws) == e.workspace }
    }

    func unread(_ a: Agent) -> Int {
        guard let name = a.name else { return 0 }
        let lbl = label(ws: a.ws)
        return comms?.agents?.first { $0.name == name && $0.workspace == lbl }?.unread ?? 0
    }

    /// Links resolved to session pairs (unresolvable endpoints dropped).
    var sessionLinks: [(String, String)] {
        (comms?.links ?? []).compactMap { l in
            guard let a = agent(for: l.a), let b = agent(for: l.b) else { return nil }
            return (a.session, b.session)
        }
    }

    func linked(_ a: Agent, _ b: Agent) -> Bool {
        sessionLinks.contains { ($0.0 == a.session && $0.1 == b.session) || ($0.0 == b.session && $0.1 == a.session) }
    }
}

struct Workspace: Decodable, Equatable, Identifiable {
    var ws: Int
    var bs: Int
    var name: String?
    var display: String?
    var windows: Int?
    var active: Bool?
    var id: Int { ws }
}

struct Display: Decodable, Equatable {
    var id: Int
    var name: String
    var focused: Bool?
    var activeWs: Int?
}

struct Agent: Decodable, Equatable, Identifiable {
    var session: String
    var kind: String?
    var status: String?
    var ws: Int?
    var title: String?
    var name: String?
    var subagents: [Subagent]?
    var id: String { session }
    var harness: String { kind ?? "claude" }
    var state: AgentStatus { AgentStatus(raw: status) }
    var squad: [Subagent] { subagents ?? [] }
}

struct Subagent: Decodable, Equatable, Identifiable {
    var id: String
    var type: String?
    var description: String?
    var started: Double?
}

enum AgentStatus: String {
    case thinking, tooling, waiting
    init(raw: String?) { self = AgentStatus(rawValue: raw ?? "") ?? .waiting }
    /// The asset stem (the bar plugin's files use `tool` for tooling).
    var stem: String { self == .tooling ? "tool" : rawValue }
    var label: String {
        switch self {
        case .thinking: return "thinking"
        case .tooling: return "using a tool"
        case .waiting: return "waiting on you"
        }
    }
    var busy: Bool { self != .waiting }
}

struct Ask: Decodable, Equatable, Identifiable {
    var id: Int
    var session: String?
    var kind: String?
    var ws: Int?
    var type: String?
    var title: String?
    var body: String?
    var options: [String]?
    var urgency: String?
    var estimate_min: Double?
    var note: String?
    var state: String?
    var answer: String?
    var created: Double?
    var answered_at: Double?
    var delivered_at: Double?
    var blocking: Bool?
    var isOpen: Bool { state == "open" }
    var isBlocking: Bool { blocking == true }
}

struct Comms: Decodable, Equatable {
    var agents: [CommsAgent]?
    var links: [Link]?
}

struct CommsAgent: Decodable, Equatable {
    var workspace: String
    var name: String?
    var kind: String?
    var status: String?
    var unread: Int?
}

struct Link: Decodable, Equatable {
    var a: Endpoint
    var b: Endpoint
}

struct Endpoint: Decodable, Equatable, Hashable {
    var workspace: String
    var name: String
}

struct Usage: Decodable, Equatable {
    var sessionPct: Double?
    var weeklyPct: Double?
    /// When the reading was taken, if the provider knows (codex: its last turn).
    var asOf: String?
}

struct Human: Decodable, Equatable {
    var state: String?
    var idle_secs: Double?
}

/// "42s", "3m", "1h 5m" — the bar plugin's elapsed format.
func fmtAge(_ seconds: Double) -> String {
    let s = max(0, Int(seconds))
    if s < 60 { return "\(s)s" }
    let m = s / 60
    if m < 60 { return "\(m)m" }
    return "\(m / 60)h \(m % 60)m"
}

extension ISO8601DateFormatter {
    /// RFC 3339 with or without fractional seconds.
    static func lenient(_ s: String) -> Date? {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let d = f.date(from: s) { return d }
        f.formatOptions = [.withInternetDateTime]
        return f.date(from: s)
    }
}
