import SwiftUI

// The app's own controls — squared, quiet, in the Deck wireframes' style
// (wireframes/battlestation-workspaces). Pure SwiftUI on purpose: they look
// the same in light and dark, and they render in --snapshot (AppKit-backed
// controls draw as placeholders offscreen).

/// Buttons: `.plain` (bordered, quiet), `.primary` (amber fill), `.icon`.
struct BSButtonStyle: ButtonStyle {
    enum Kind { case plain, primary, icon }
    var kind: Kind = .plain
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(kind == .icon ? .system(size: 11, weight: .semibold) : .callout)
            .foregroundStyle(kind == .primary ? Theme.bg : Theme.fg)
            .padding(.horizontal, kind == .icon ? 6 : 10)
            .padding(.vertical, kind == .icon ? 4 : 4)
            .background(RoundedRectangle(cornerRadius: 5)
                .fill(kind == .primary ? Theme.primary : Theme.surface))
            .overlay(RoundedRectangle(cornerRadius: 5)
                .stroke(kind == .primary ? Theme.primary : Theme.line))
            .opacity(isEnabled ? (configuration.isPressed ? 0.7 : 1) : 0.35)
            .contentShape(Rectangle())
    }
}

extension ButtonStyle where Self == BSButtonStyle {
    static var bs: BSButtonStyle { BSButtonStyle() }
    static var bsPrimary: BSButtonStyle { BSButtonStyle(kind: .primary) }
    static var bsIcon: BSButtonStyle { BSButtonStyle(kind: .icon) }
}

/// A checkbox: square + label.
struct BSCheckbox: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        Button { configuration.isOn.toggle() } label: {
            HStack(spacing: 6) {
                Image(systemName: configuration.isOn ? "checkmark.square.fill" : "square")
                    .foregroundStyle(configuration.isOn ? Theme.primary : Theme.muted)
                configuration.label.font(.callout).foregroundStyle(Theme.fg)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// A toggle that reads as a pill button: amber when on.
struct BSPillToggle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        Button { configuration.isOn.toggle() } label: {
            configuration.label
                .font(.callout)
                .foregroundStyle(configuration.isOn ? Theme.primary : Theme.fg)
                .padding(.horizontal, 10).padding(.vertical, 4)
                .background(RoundedRectangle(cornerRadius: 5)
                    .fill(configuration.isOn ? Theme.primary.opacity(0.15) : Theme.surface))
                .overlay(RoundedRectangle(cornerRadius: 5)
                    .stroke(configuration.isOn ? Theme.primary : Theme.line))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// The tab switcher: a squared segmented control.
struct TabSwitcher: View {
    @Binding var tab: Store.Tab
    var deckCount: Int

    var body: some View {
        HStack(spacing: 2) {
            ForEach(Store.Tab.allCases) { t in
                Button { tab = t } label: {
                    Text(t == .deck && deckCount > 0 ? "Deck (\(deckCount))" : t.rawValue)
                        .font(.callout.weight(tab == t ? .semibold : .regular))
                        .foregroundStyle(tab == t ? Theme.fg : Theme.muted)
                        .padding(.horizontal, 12).padding(.vertical, 4)
                        .background(RoundedRectangle(cornerRadius: 5).fill(tab == t ? Theme.card : .clear))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(2)
        .background(RoundedRectangle(cornerRadius: 7).fill(Theme.surface))
        .overlay(RoundedRectangle(cornerRadius: 7).stroke(Theme.line))
    }
}

/// A thin percentage bar.
struct Meter: View {
    var value: Double // 0...100
    var body: some View {
        ZStack(alignment: .leading) {
            Capsule().fill(Theme.line)
            Capsule().fill(Theme.primary).frame(width: 54 * min(max(value, 0), 100) / 100)
        }
        .frame(width: 54, height: 5)
    }
}
