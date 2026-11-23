// SPDX-License-Identifier: GPL-3.0-only
import Foundation
import CoreGraphics

struct DeviceWindowLayoutSpec {
    let aspect: CGFloat
    let side: CGFloat
    let decorations: CGFloat
    let minimumHeight: CGFloat
}

enum DeviceWindowLayout {
    /// Fits all windows into one display's visible frame, preserving each
    /// player's content aspect and fixed sidebar/titlebar dimensions.
    static func frames(specs: [DeviceWindowLayoutSpec], visible: CGRect, gap: CGFloat = 12) -> [CGRect]? {
        guard !specs.isEmpty, visible.width > 0, visible.height > 0 else { return nil }
        var best: (score: CGFloat, frames: [CGRect])?
        for columns in 1...specs.count {
            let rows = (specs.count + columns - 1) / columns
            let width = (visible.width - CGFloat(columns - 1) * gap) / CGFloat(columns)
            let height = (visible.height - CGFloat(rows - 1) * gap) / CGFloat(rows)
            guard width > 0, height > 0 else { continue }
            var frames: [CGRect] = []
            var score = CGFloat.greatestFiniteMagnitude
            for (index, spec) in specs.enumerated() {
                guard spec.aspect > 0 else { frames.removeAll(); break }
                let contentHeight = floor(min(height - spec.decorations, (width - spec.side) / spec.aspect))
                guard contentHeight >= spec.minimumHeight else { frames.removeAll(); break }
                let size = CGSize(width: round(contentHeight * spec.aspect) + spec.side,
                                  height: contentHeight + spec.decorations)
                let col = index % columns, row = index / columns
                let x = visible.minX + CGFloat(col) * (width + gap) + (width - size.width) / 2
                let y = visible.maxY - CGFloat(row) * (height + gap) - size.height
                frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
                score = min(score, contentHeight)
            }
            if frames.count == specs.count && score > (best?.score ?? -.infinity) { best = (score, frames) }
        }
        return best?.frames
    }
}
