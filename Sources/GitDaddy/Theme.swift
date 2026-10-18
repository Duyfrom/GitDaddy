import AppKit
import SwiftUI

enum Theme {
    // MARK: - Stepped Surfaces & Borders

    static let canvas = Color(nsColor: adaptiveColor(
        dark: (0x11, 0x11, 0x13, 1.0),
        light: (0xF4, 0xF4, 0xF5, 1.0)
    ))

    static let surface1 = Color(nsColor: adaptiveColor(
        dark: (0x18, 0x18, 0x1B, 1.0),
        light: (0xFF, 0xFF, 0xFF, 1.0)
    ))

    static let surface2 = Color(nsColor: adaptiveColor(
        dark: (0x20, 0x20, 0x24, 1.0),
        light: (0xF0, 0xF0, 0xF2, 1.0)
    ))

    static let border = Color(nsColor: adaptiveColor(
        dark: (0xFF, 0xFF, 0xFF, 0.08),
        light: (0x00, 0x00, 0x00, 0.08)
    ))

    static let borderStrong = Color(nsColor: adaptiveColor(
        dark: (0xFF, 0xFF, 0xFF, 0.14),
        light: (0x00, 0x00, 0x00, 0.14)
    ))

    // MARK: - Text Hierarchy

    static let textPrimary = Color(nsColor: adaptiveColor(
        dark: (0xFF, 0xFF, 0xFF, 0.92),
        light: (0x00, 0x00, 0x00, 0.88)
    ))

    static let textSecondary = Color(nsColor: adaptiveColor(
        dark: (0xFF, 0xFF, 0xFF, 0.62),
        light: (0x00, 0x00, 0x00, 0.58)
    ))

    static let textTertiary = Color(nsColor: adaptiveColor(
        dark: (0xFF, 0xFF, 0xFF, 0.42),
        light: (0x00, 0x00, 0x00, 0.42)
    ))

    // MARK: - Single Accent

    static let accent = Color(red: 0x5B / 255.0, green: 0x8D / 255.0, blue: 0xEF / 255.0)
    static let accentText = Color.white

    // MARK: - Semantic Status Colors (Green = Good, Yellow = Notice, Red = Bad)

    /// Good: Up to Date / Clean / Succeeded (Emerald Green #34D399)
    static let statusGood = Color(red: 0x34 / 255.0, green: 0xD3 / 255.0, blue: 0x99 / 255.0)

    /// Notice: Updates Available / Unpushed / Dirty / In Progress (Amber Yellow #FBBF24)
    static let statusNotice = Color(red: 0xFB / 255.0, green: 0xBF / 255.0, blue: 0x24 / 255.0)

    /// Bad: Sync Error / Diverged / Missing Remote (Coral Red #F87171)
    static let statusBad = Color(red: 0xF8 / 255.0, green: 0x71 / 255.0, blue: 0x71 / 255.0)

    // Backwards-compatible aliases
    static var statusSuccess: Color { statusGood }
    static var statusWarning: Color { statusNotice }
    static var statusInfo: Color { statusNotice }
    static var statusDanger: Color { statusBad }

    // MARK: - Typography (3 sizes: 12, 13, 15; 2 weights: 400, 500)

    static let fsMeta: CGFloat = 12
    static let fsBody: CGFloat = 13
    static let fsTitle: CGFloat = 15

    static let trackingBody: CGFloat = -0.13
    static let trackingTitle: CGFloat = -0.22

    static func fontMeta(weight: Font.Weight = .regular, monospaced: Bool = false) -> Font {
        monospaced
            ? .system(size: fsMeta, weight: weight, design: .monospaced)
            : .system(size: fsMeta, weight: weight, design: .default)
    }

    static func fontBody(weight: Font.Weight = .regular, monospaced: Bool = false) -> Font {
        monospaced
            ? .system(size: fsBody, weight: weight, design: .monospaced)
            : .system(size: fsBody, weight: weight, design: .default)
    }

    static func fontTitle(weight: Font.Weight = .medium) -> Font {
        .system(size: fsTitle, weight: weight, design: .default)
    }

    // MARK: - Geometry & 4px Grid

    static let rowHeight: CGFloat = 32
    static let rowCompact: CGFloat = 28
    static let iconSize: CGFloat = 16
    static let iconCompact: CGFloat = 14
    static let radiusControl: CGFloat = 6
    static let radiusContainer: CGFloat = 8

    static let space1: CGFloat = 4
    static let space2: CGFloat = 8
    static let space3: CGFloat = 12
    static let space4: CGFloat = 16
    static let space6: CGFloat = 24
    static let space8: CGFloat = 32

    private static func adaptiveColor(
        dark: (Int, Int, Int, CGFloat),
        light: (Int, Int, Int, CGFloat)
    ) -> NSColor {
        NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            let c = isDark ? dark : light
            return NSColor(
                srgbRed: CGFloat(c.0) / 255.0,
                green: CGFloat(c.1) / 255.0,
                blue: CGFloat(c.2) / 255.0,
                alpha: c.3
            )
        }
    }
}

// MARK: - Canonical Status Icon Component

enum CanonicalStatusKind {
    case allRepos
    case success
    case inProgress
    case draft
    case needsUpdate
    case dirty
    case unpushed
    case unpublished
    case diverged
    case danger
    case disabled

    var symbolName: String {
        switch self {
        case .allRepos:
            return "tray"
        case .success:
            return "checkmark.circle"
        case .inProgress:
            return "circle.dashed"
        case .draft:
            return "circle.dashed"
        case .needsUpdate:
            return "arrow.down.circle"
        case .dirty:
            return "pencil.circle"
        case .unpushed:
            return "arrow.up.circle"
        case .unpublished:
            return "arrow.up.right.circle"
        case .diverged:
            return "arrow.up.arrow.down.circle"
        case .danger:
            return "exclamationmark.triangle"
        case .disabled:
            return "slash.circle"
        }
    }

    /// Strictly: Green = Good, Yellow = Notice, Red = Bad
    func strokeColor(mutedWhenNormal: Bool = false) -> Color {
        switch self {
        case .allRepos:
            return Theme.textSecondary
        case .success:
            return Theme.statusGood
        case .needsUpdate, .dirty, .unpushed, .unpublished, .inProgress:
            return Theme.statusNotice
        case .diverged, .danger, .disabled:
            return Theme.statusBad
        case .draft:
            return Theme.textTertiary
        }
    }
}

extension SyncState {
    var canonicalStatus: CanonicalStatusKind {
        switch self {
        case .unknown:
            return .draft
        case .checking, .pulling, .pushing:
            return .inProgress
        case .upToDate:
            return .success
        case .behind:
            return .needsUpdate
        case .diverged:
            return .diverged
        case .ahead:
            return .unpushed
        case .unpublishedBranch:
            return .unpublished
        case .noRemoteConfigured, .emptyRepo, .detachedHead:
            return .disabled
        case .error:
            return .danger
        }
    }
}

/// Dedicated Canvas spinner using TimelineView so NO SwiftUI view or CALayer ever has a rotationEffect animation.
struct DedicatedSpinIndicator: View {
    let size: CGFloat

    var body: some View {
        TimelineView(.animation(minimumInterval: 1.0 / 30.0)) { context in
            Canvas { graphicsContext, canvasSize in
                let t = context.date.timeIntervalSinceReferenceDate
                let startAngle = Angle.degrees((t.truncatingRemainder(dividingBy: 0.75) / 0.75) * 360.0)
                let endAngle = startAngle + Angle.degrees(250.0)

                let center = CGPoint(x: canvasSize.width / 2.0, y: canvasSize.height / 2.0)
                let radius = max(2.0, (min(canvasSize.width, canvasSize.height) - 3.0) / 2.0)

                // Subtle track ring
                var trackPath = Path()
                trackPath.addArc(
                    center: center,
                    radius: radius,
                    startAngle: .degrees(0),
                    endAngle: .degrees(360),
                    clockwise: false
                )
                graphicsContext.stroke(
                    trackPath,
                    with: .color(Theme.borderStrong),
                    lineWidth: 1.5
                )

                // Active spinning arc in Notice (Yellow) color
                var arcPath = Path()
                arcPath.addArc(
                    center: center,
                    radius: radius,
                    startAngle: startAngle,
                    endAngle: endAngle,
                    clockwise: false
                )
                graphicsContext.stroke(
                    arcPath,
                    with: .color(Theme.statusNotice),
                    style: StrokeStyle(lineWidth: 1.5, lineCap: .round)
                )
            }
            .frame(width: size, height: size)
        }
    }
}

struct StatusIcon: View {
    let state: SyncState
    var compact: Bool = false
    var showLabel: Bool = false
    var mutedNormal: Bool = false

    private var size: CGFloat {
        compact ? Theme.iconCompact : Theme.iconSize
    }

    var body: some View {
        HStack(spacing: compact ? 6 : Theme.space2) {
            ZStack {
                if state.isBusy {
                    DedicatedSpinIndicator(size: size)
                        .id("spinner")
                } else {
                    Image(systemName: state.canonicalStatus.symbolName)
                        .symbolRenderingMode(.monochrome)
                        .font(.system(size: size - 2, weight: .medium))
                        .foregroundColor(state.canonicalStatus.strokeColor())
                        .foregroundStyle(state.canonicalStatus.strokeColor())
                        .id("icon-\(state.canonicalStatus.symbolName)")
                }
            }
            .frame(width: size, height: size)
            .transaction { transaction in
                transaction.animation = nil
            }

            if showLabel {
                Text(state.label)
                    .font(Theme.fontBody(weight: .regular))
                    .tracking(Theme.trackingBody)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
            }
        }
        .help(state.label + " — " + state.inspectorExplanation)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(state.label)
    }
}

// MARK: - Standardized Button Components

struct RestyleButton: View {
    enum Variant {
        case primary
        case secondary
        case ghost
    }

    let label: String
    var icon: String? = nil
    var variant: Variant = .secondary
    var compact: Bool = false
    var isDisabled: Bool = false
    let action: () -> Void

    @State private var isHovered: Bool = false

    private var height: CGFloat {
        compact ? Theme.rowCompact : Theme.rowHeight
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: compact ? 6 : Theme.space2) {
                if let icon {
                    Image(systemName: icon)
                        .font(.system(size: (compact ? Theme.iconCompact : Theme.iconSize) - 2, weight: .regular))
                        .foregroundStyle(iconColor)
                }
                Text(label)
                    .font(compact ? Theme.fontMeta(weight: .medium) : Theme.fontBody(weight: .medium))
                    .tracking(Theme.trackingBody)
                    .foregroundStyle(textColor)
                    .lineLimit(1)
            }
            .padding(.horizontal, Theme.space3)
            .frame(height: height)
            .background(backgroundColor, in: RoundedRectangle(cornerRadius: Theme.radiusControl))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.radiusControl)
                    .strokeBorder(borderColor, lineWidth: variant == .secondary ? 1 : 0)
            )
            .contentShape(RoundedRectangle(cornerRadius: Theme.radiusControl))
        }
        .buttonStyle(.plain)
        .disabled(isDisabled)
        .opacity(isDisabled ? 0.45 : 1.0)
        .onHover { hovering in
            isHovered = hovering
        }
    }

    private var backgroundColor: Color {
        switch variant {
        case .primary:
            return isHovered && !isDisabled ? Theme.accent.opacity(0.9) : Theme.accent
        case .secondary:
            return isHovered && !isDisabled ? Theme.surface2 : Theme.surface1
        case .ghost:
            return isHovered && !isDisabled ? Theme.surface2 : .clear
        }
    }

    private var borderColor: Color {
        variant == .secondary ? Theme.border : .clear
    }

    private var textColor: Color {
        switch variant {
        case .primary:
            return Theme.accentText
        case .secondary:
            return Theme.textPrimary
        case .ghost:
            return isHovered ? Theme.textPrimary : Theme.textSecondary
        }
    }

    private var iconColor: Color {
        switch variant {
        case .primary:
            return Theme.accentText
        case .secondary, .ghost:
            return isHovered ? Theme.textPrimary : Theme.textSecondary
        }
    }
}

struct RestyleIconButton: View {
    let icon: String
    let tooltip: String
    var isDisabled: Bool = false
    let action: () -> Void

    @State private var isHovered: Bool = false

    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: Theme.iconSize - 2, weight: .regular))
                .foregroundStyle(isHovered && !isDisabled ? Theme.textPrimary : Theme.textSecondary)
                .frame(width: Theme.rowCompact, height: Theme.rowCompact)
                .background(
                    (isHovered && !isDisabled) ? Theme.surface2 : Color.clear,
                    in: RoundedRectangle(cornerRadius: Theme.radiusControl)
                )
                .contentShape(RoundedRectangle(cornerRadius: Theme.radiusControl))
        }
        .buttonStyle(.plain)
        .disabled(isDisabled)
        .opacity(isDisabled ? 0.35 : 1.0)
        .help(tooltip)
        .accessibilityLabel(tooltip)
        .onHover { hovering in
            isHovered = hovering
        }
    }
}
