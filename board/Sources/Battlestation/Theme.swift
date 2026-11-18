import AppKit
import SwiftUI

/// Colour tokens from the Deck wireframes' palette (wireframes/
/// battlestation-workspaces/README.md), with a light counterpart. Status
/// colours match the harness SVGs' own fills so a dot and a bot agree.
enum Theme {
    static let bg = dyn(0xF4F0E8, 0x16130F)
    static let surface = dyn(0xFBF8F2, 0x1B1813)
    static let card = dyn(0xFFFFFF, 0x221D17)
    static let term = dyn(0xF1ECE2, 0x0F0C08)
    static let line = dyn(0xDDD4C4, 0x352E25)
    static let fg = dyn(0x2A241B, 0xF2EDE2)
    static let muted = dyn(0x776C5C, 0xA89E8F)
    static let primary = dyn(0xA8721F, 0xD9A04A)

    static let thinking = Color(hex: 0x3FB950)
    static let tooling = Color(hex: 0xA371F7)
    static let waiting = Color(hex: 0xD97757)

    static func status(_ s: AgentStatus) -> Color {
        switch s {
        case .thinking: return thinking
        case .tooling: return tooling
        case .waiting: return waiting
        }
    }

    static func urgency(_ u: String?) -> Color {
        switch u {
        case "high": return dyn(0xC4533B, 0xE0765F)
        case "medium": return primary
        default: return dyn(0x8C8274, 0x9A8F7D)
        }
    }

    static let mono = Font.system(.caption, design: .monospaced)

    static func dyn(_ light: UInt32, _ dark: UInt32) -> Color {
        Color(nsColor: NSColor(name: nil) { ap in
            ap.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? NSColor(hex: dark) : NSColor(hex: light)
        })
    }
}

extension NSColor {
    convenience init(hex: UInt32) {
        self.init(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
                  blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
}

extension Color {
    init(hex: UInt32) { self.init(nsColor: NSColor(hex: hex)) }
}

/// A small bordered tag (kind, #id, age, …).
struct Chip: View {
    var text: String
    var color: Color = Theme.muted
    var body: some View {
        Text(text)
            .font(Theme.mono)
            .foregroundStyle(color)
            .padding(.horizontal, 5)
            .padding(.vertical, 1)
            .overlay(RoundedRectangle(cornerRadius: 4).stroke(Theme.line))
    }
}
