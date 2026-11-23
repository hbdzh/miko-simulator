// SPDX-License-Identifier: GPL-3.0-only
// Shared visual components for the SwiftUI centre.
//
// This is the product-level layer on top of the D41 design tokens: card frames,
// form rows, status labels, thumbnails and button styles that the device center,
// settings window and sheets compose. It intentionally contains no page layout
// and no strings; callers pass localised text.
import AppKit
import SwiftUI

// MARK: - Actions

/// Shared pill treatment for text actions across the centre and its panels.
struct WbPillButtonStyle: ButtonStyle {
    var prominent = false
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.controlSize) private var controlSize

    func makeBody(configuration: Configuration) -> some View {
        let compact = controlSize == .small || controlSize == .mini
        let foreground = prominent ? WbColor.raised
            : (configuration.role == .destructive ? WbColor.negative : WbColor.textPrimary)
        configuration.label
            .font(.system(size: compact ? 11 : 12, weight: .medium))
            .foregroundStyle(foreground)
            .padding(.horizontal, compact ? 10 : 14)
            .padding(.vertical, compact ? 4 : 6)
            .background(
                prominent ? AnyShapeStyle(WbColor.textPrimary) : AnyShapeStyle(Color.primary.opacity(0.035)),
                in: Capsule()
            )
            .overlay(
                Capsule()
                    .strokeBorder(prominent ? .clear : WbColor.borderDefault.opacity(0.8), lineWidth: 1)
            )
            .opacity(isEnabled ? (configuration.isPressed ? 0.8 : 1.0) : 0.45)
            .contentShape(Capsule())
    }
}

/// A shared macOS segmented control for mutually exclusive choices.
struct WbSegmentedControl<Value: Hashable>: View {
    struct Option {
        let value: Value
        let title: String
        var symbol: String? = nil
    }

    let label: String
    let selectedLabel: String
    @Binding var selection: Value
    let options: [Option]
    var iconOnly = false
    var segmentHeight: CGFloat = 28
    var fontSize: CGFloat = 12
    var inset: CGFloat = 3
    @Environment(\.isEnabled) private var isEnabled

    @ViewBuilder var body: some View {
        let shape = RoundedRectangle(cornerRadius: 10, style: .continuous)
        let segments = HStack(spacing: 0) {
            ForEach(options.indices, id: \.self) { index in
                let option = options[index]
                let selected = selection == option.value
                Button { selection = option.value } label: {
                    HStack(spacing: 4) {
                        if let symbol = option.symbol { Image(systemName: symbol) }
                        if !iconOnly { Text(option.title).lineLimit(1) }
                    }
                    .font(.system(size: fontSize, weight: .medium))
                    .foregroundStyle(selected ? Color(nsColor: .alternateSelectedControlTextColor) : WbColor.textPrimary)
                    .frame(maxWidth: .infinity, minHeight: segmentHeight)
                    .background(selected ? Color(nsColor: .controlAccentColor).opacity(0.88) : .clear,
                                in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(option.title)
                .accessibilityLabel(option.title)
                .accessibilityValue(selected ? selectedLabel : "")
                .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }
        .padding(inset)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(label)
        .onMoveCommand { direction in
            guard isEnabled, let index = options.firstIndex(where: { $0.value == selection }) else { return }
            switch direction {
            case .left: selection = options[max(0, index - 1)].value
            case .right: selection = options[min(options.count - 1, index + 1)].value
            default: break
            }
        }
        if #available(macOS 26.0, *) {
            segments.glassEffect(.regular, in: shape)
        } else {
            segments.background(.regularMaterial, in: shape)
        }
    }
}

// MARK: - Surfaces

/// Shared translucent card used throughout the app.
struct WbCardMaterial: ViewModifier {
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    var radius: CGFloat = 16

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        let dark = colorScheme == .dark
        content
            .background {
                shape.fill(reduceTransparency ? Color(nsColor: .windowBackgroundColor) : Color.white.opacity(dark ? 0.08 : 0.36))
                    .shadow(color: .black.opacity(dark ? 0.10 : 0.08), radius: 12, y: 5)
                    .allowsHitTesting(false)
            }
            .overlay(shape.strokeBorder(dark ? Color.white.opacity(0.16) : Color.black.opacity(0.12)).allowsHitTesting(false))
    }
}

extension View {
    /// Native, borderless menu picker; keeps standard keyboard and accessibility behavior.
    func wbDropdownStyle() -> some View {
        pickerStyle(.menu).buttonStyle(.borderless).controlSize(.regular)
    }
}

/// A raised card: the base material for panels and sheets.
struct WbCard<Content: View>: View {
    var padding: CGFloat = WbSpace.lg
    @ViewBuilder var content: () -> Content

    var body: some View {
        content()
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .modifier(WbCardMaterial())
    }
}

extension View {
    /// Card material without an extra layout wrapper, for views that already
    /// manage their own padding.
    func wbCardSurface(radius: CGFloat = 16) -> some View {
        modifier(WbCardMaterial(radius: radius))
    }

    func wbDivider() -> some View {
        overlay(alignment: .bottom) {
            Rectangle().fill(WbColor.borderSubtle).frame(height: 1)
        }
    }
}

struct WbDivider: View {
    var body: some View {
        Rectangle().fill(WbColor.borderSubtle).frame(height: 1)
    }
}

// MARK: - Headers and labels

/// Small uppercase-style label above a group of rows.
struct WbGroupLabel: View {
    var text: String
    var body: some View {
        Text(text)
            .wbFont(WbType.label)
            .foregroundStyle(WbColor.textSecondary)
            .padding(.leading, WbSpace.xxs)
    }
}

/// A card header: title on the left, an optional trailing control on the right.
struct WbCardHeader<Trailing: View>: View {
    var title: String
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        HStack(spacing: WbSpace.sm) {
            Text(title)
                .wbFont(WbType.title)
                .foregroundStyle(WbColor.textPrimary)
            Spacer(minLength: WbSpace.sm)
            trailing()
        }
    }
}

extension WbCardHeader where Trailing == EmptyView {
    init(title: String) {
        self.init(title: title, trailing: { EmptyView() })
    }
}

// MARK: - Status

enum WbStatusTone {
    case running, starting, stopped, error, busy, neutral

    var color: Color {
        switch self {
        case .running: return WbColor.positive
        case .starting: return WbColor.warning
        case .busy: return WbColor.warning
        case .error: return WbColor.negative
        case .stopped, .neutral: return WbColor.textTertiary
        }
    }
}

/// Status is a small dot plus a label, never a saturated pill.
struct WbStatusLabel: View {
    var text: String
    var tone: WbStatusTone

    var body: some View {
        HStack(spacing: WbSpace.xxs + 2) {
            Circle()
                .fill(tone.color)
                .frame(width: 6, height: 6)
            Text(text)
                .wbFont(WbType.label)
                .foregroundStyle(tone == .stopped || tone == .neutral ? WbColor.textSecondary : tone.color)
        }
        .accessibilityElement(children: .combine)
    }
}

// MARK: - Cached images

/// One cached image, or a remembered miss so a missing file is not retried
/// from every view body evaluation.
private final class WbCachedImage: NSObject {
    let image: NSImage?

    init(_ image: NSImage?) {
        self.image = image
    }
}

/// Bounded, thread-safe cache for the images the device views load from disk.
///
/// `WbAppIcon` and `DeviceArt` used to read their files from inside `body`,
/// repeating the IO whenever SwiftUI re-evaluates a row (device events, status
/// ticks, selection changes). `NSCache` is thread-safe and evicts under memory
/// pressure; `countLimit` keeps the cache small even when the SDK lists many
/// installed apps.
enum WbImageCache {
    private static let images: NSCache<NSString, WbCachedImage> = {
        let cache = NSCache<NSString, WbCachedImage>()
        cache.countLimit = 64
        return cache
    }()

    /// An image at an absolute file path, or `nil` when the file is missing or
    /// not a decodable image. Misses are cached too.
    static func image(atPath path: String) -> NSImage? {
        cachedImage(for: "file:\(path)") {
            NSImage(contentsOfFile: path)
        }
    }

    /// A PNG in the main bundle, by resource name without the extension.
    static func image(named name: String) -> NSImage? {
        cachedImage(for: "resource:\(name)") {
            guard let url = Bundle.main.url(forResource: name, withExtension: "png") else {
                return nil
            }
            return NSImage(contentsOf: url)
        }
    }

    private static func cachedImage(for key: String, load: () -> NSImage?) -> NSImage? {
        let cacheKey = key as NSString
        if let cached = images.object(forKey: cacheKey) {
            return cached.image
        }
        let image = load()
        images.setObject(WbCachedImage(image), forKey: cacheKey)
        return image
    }
}

// MARK: - Device thumbnail

/// AppKit-drawn device artwork inside a rounded bezel, matching the card system.
struct WbDeviceThumbnail: View {
    var name: String = "device-list"
    var width: CGFloat = 52
    var height: CGFloat = 96

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: WbRadius.sm + 2, style: .continuous)
                .fill(WbColor.bezel)
            DeviceArt(name: name, width: width)
        }
        .frame(width: width, height: height)
        .clipShape(RoundedRectangle(cornerRadius: WbRadius.sm + 2, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: WbRadius.sm + 2, style: .continuous)
                .strokeBorder(WbColor.borderSubtle, lineWidth: 1)
        )
    }
}

// MARK: - App tile

struct WbAppIcon: View {
    var symbol: String
    var imagePath: String? = nil
    var size: CGFloat = 28

    var body: some View {
        Group {
            if let imagePath, let image = WbImageCache.image(atPath: imagePath) {
                Image(nsImage: image)
                    .resizable()
                    .interpolation(.high)
            } else {
                ZStack {
                    RoundedRectangle(cornerRadius: size * 0.24, style: .continuous)
                        .fill(WbColor.control)
                    Image(systemName: symbol)
                        .font(.system(size: size * 0.46, weight: .medium))
                        .foregroundStyle(WbColor.textSecondary)
                }
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: size * 0.24, style: .continuous))
    }
}

// MARK: - Buttons

/// A compact flat action used in the "common actions" strip: icon + label with
/// a hover background and no permanent fill.
struct WbFlatActionLabel: View {
    var title: String
    var icon: String

    var body: some View {
        HStack(spacing: WbSpace.xs) {
            Image(systemName: icon)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(WbColor.accentText)
            Text(title)
                .wbFont(WbType.body)
                .foregroundStyle(WbColor.textPrimary)
        }
        .padding(.horizontal, WbSpace.sm)
        .frame(height: 28)
        .contentShape(RoundedRectangle(cornerRadius: WbRadius.sm, style: .continuous))
    }
}

// MARK: - Form rows

/// One settings row: label (+ optional detail) on the left, a control on the right.
struct WbFormRow<Control: View>: View {
    var label: String
    var detail: String? = nil
    @ViewBuilder var control: () -> Control

    var body: some View {
        HStack(alignment: .center, spacing: WbSpace.md) {
            VStack(alignment: .leading, spacing: 2) {
                Text(label)
                    .wbFont(WbType.body)
                    .foregroundStyle(WbColor.textPrimary)
                if let detail {
                    Text(detail)
                        .wbFont(WbType.caption)
                        .foregroundStyle(WbColor.textTertiary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
            Spacer(minLength: WbSpace.md)
            control()
        }
        .frame(minHeight: 44)
        .padding(.horizontal, WbSpace.lg)
    }
}

extension WbFormRow where Control == EmptyView {
    init(label: String, detail: String? = nil) {
        self.init(label: label, detail: detail, control: { EmptyView() })
    }
}

/// A grouped card of form rows with hairline separators between them.
struct WbFormGroup<Content: View>: View {
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(spacing: 0) {
            content()
        }
        .wbCardSurface()
    }
}

/// Row divider inset from the card edge, for grouped form rows.
struct WbRowDivider: View {
    var inset: CGFloat = WbSpace.lg
    var body: some View {
        Rectangle()
            .fill(WbColor.borderSubtle)
            .frame(height: 1)
            .padding(.leading, inset)
    }
}

// MARK: - Sidebar

/// One navigation row in the settings window.
struct WbSidebarRow: View {
    var title: String
    var icon: String
    var selected: Bool

    var body: some View {
        HStack(spacing: WbSpace.sm + 1) {
            Image(systemName: icon)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(selected ? WbColor.accentText : WbColor.textSecondary)
                .frame(width: 17)
            Text(title)
                .wbFont(selected ? WbType.body.weighted(.medium) : WbType.body)
                .foregroundStyle(selected ? WbColor.textPrimary : WbColor.textSecondary)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, WbSpace.sm)
        .frame(height: 31)
        .background(
            RoundedRectangle(cornerRadius: WbRadius.sm, style: .continuous)
                .fill(selected ? WbColor.accentContainer : .clear)
        )
        .overlay(
            RoundedRectangle(cornerRadius: WbRadius.sm, style: .continuous)
                .strokeBorder(selected ? WbColor.accentText.opacity(0.18) : .clear, lineWidth: 1)
        )
        .contentShape(RoundedRectangle(cornerRadius: WbRadius.sm, style: .continuous))
    }
}

// MARK: - Progress

struct WbProgressLine: View {
    var text: String
    var value: Double?

    var body: some View {
        HStack(alignment: .center, spacing: WbSpace.sm) {
            Text(text)
                .wbFont(WbType.caption)
                .monospacedDigit()
                .foregroundStyle(WbColor.textSecondary)
                .lineLimit(1)
            if let value {
                ProgressView(value: min(max(value, 0), 1))
                    .progressViewStyle(.linear)
                    .frame(minWidth: 120)
            }
        }
    }
}
