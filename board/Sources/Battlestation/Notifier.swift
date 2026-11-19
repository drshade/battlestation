import Foundation
@preconcurrency import UserNotifications

/// macOS notification banners for the Deck. Like everything else in the app
/// it holds no protocol state: it diffs the open asks of successive worlds,
/// posts one banner per ask that newly appears, withdraws the banner when the
/// ask leaves the queue (answered anywhere), and turns a banner gesture into
/// the same bsctl verbs the Deck tab runs.
///
/// The first world after launch is a baseline, not news: asks already queued
/// when the app starts don't burst onto the screen.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    private let store: Store
    private let open: (Int) -> Void
    private var known: Set<Int>?
    private let center = UNUserNotificationCenter.current()

    private enum ID {
        static let ask = "ask", askOptions = "ask.options", note = "notify"
        static let reply = "reply", gotIt = "gotit", option = "opt."
    }

    /// `open(id)` shows that ask in the Deck tab (banner click).
    init(store: Store, open: @escaping (Int) -> Void) {
        self.store = store
        self.open = open
        super.init()
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
        let reply = UNTextInputNotificationAction(
            identifier: ID.reply, title: "Reply", options: [],
            textInputButtonTitle: "Send", textInputPlaceholder: "Answer the agent…")
        let gotIt = UNNotificationAction(identifier: ID.gotIt, title: "Got it")
        baseCategories = [
            UNNotificationCategory(identifier: ID.ask, actions: [reply], intentIdentifiers: []),
            UNNotificationCategory(identifier: ID.note, actions: [gotIt], intentIdentifiers: []),
        ]
        setCategories()
    }

    private var baseCategories: Set<UNNotificationCategory> = []
    /// Per-ask categories (option buttons), dropped when the ask closes.
    private var optionCategories: [Int: UNNotificationCategory] = [:]

    private func setCategories() {
        center.setNotificationCategories(baseCategories.union(optionCategories.values))
    }

    /// Called on every new world.
    func update(_ world: World) {
        let open = world.openAsks
        let ids = Set(open.map(\.id))
        defer { known = ids }
        guard let known else { return }
        for ask in open where !known.contains(ask.id) { post(ask, world) }
        let closed = known.subtracting(ids)
        let gone = closed.map(key)
        if !gone.isEmpty {
            let dropped = closed.compactMap { optionCategories.removeValue(forKey: $0) }
            if !dropped.isEmpty { setCategories() }
            center.removeDeliveredNotifications(withIdentifiers: gone)
            center.removePendingNotificationRequests(withIdentifiers: gone)
        }
    }

    private func key(_ id: Int) -> String { "ask-\(id)" }

    private func post(_ ask: Ask, _ world: World) {
        let who = world.agent(session: ask.session)
        let c = UNMutableNotificationContent()
        // "agent_one · api" — who is asking, from where.
        let name = who?.name ?? who?.kind ?? ask.kind ?? "agent"
        c.title = "\(name) · \(world.label(ws: who?.ws ?? ask.ws))"
        c.subtitle = ask.title ?? ""
        c.body = ask.body ?? ""
        c.threadIdentifier = ask.session ?? "deck"
        c.userInfo = ["id": ask.id]
        if ask.urgency != "low" { c.sound = .default }
        if ask.type == "notify" {
            c.categoryIdentifier = ID.note
        } else if let opts = ask.options, !opts.isEmpty {
            // Categories are static per identifier, so an ask with options
            // gets its own: one button per option, then Reply.
            let cat = "\(ID.askOptions).\(ask.id)"
            let buttons = opts.enumerated().map { i, o in
                UNNotificationAction(identifier: "\(ID.option)\(i)", title: o)
            }
            let reply = UNTextInputNotificationAction(
                identifier: ID.reply, title: "Reply", options: [],
                textInputButtonTitle: "Send", textInputPlaceholder: "Answer the agent…")
            optionCategories[ask.id] = UNNotificationCategory(
                identifier: cat, actions: buttons + [reply], intentIdentifiers: [])
            setCategories()
            c.categoryIdentifier = cat
        } else {
            c.categoryIdentifier = ID.ask
        }
        center.add(UNNotificationRequest(identifier: key(ask.id), content: c, trigger: nil))
    }

    // MARK: UNUserNotificationCenterDelegate

    /// Show the banner even while the app is active — it is a menu-bar app,
    /// "active" usually means only that its popover or board was last used.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler done: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        done([.banner, .list, .sound])
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler done: @escaping () -> Void
    ) {
        let info = response.notification.request.content.userInfo
        let action = response.actionIdentifier
        let text = (response as? UNTextInputNotificationResponse)?.userText
        guard let id = info["id"] as? Int else { done(); return }
        Task { @MainActor in
            self.respond(id: id, action: action, text: text)
            done()
        }
    }

    private func respond(id: Int, action: String, text: String?) {
        let ask = store.world.allAsks.first { $0.id == id }
        switch action {
        case ID.reply:
            if let ask, let text { store.submit(ask, text) }
        case ID.gotIt:
            store.run(["asks", "dismiss", String(id)])
        case let a where a.hasPrefix(ID.option):
            if let ask, let i = Int(a.dropFirst(ID.option.count)),
               let opts = ask.options, opts.indices.contains(i) {
                store.submit(ask, opts[i])
            }
        default: // the banner itself (or "Show")
            open(id)
        }
    }
}
