// SPDX-License-Identifier: GPL-3.0-only
import AppKit
import Combine
import CoreGraphics
import Foundation
import QuartzCore
import SwiftUI
import UniformTypeIdentifiers

final class ScreenView: NSView, NSTextInputClient {
    var transport: PlayerTransport?
    var image: CGImage? { didSet { needsDisplay = true } }
    private enum EditingDrag {
        case binding(key: String, rotation: Int, start: NSPoint, moved: Bool)
        case joystick(index: Int, start: NSPoint, moved: Bool)
    }
    private var rotation = 0
    private var mappedKeys: [UInt16: (id: Int, x: Double, y: Double)] = [:]
    private var mappedActions: [UInt16: MappingAction] = [:]
    private var mappingPhysicalKeys = Set<UInt16>()
    private var repeatCycles: [UInt16: MappingRepeatCycle] = [:]
    private var mappingTimers: [UInt16: DispatchWorkItem] = [:]
    private var mappingTokens: [UInt16: UUID] = [:]
    private var activeJoysticks: [Int: (id: Int, x: Double, y: Double, held: Set<JoystickDirection>)] = [:]
    private var activeJoystickKeys: [UInt16: (index: Int, direction: JoystickDirection)] = [:]
    private var editingDrag: EditingDrag?
    private var traceRecording: (target: MappingTraceTarget, started: TimeInterval, points: [MappingTracePoint])?
    private var mouseAim: (key: UInt16, binding: MappingBinding, state: MappingAimState)?
    private var gamepadAim: (key: UInt16, binding: MappingBinding, state: MappingAimState)?
    private var gamepadAimTimer: DispatchWorkItem?
    private var gamepadAxes: [String: Double] = [:]
    private var gamepadPhysicalDown = Set<String>()
    private var gamepadJoystick: (id: Int, x: Double, y: Double)?
    private var consumedCancelKeys = Set<UInt16>()
    private var priorMouseMovedEvents = false
    func present(_ frame: CGImage, rotation next: Int) {
        if rotation != next { releaseMappedKeys(); rotation = next; transport?.mapping.rotationChanged(to: next) }
        image = frame
    }
    private var marked = NSMutableAttributedString(string: "")
    private var markedSelection = NSRange(location: 0, length: 0)
    private var dragging = false
    private var lastTouch: (CGFloat, CGFloat)?
    private var gesture: PlayerGesture = .single
    private var secondFingerDown = false
    private var secondFinger: (x: Double, y: Double)?
    private var pendingKey: (UInt16, String)?
    private var pressedKeys: [UInt16: String] = [:]
    private var pressedModifiers: Set<String> = []
    private var scriptPlaybackBlocked = false
    private var syncFollowerLocked = false
    private var lastSyncRole: PlayerSyncRole = .off
    private var windowObserver: NSObjectProtocol?
    override var acceptsFirstResponder: Bool { true }
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        registerForDraggedTypes([.fileURL])
    }
    required init?(coder: NSCoder) { super.init(coder: coder); registerForDraggedTypes([.fileURL]) }
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard sender.draggingPasteboard.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) else { return [] }
        transport?.setDropHover(draggedURL(sender))
        return .copy
    }
    override func draggingExited(_ sender: NSDraggingInfo?) { transport?.setDropHover(nil) }
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        guard let url = draggedURL(sender), sender.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])?.count == 1 else { return false }
        transport?.setDropHover(nil)
        transport?.sendFile(url)
        return true
    }
    private func draggedURL(_ sender: NSDraggingInfo) -> URL? {
        guard let url = (sender.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL])?.first,
              url.isFileURL else { return nil }
        return url
    }
    override var isFlipped: Bool { true }
    override func viewDidMoveToWindow() {
        if window == nil { releaseMouseAim() }
        super.viewDidMoveToWindow()
        if let windowObserver { NotificationCenter.default.removeObserver(windowObserver) }
        if let window {
            windowObserver = NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: window, queue: .main) { [weak self] _ in
                guard let self else { return }
                if self.transport?.scriptPlaybackActive == true { self.transport?.scriptPlayStop() }
                self.releaseHeldInputs()
            }
        } else {
            windowObserver = nil
        }
    }
    deinit {
        releaseMouseAim()
        if let windowObserver { NotificationCenter.default.removeObserver(windowObserver) }
    }

    private var imageRect: NSRect {
        guard let image, image.width > 0, image.height > 0, bounds.width > 0, bounds.height > 0 else { return .zero }
        let imageAspect = CGFloat(image.width) / CGFloat(image.height)
        let boundsAspect = bounds.width / bounds.height
        if abs(boundsAspect - imageAspect) / imageAspect < 0.01 {
            return bounds
        }
        let scale = min(bounds.width / CGFloat(image.width), bounds.height / CGFloat(image.height))
        let size = NSSize(width: CGFloat(image.width) * scale, height: CGFloat(image.height) * scale)
        return NSRect(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2, width: size.width, height: size.height)
    }
    override func draw(_ dirtyRect: NSRect) {
        // AppKit can request drawing outside a view's bounds. Keep the video
        // background from painting over the shared titlebar or side rail.
        NSGraphicsContext.saveGraphicsState()
        bounds.clip()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSColor.black.setFill()
        dirtyRect.fill()
        if let image {
            let backing = window?.backingScaleFactor ?? 1
            let exact = abs(imageRect.width * backing - CGFloat(image.width)) < 0.5 && abs(imageRect.height * backing - CGFloat(image.height)) < 0.5
            NSGraphicsContext.current?.imageInterpolation = exact ? .none : .high
            NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height)).draw(in: imageRect)
            transport?.onFrameDrawn?()
        }
        if let mapping = transport?.mapping {
            let markerFont = WbFontResolver.nsFont(family: WbType.label.family, weight: .semibold, size: WbType.label.size)
            let joystickFont = WbFontResolver.nsFont(family: WbType.caption.family, weight: .semibold, size: WbType.caption.size)
            let markerAlpha = CGFloat(mapping.editing ? max(0.75, mapping.markerOpacity) : mapping.markerOpacity)
            let labelAlpha = min(1.0, markerAlpha + 0.15)
            let rect = imageRect
            let shortSide = min(rect.width, rect.height)
            for (index, joystick) in mapping.visibleJoysticks.enumerated() where joystick.rotation == rotation {
                let c = NSPoint(x: rect.minX + rect.width * joystick.x, y: rect.minY + rect.height * joystick.y)
                let r = max(20, shortSide * joystick.radius)
                let circle = NSRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2)
                NSColor(white: 0.14, alpha: markerAlpha * 0.56).setFill()
                let ringPath = NSBezierPath(ovalIn: circle)
                ringPath.fill()
                ringPath.lineWidth = 1.5
                NSColor.white.withAlphaComponent(markerAlpha * 0.78).setStroke()
                ringPath.stroke()
                if mapping.editing && mapping.selectedJoystickIndex == index {
                    let selectedRing = NSBezierPath(ovalIn: circle.insetBy(dx: -2, dy: -2))
                    selectedRing.lineWidth = 2.5
                    NSColor(WbColor.accent).setStroke()
                    selectedRing.stroke()
                }
                let centerDot = NSRect(x: c.x - 4, y: c.y - 4, width: 8, height: 8)
                NSColor.white.withAlphaComponent(min(1.0, markerAlpha * 0.9)).setFill()
                NSBezierPath(ovalIn: centerDot).fill()
                let offset = r * 0.62
                let labels: [(String, NSPoint)] = [
                    (joystick.up, NSPoint(x: c.x, y: c.y - offset)),
                    (joystick.down, NSPoint(x: c.x, y: c.y + offset)),
                    (joystick.left, NSPoint(x: c.x - offset, y: c.y)),
                    (joystick.right, NSPoint(x: c.x + offset, y: c.y)),
                ]
                let attrs: [NSAttributedString.Key: Any] = [.font: joystickFont, .foregroundColor: NSColor.white.withAlphaComponent(labelAlpha)]
                for (rawKey, pos) in labels {
                    let label = MappingKey.displayLabel(rawKey)
                    let size = label.size(withAttributes: attrs)
                    label.draw(at: NSPoint(x: pos.x - size.width / 2, y: pos.y - size.height / 2), withAttributes: attrs)
                }
            }
            for binding in mapping.visibleBindings where binding.rotation == rotation {
                let p = NSPoint(x: rect.minX + rect.width * binding.x, y: rect.minY + rect.height * binding.y)
                let circle = NSRect(x: p.x - 17, y: p.y - 17, width: 34, height: 34)
                NSColor(white: 0.14, alpha: markerAlpha * 0.78).setFill()
                let oval = NSBezierPath(ovalIn: circle)
                oval.fill()
                oval.lineWidth = 1.5
                NSColor.white.withAlphaComponent(markerAlpha * 0.85).setStroke()
                oval.stroke()
                if mapping.editing && mapping.selectedKey == binding.key {
                    let ring = NSBezierPath(ovalIn: circle.insetBy(dx: -2, dy: -2))
                    ring.lineWidth = 2.5
                    NSColor(WbColor.accent).setStroke()
                    ring.stroke()
                }
                let label = MappingKey.displayLabel(binding.key)
                let attrs: [NSAttributedString.Key: Any] = [.font: markerFont, .foregroundColor: NSColor.white.withAlphaComponent(labelAlpha)]
                let size = label.size(withAttributes: attrs)
                label.draw(at: NSPoint(x: p.x - size.width / 2, y: p.y - size.height / 2), withAttributes: attrs)
            }
            if mapping.editing, let pending = mapping.pendingPoint, pending.rotation == rotation {
                let p = NSPoint(x: rect.minX + rect.width * pending.x, y: rect.minY + rect.height * pending.y)
                let circle = NSRect(x: p.x - 17, y: p.y - 17, width: 34, height: 34)
                NSColor(WbColor.accent).withAlphaComponent(0.28).setFill()
                let path = NSBezierPath(ovalIn: circle)
                path.fill()
                path.lineWidth = 2
                let dashes: [CGFloat] = [4, 3]
                path.setLineDash(dashes, count: dashes.count, phase: 0)
                NSColor(WbColor.accent).setStroke()
                path.stroke()
                let attrs: [NSAttributedString.Key: Any] = [.font: markerFont, .foregroundColor: NSColor(WbColor.textOnAccent)]
                let label = "?"
                let size = label.size(withAttributes: attrs)
                label.draw(at: NSPoint(x: p.x - size.width / 2, y: p.y - size.height / 2), withAttributes: attrs)
            }
        }
        if marked.length > 0 {
            let box = NSRect(x: 12, y: max(12, bounds.height - 46), width: max(0, bounds.width - 24), height: 34)
            NSColor(WbColor.chrome).withAlphaComponent(0.95).setFill()
            NSBezierPath(roundedRect: box, xRadius: 6, yRadius: 6).fill()
            marked.draw(at: NSPoint(x: box.minX + 8, y: box.minY + 6))
        }
    }
    private func normalized(_ event: NSEvent) -> (CGFloat, CGFloat)? {
        let rect = imageRect
        guard rect.width > 0, rect.height > 0 else { return nil }
        let point = convert(event.locationInWindow, from: nil)
        return (min(1, max(0, (point.x - rect.minX) / rect.width)),
                min(1, max(0, (point.y - rect.minY) / rect.height)))
    }
    private func sendTouch(_ event: NSEvent, down: Bool) {
        let rect = imageRect, point = convert(event.locationInWindow, from: nil)
        guard rect.width > 0, rect.height > 0 else { return }
        if !dragging && !rect.contains(point) { return }
        let x = min(1, max(0, (point.x - rect.minX) / rect.width))
        let y = min(1, max(0, (point.y - rect.minY) / rect.height))
        lastTouch = (x, y)
        transport?.noteInput()
        transport?.send(["type": "touch", "x": x, "y": y, "down": down])
    }
    /// ⌥ drag = pinch, ⇧⌥ drag = two-finger pan. The second finger travels as a
    /// mapped touch (identifier 3) and goes through the same rotation mapping.
    private func secondFingerPoint(_ event: NSEvent) -> (x: Double, y: Double)? {
        guard let image, let (x, y) = normalized(event) else { return nil }
        return PlayerTouch.secondFinger(gesture, x: Double(x), y: Double(y),
                                        shownWidth: Double(image.width), shownHeight: Double(image.height))
    }
    private func sendSecondFinger(_ point: (x: Double, y: Double), down: Bool) {
        transport?.noteInput()
        transport?.send(["type": "mapped_touch", "id": PlayerTouch.secondFingerID,
                         "x": point.x, "y": point.y, "down": down])
    }
    private func clampedNormalizedPoint(_ point: NSPoint) -> (Double, Double)? {
        let rect = imageRect
        guard rect.width > 0, rect.height > 0 else { return nil }
        return (Double(min(1, max(0, (point.x - rect.minX) / rect.width))),
                Double(min(1, max(0, (point.y - rect.minY) / rect.height))))
    }
    private func bindingHit(at point: NSPoint) -> MappingBinding? {
        let rect = imageRect
        guard rect.width > 0, rect.height > 0, let mapping = transport?.mapping else { return nil }
        return mapping.visibleBindings.reversed().first { binding in
            guard binding.rotation == rotation else { return false }
            let center = NSPoint(x: rect.minX + rect.width * binding.x, y: rect.minY + rect.height * binding.y)
            return hypot(point.x - center.x, point.y - center.y) <= 19
        }
    }
    private func joystickHit(at point: NSPoint) -> Int? {
        let rect = imageRect
        let shortSide = min(rect.width, rect.height)
        guard rect.width > 0, rect.height > 0, let mapping = transport?.mapping else { return nil }
        for (index, joystick) in mapping.visibleJoysticks.enumerated().reversed() where joystick.rotation == rotation {
            let center = NSPoint(x: rect.minX + rect.width * joystick.x, y: rect.minY + rect.height * joystick.y)
            let r = max(20, shortSide * joystick.radius)
            if hypot(point.x - center.x, point.y - center.y) <= r + 6 {
                return index
            }
        }
        return nil
    }
    private func nextTouchID() -> Int? {
        var used = Set(mappedKeys.values.map(\.id)).union(activeJoysticks.values.map(\.id))
        if let gamepadJoystick { used.insert(gamepadJoystick.id) }
        return (3...9).first { !used.contains($0) }
    }
    private var imageAspect: Double {
        if let image, image.height > 0 { return Double(image.width) / Double(image.height) }
        let rect = imageRect
        return rect.height > 0 ? Double(rect.width / rect.height) : 1.0
    }
    override func mouseDown(with event: NSEvent) {
        if syncFollowerLocked {
            transport?.showSyncLockedHint()
            return
        }
        window?.makeFirstResponder(self)
        let point = convert(event.locationInWindow, from: nil)
        guard imageRect.contains(point) else { return }
        if let mapping = transport?.mapping, mapping.editing, let target = mapping.traceTarget,
           target.rotation == rotation, let (x, y) = clampedNormalizedPoint(point) {
            traceRecording = (target, ProcessInfo.processInfo.systemUptime, [MappingTracePoint(x: x, y: y, tMs: 0)])
            return
        }
        if let mapping = transport?.mapping, mapping.editing, mapping.placementMode != .none {
            let lang = transport?.language ?? 1
            if let hit = bindingHit(at: point), mapping.select(key: hit.key, rotation: rotation) {
                editingDrag = .binding(key: hit.key, rotation: rotation, start: point, moved: false)
            } else if let idx = joystickHit(at: point), mapping.selectJoystick(at: idx) {
                editingDrag = .joystick(index: idx, start: point, moved: false)
            } else if let (nx, ny) = clampedNormalizedPoint(point) {
                editingDrag = nil
                if mapping.placementMode == .joystick {
                    if mapping.addJoystick(x: nx, y: ny, rotation: rotation, preset: .wasd)
                        || mapping.addJoystick(x: nx, y: ny, rotation: rotation, preset: .arrows) {
                        mapping.placementMode = .tap
                    } else {
                        transport?.onStatus?(t("WASD 和方向键均已被占用", "WASD and arrow keys are already bound", lang))
                    }
                } else {
                    mapping.place(x: nx, y: ny, rotation: rotation)
                }
            }
            return
        }
        transport?.takeOverGamepadMapping()
        if mouseAim?.binding.action != .fpsView { releaseMappedKeys() }
        gesture = mouseAim?.binding.action == .fpsView ? .single : PlayerGesture.from(
            option: event.modifierFlags.contains(.option), shift: event.modifierFlags.contains(.shift))
        dragging = true
        if gesture.needsSecondFinger, let second = secondFingerPoint(event) {
            secondFinger = second
            secondFingerDown = true
            sendSecondFinger(second, down: true)
        }
        sendTouch(event, down: true)
    }
    override func mouseDragged(with event: NSEvent) {
        if mouseAim != nil { mouseMoved(with: event); return }
        if traceRecording != nil {
            appendTracePoint(event, final: false)
            return
        }
        if let mapping = transport?.mapping, mapping.editing, let drag = editingDrag {
            let point = convert(event.locationInWindow, from: nil)
            switch drag {
            case .binding(let key, let rot, let start, var moved):
                if moved || hypot(point.x - start.x, point.y - start.y) >= 3 {
                    moved = true
                    editingDrag = .binding(key: key, rotation: rot, start: start, moved: moved)
                    if let (nx, ny) = clampedNormalizedPoint(point) {
                        _ = mapping.move(key: key, rotation: rot, x: nx, y: ny)
                    }
                }
            case .joystick(let idx, let start, var moved):
                if moved || hypot(point.x - start.x, point.y - start.y) >= 3 {
                    moved = true
                    editingDrag = .joystick(index: idx, start: start, moved: moved)
                    if let (nx, ny) = clampedNormalizedPoint(point) {
                        _ = mapping.moveJoystick(at: idx, x: nx, y: ny)
                    }
                }
            }
            return
        }
        guard dragging, !inputBlocked else { return }
        if secondFingerDown, let second = secondFingerPoint(event) {
            secondFinger = second
            sendSecondFinger(second, down: true)
        }
        sendTouch(event, down: true)
    }
    override func mouseUp(with event: NSEvent) {
        if traceRecording != nil {
            appendTracePoint(event, final: true)
            if let recording = traceRecording, let mapping = transport?.mapping, mapping.traceTarget == recording.target {
                if !mapping.setTrace(key: recording.target.key, rotation: recording.target.rotation, points: recording.points) {
                    transport?.onStatus?(t("轨迹无效，请重新录制。", "Invalid path. Record it again.", transport?.language ?? 1))
                }
                mapping.cancelTrace()
            }
            traceRecording = nil
            return
        }
        if let mapping = transport?.mapping, mapping.editing, editingDrag != nil {
            editingDrag = nil
            return
        }
        if inputBlocked { clearHeldInputState(); return }
        guard dragging else { return }
        if secondFingerDown, let second = secondFinger {
            sendSecondFinger(second, down: false)
        }
        sendTouch(event, down: false)
        dragging = false
        secondFingerDown = false
        secondFinger = nil
        gesture = .single
    }
    override func rightMouseDown(with event: NSEvent) {
        if cancelMouseSkill() || cancelGamepadSkill() { return }
        guard let mapping = transport?.mapping, mapping.editing, mapping.placementMode != .none else {
            super.rightMouseDown(with: event)
            return
        }
        window?.makeFirstResponder(self)
        let point = convert(event.locationInWindow, from: nil)
        let lang = transport?.language ?? 1
        if let hit = bindingHit(at: point) {
            if mapping.remove(key: hit.key, rotation: rotation) {
                transport?.onStatus?(t("已删除键位 ", "Removed ", lang) + MappingKey.displayLabel(hit.key))
            }
            return
        }
        if let idx = joystickHit(at: point), mapping.removeJoystick(at: idx) {
            transport?.onStatus?(t("已删除摇杆", "Removed joystick", lang))
        }
    }
    private func releaseMappedKeys() {
        _ = cancelMouseSkill()
        _ = cancelGamepadSkill()
        for key in Array(mappedKeys.keys) { stopMappedControl(key) }
        mappingPhysicalKeys.removeAll()
        for state in activeJoysticks.values { transport?.send(["type": "mapped_touch", "id": state.id, "x": state.x, "y": state.y, "down": false]) }
        activeJoysticks.removeAll()
        activeJoystickKeys.removeAll()
        if let state = gamepadJoystick {
            transport?.send(["type": "mapped_touch", "id": state.id, "x": state.x, "y": state.y, "down": false])
        }
        gamepadJoystick = nil
        gamepadAxes.removeAll()
        gamepadAimTimer?.cancel(); gamepadAimTimer = nil; gamepadAim = nil
        releaseMouseAim()
    }

    @discardableResult private func cancelGamepadSkill() -> Bool {
        guard let aim = gamepadAim, aim.binding.action == .skillAim, let value = mappedKeys[aim.key] else { return false }
        mappedKeys[aim.key] = (value.id, aim.binding.endX, aim.binding.endY)
        transport?.send(["type": "mapped_touch", "id": value.id, "x": aim.binding.endX, "y": aim.binding.endY, "down": true])
        stopMappedControl(aim.key)
        return true
    }

    private let gamepadButtons = ["South", "East", "West", "North", "LeftBumper", "RightBumper", "Back", "Start", "Guide", "LeftStick", "RightStick", "Up", "Down", "Left", "Right", "LeftTrigger", "RightTrigger"]

    fileprivate func resetGamepadPhysicalState() { gamepadPhysicalDown.removeAll() }

    fileprivate func receiveGamepadEvent(_ event: [String: Any]) {
        guard !inputBlocked, let kind = event["kind"] as? String, let control = event["control"] as? String else { return }
        if scriptPlaybackBlocked {
            let down: Bool?
            if kind == "button" { down = event["down"] as? Bool }
            else if kind == "axis", control == "LeftTrigger" || control == "RightTrigger",
                    let value = event["value"] as? Double, value.isFinite { down = value > 0.01 }
            else { down = nil }
            if let down {
                if down { gamepadPhysicalDown.insert(control) }
                else { gamepadPhysicalDown.remove(control) }
            }
            return
        }
        if kind == "button", let down = event["down"] as? Bool {
            gamepadButton(control, down: down)
        } else if kind == "axis", let value = event["value"] as? Double, value.isFinite {
            let value = min(1, max(-1, value))
            gamepadAxes[control] = value
            if control == "LeftTrigger" || control == "RightTrigger" {
                gamepadButton(control, down: value > 0.01)
            } else if control == "LeftX" || control == "LeftY" {
                updateGamepadJoystick()
            } else if (control == "RightX" || control == "RightY"), gamepadAim != nil,
                      gamepadAimTimer == nil, value != 0 {
                scheduleGamepadAim()
            }
        }
    }

    private func gamepadButton(_ control: String, down: Bool) {
        guard let index = gamepadButtons.firstIndex(of: control), let mapping = transport?.mapping else { return }
        if down {
            guard gamepadPhysicalDown.insert(control).inserted else { return }
        } else {
            guard gamepadPhysicalDown.remove(control) != nil else { return }
        }
        let code = UInt16(1000 + index)
        let key = "PAD_" + control.uppercased()
        if mapping.editing {
            if down && (mapping.pending || mapping.selectedKey != nil), mapping.assign(key) {
                transport?.onStatus?(t("已绑定 ", "Bound ", transport?.language ?? 1) + control)
            }
            return
        }
        guard mapping.enabled else { return }
        if down {
            if consumedCancelKeys.contains(code) { return }
            if let aim = gamepadAim, aim.binding.cancelsSkill(with: key) {
                consumedCancelKeys.insert(code)
                _ = cancelGamepadSkill()
                return
            }
            guard let binding = mapping.scheme.bindings.first(where: { $0.key == key && $0.rotation == rotation }) else { return }
            startMapping(binding, code: code, isRepeat: false, gamepad: true)
        } else {
            if consumedCancelKeys.remove(code) != nil { return }
            mappingPhysicalKeys.remove(code)
            if let action = mappedActions[code], action.stopsOnKeyUp { stopMappedControl(code) }
        }
    }

    private func updateGamepadJoystick() {
        guard let mapping = transport?.mapping, mapping.enabled, !mapping.editing,
              let joystick = mapping.scheme.joysticks.first(where: { $0.rotation == rotation }) else { return }
        let x = gamepadAxes["LeftX"] ?? 0, y = gamepadAxes["LeftY"] ?? 0
        if x == 0 && y == 0 {
            if let state = gamepadJoystick { transport?.send(["type": "mapped_touch", "id": state.id, "x": state.x, "y": state.y, "down": false]) }
            gamepadJoystick = nil
            return
        }
        let target = joystick.analogTarget(x: x, y: y, aspectWidthOverHeight: imageAspect)
        if gamepadJoystick == nil {
            guard let id = nextTouchID() else { return }
            releaseManualInputs()
            gamepadJoystick = (id, joystick.x, joystick.y)
            transport?.send(["type": "mapped_touch", "id": id, "x": joystick.x, "y": joystick.y, "down": true])
        }
        guard let current = gamepadJoystick else { return }
        gamepadJoystick = (current.id, target.x, target.y)
        transport?.noteInput()
        transport?.send(["type": "mapped_touch", "id": current.id, "x": target.x, "y": target.y, "down": true])
    }

    private func scheduleGamepadAim() {
        gamepadAimTimer?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self, var aim = self.gamepadAim, !self.inputBlocked, let value = self.mappedKeys[aim.key] else { return }
            self.gamepadAimTimer = nil
            let rect = self.imageRect
            let x = self.gamepadAxes["RightX"] ?? 0, y = self.gamepadAxes["RightY"] ?? 0
            if (x != 0 || y != 0) && rect.width > 0 && rect.height > 0 {
                let speed = min(rect.width, rect.height) * 0.012
                aim.state.move(dx: x * speed, dy: -y * speed, width: rect.width, height: rect.height)
                self.mappedKeys[aim.key] = (value.id, aim.state.x, aim.state.y)
                self.transport?.send(["type": "mapped_touch", "id": value.id, "x": aim.state.x, "y": aim.state.y, "down": true])
                if aim.binding.action == .fpsView && aim.state.atLimit(width: rect.width, height: rect.height) {
                    self.transport?.send(["type": "mapped_touch", "id": value.id, "x": aim.state.x, "y": aim.state.y, "down": false])
                    aim.state = MappingAimState(binding: aim.binding)
                    self.mappedKeys[aim.key] = (value.id, aim.state.x, aim.state.y)
                    self.transport?.send(["type": "mapped_touch", "id": value.id, "x": aim.state.x, "y": aim.state.y, "down": true])
                }
            }
            self.gamepadAim = aim
            if x != 0 || y != 0 { self.scheduleGamepadAim() }
        }
        gamepadAimTimer = work
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(16), execute: work)
    }

    @discardableResult private func cancelMouseSkill() -> Bool {
        guard let aim = mouseAim, aim.binding.action == .skillAim, let value = mappedKeys[aim.key] else { return false }
        mappedKeys[aim.key] = (value.id, aim.binding.endX, aim.binding.endY)
        transport?.send(["type": "mapped_touch", "id": value.id, "x": aim.binding.endX, "y": aim.binding.endY, "down": true])
        stopMappedControl(aim.key)
        return true
    }

    private func beginMouseAim(key: UInt16, binding: MappingBinding) -> Bool {
        guard let window else { return false }
        if let current = mouseAim, !cancelMouseSkill() { stopMappedControl(current.key) }
        if let current = gamepadAim, !cancelGamepadSkill() { stopMappedControl(current.key) }
        guard CGAssociateMouseAndMouseCursorPosition(0) == .success else {
            transport?.onStatus?(t("无法捕获鼠标，请检查系统权限。", "Could not capture the mouse. Check system permissions.", transport?.language ?? 1))
            return false
        }
        priorMouseMovedEvents = window.acceptsMouseMovedEvents
        window.acceptsMouseMovedEvents = true
        NSCursor.hide()
        mouseAim = (key, binding, MappingAimState(binding: binding))
        return true
    }

    private func releaseMouseAim() {
        guard mouseAim != nil else { return }
        mouseAim = nil
        _ = CGAssociateMouseAndMouseCursorPosition(1)
        NSCursor.unhide()
        window?.acceptsMouseMovedEvents = priorMouseMovedEvents
    }

    override func mouseMoved(with event: NSEvent) {
        guard var aim = mouseAim, !inputBlocked, let value = mappedKeys[aim.key] else { return }
        let rect = imageRect
        aim.state.move(dx: event.deltaX, dy: event.deltaY, width: rect.width, height: rect.height)
        mappedKeys[aim.key] = (value.id, aim.state.x, aim.state.y)
        transport?.send(["type": "mapped_touch", "id": value.id, "x": aim.state.x, "y": aim.state.y, "down": true])
        if aim.binding.action == .fpsView && (aim.state.atLimit(width: rect.width, height: rect.height) || aim.state.x <= 0 || aim.state.x >= 1 || aim.state.y <= 0 || aim.state.y >= 1) {
            transport?.send(["type": "mapped_touch", "id": value.id, "x": aim.state.x, "y": aim.state.y, "down": false])
            aim.state = MappingAimState(binding: aim.binding)
            mappedKeys[aim.key] = (value.id, aim.state.x, aim.state.y)
            transport?.send(["type": "mapped_touch", "id": value.id, "x": aim.state.x, "y": aim.state.y, "down": true])
        }
        mouseAim = aim
    }
    private func appendTracePoint(_ event: NSEvent, final: Bool) {
        guard var recording = traceRecording,
              transport?.mapping.traceTarget == recording.target,
              let (x, y) = clampedNormalizedPoint(convert(event.locationInWindow, from: nil)) else { return }
        let elapsed = max(final ? 1 : 0, Int((ProcessInfo.processInfo.systemUptime - recording.started) * 1000))
        guard elapsed <= 10000, recording.points.count < 1024 else {
            traceRecording = nil
            transport?.mapping.cancelTrace()
            transport?.onStatus?(t("轨迹超过时间或采样上限，请录制较短的轨迹。", "Path exceeded the time or sample limit. Record a shorter path.", transport?.language ?? 1))
            return
        }
        let point = MappingTracePoint(x: x, y: y, tMs: elapsed)
        recording.points.append(point)
        traceRecording = recording
    }
    /// The bridge releases old input on playback start. Drop the corresponding
    /// local state so subsequent manual actions can take ownership cleanly.
    func setScriptPlaybackBlocked(_ blocked: Bool) {
        guard blocked != scriptPlaybackBlocked else { return }
        scriptPlaybackBlocked = blocked
        if blocked { clearHeldInputState() }
    }

    func setSyncState(role: PlayerSyncRole, locked: Bool) {
        let roleChanged = role != lastSyncRole
        let lockedNow = role == .follower && locked
        let becameLocked = lockedNow && !syncFollowerLocked
        lastSyncRole = role
        syncFollowerLocked = lockedNow
        if roleChanged || becameLocked {
            clearHeldInputState()
        }
    }

    private func clearHeldInputState() {
        releaseMouseAim()
        for (key, action) in mappedActions where action.holdsOwnershipBetweenTouches {
            transport?.send(["type": "mapping_hold", "id": Int(key), "active": false])
        }
        for timer in mappingTimers.values { timer.cancel() }
        mappingTimers.removeAll()
        mappingTokens.removeAll()
        mappedKeys.removeAll()
        mappedActions.removeAll()
        mappingPhysicalKeys.removeAll()
        consumedCancelKeys.removeAll()
        repeatCycles.removeAll()
        activeJoysticks.removeAll()
        activeJoystickKeys.removeAll()
        gamepadJoystick = nil
        gamepadAxes.removeAll()
        gamepadAimTimer?.cancel(); gamepadAimTimer = nil; gamepadAim = nil
        editingDrag = nil
        dragging = false
        secondFingerDown = false
        secondFinger = nil
        pendingKey = nil
        pressedKeys.removeAll()
        pressedModifiers.removeAll()
        marked = NSMutableAttributedString(string: "")
        markedSelection = NSRange(location: 0, length: 0)
        needsDisplay = true
    }

    private var inputBlocked: Bool {
        syncFollowerLocked
    }

    fileprivate func releaseHeldInputs() {
        if transport?.mapping.placementMode == MappingPlacementMode.none { editingDrag = nil }
        consumedCancelKeys.removeAll()
        traceRecording = nil
        transport?.mapping.cancelTrace()
        releaseMappedKeys()
        releaseManualInputs()
    }
    private func releaseManualInputs() {
        if secondFingerDown, let secondFinger { sendSecondFinger(secondFinger, down: false) }
        secondFingerDown = false
        secondFinger = nil
        if dragging, let lastTouch {
            transport?.send(["type": "touch", "x": lastTouch.0, "y": lastTouch.1, "down": false])
            dragging = false
        }
        for key in pressedKeys.values { transport?.send(["type": "key", "key": key, "down": false]) }
        for key in pressedModifiers { transport?.send(["type": "key", "key": key, "down": false]) }
        pressedKeys.removeAll()
        pressedModifiers.removeAll()
    }
    override func resignFirstResponder() -> Bool {
        releaseHeldInputs()
        return super.resignFirstResponder()
    }
    override func flagsChanged(with event: NSEvent) {
        guard !inputBlocked else { return }
        if transport?.mapping.editing != true { transport?.takeOverGamepadMapping() }
        if mouseAim?.binding.action != .fpsView { releaseMappedKeys() }
        for (flag, key) in [(NSEvent.ModifierFlags.shift, "Shift"), (.control, "Control"), (.option, "Alt")] {
            let down = event.modifierFlags.contains(flag)
            if down && !pressedModifiers.contains(key) {
                pressedModifiers.insert(key)
                transport?.send(["type": "key", "key": key, "down": true])
            } else if !down && pressedModifiers.remove(key) != nil {
                transport?.send(["type": "key", "key": key, "down": false])
            }
        }
    }
    func handleSkillCancelKeyDown(_ event: NSEvent) -> Bool {
        if consumedCancelKeys.contains(event.keyCode) { return true }
        guard !syncFollowerLocked, transport?.mapping.editing == false,
              !event.modifierFlags.contains(.command), !event.modifierFlags.contains(.control),
              !event.modifierFlags.contains(.option),
              let key = MappingKey.normalizeCancel(keyCode: event.keyCode, charactersIgnoringModifiers: event.charactersIgnoringModifiers) else { return false }
        let mouseMatches = mouseAim?.binding.cancelsSkill(with: key) == true
        let gamepadMatches = gamepadAim?.binding.cancelsSkill(with: key) == true
        guard mouseMatches || gamepadMatches else { return false }
        consumedCancelKeys.insert(event.keyCode)
        if mouseMatches { _ = cancelMouseSkill() } else { _ = cancelGamepadSkill() }
        return true
    }

    override func keyDown(with event: NSEvent) {
        if syncFollowerLocked {
            transport?.showSyncLockedHint()
            return
        }
        if handleSkillCancelKeyDown(event) { return }
        if let mapping = transport?.mapping, mapping.editing, mapping.placementMode != .none {
            let lang = transport?.language ?? 1
            if event.keyCode == 53 {
                mapping.clearSelection()
                return
            }
            if event.keyCode == 51 || event.keyCode == 117 {
                if let selected = mapping.selectedKey, mapping.remove(key: selected, rotation: rotation) {
                    transport?.onStatus?(t("已删除键位 ", "Removed ", lang) + MappingKey.displayLabel(selected))
                } else if let idx = mapping.selectedJoystickIndex, mapping.removeJoystick(at: idx) {
                    transport?.onStatus?(t("已删除摇杆", "Removed joystick", lang))
                }
                return
            }
            if !event.modifierFlags.contains(.command), !event.modifierFlags.contains(.control), !event.modifierFlags.contains(.option) {
                if mapping.pending || mapping.selectedKey != nil {
                    if let key = MappingKey.normalize(keyCode: event.keyCode, charactersIgnoringModifiers: event.charactersIgnoringModifiers),
                       mapping.assign(key) {
                        transport?.onStatus?(t("已绑定 ", "Bound ", lang) + MappingKey.displayLabel(key))
                    } else {
                        transport?.onStatus?(t("请使用字母、数字、方向键或常用按键", "Use a letter, digit, arrow, or standard key", lang))
                    }
                }
                return
            }
        }
        if let mapping = transport?.mapping, !event.modifierFlags.contains(.command), !event.modifierFlags.contains(.control), !event.modifierFlags.contains(.option) {
            if mappingPhysicalKeys.contains(event.keyCode) || activeJoystickKeys[event.keyCode] != nil { return }
            if mapping.enabled, !mapping.editing,
               let key = MappingKey.normalize(keyCode: event.keyCode, charactersIgnoringModifiers: event.charactersIgnoringModifiers) {
                if let binding = mapping.scheme.bindings.first(where: { $0.key == key && $0.rotation == rotation }) {
                    startMapping(binding, code: event.keyCode, isRepeat: event.isARepeat, gamepad: false)
                    return
                }
                for (index, joystick) in mapping.scheme.joysticks.enumerated() where joystick.rotation == rotation {
                    guard let direction = joystick.direction(for: key) else { continue }
                    releaseManualInputs()
                    handleJoystickKeyDown(keyCode: event.keyCode, index: index, joystick: joystick, direction: direction)
                    return
                }
            }
        }
        transport?.takeOverGamepadMapping()
        if mouseAim?.binding.action != .fpsView { releaseMappedKeys() }
        if event.isARepeat && pressedKeys[event.keyCode] != nil { return }
        if event.modifierFlags.contains(.command), let key = event.charactersIgnoringModifiers?.lowercased() {
            switch key {
            case "v": transport?.paste(); return
            case "c": transport?.copy(cut: false); return
            case "x": transport?.copy(cut: true); return
            case "a": transport?.send(["type": "select_all"]); return
            default: break
            }
        }
        let specials: [UInt16: String] = [36: "Enter", 48: "Tab", 51: "Backspace", 53: "Escape", 117: "Delete", 123: "ArrowLeft", 124: "ArrowRight", 125: "ArrowDown", 126: "ArrowUp"]
        if let key = specials[event.keyCode], !hasMarkedText() {
            pressedKeys[event.keyCode] = key
            transport?.noteInput()
            transport?.send(["type": "key", "key": key, "down": true])
            return
        }
        if !event.isARepeat, let value = event.charactersIgnoringModifiers, value.unicodeScalars.count == 1, let scalar = value.unicodeScalars.first, scalar.isASCII, !CharacterSet.controlCharacters.contains(scalar) {
            pendingKey = (event.keyCode, value.lowercased())
        }
        interpretKeyEvents([event])
        pendingKey = nil
    }

    private func startMapping(_ binding: MappingBinding, code: UInt16, isRepeat: Bool, gamepad: Bool) {
        if binding.action == .script {
            guard !isRepeat else { return }
            mappingPhysicalKeys.insert(code)
            releaseHeldInputs()
            transport?.scriptPlayDefault(id: binding.scriptID)
            return
        }
        let decision = MappingPressDecision.decide(action: binding.action, active: mappedKeys[code] != nil,
                                                    physicalDown: mappingPhysicalKeys.contains(code), isRepeat: isRepeat)
        guard decision != .ignore else { return }
        releaseManualInputs()
        if decision == .stop { mappingPhysicalKeys.insert(code); stopMappedControl(code); return }
        guard let id = nextTouchID() else { return }
        if binding.action == .fpsView || binding.action == .skillAim {
            if gamepad {
                if let existing = gamepadAim, !cancelGamepadSkill() { stopMappedControl(existing.key) }
                if let existing = mouseAim, !cancelMouseSkill() { stopMappedControl(existing.key) }
                gamepadAim = (code, binding, MappingAimState(binding: binding))
            } else if !beginMouseAim(key: code, binding: binding) { return }
        }
        mappingPhysicalKeys.insert(code)
        mappedKeys[code] = (id, binding.x, binding.y)
        mappedActions[code] = binding.action
        transport?.noteInput()
        if binding.action.holdsOwnershipBetweenTouches { transport?.send(["type": "mapping_hold", "id": Int(code), "active": true]) }
        transport?.send(["type": "mapped_touch", "id": id, "x": binding.x, "y": binding.y, "down": true])
        if binding.action == .repeatHold || binding.action == .repeatToggle {
            repeatCycles[code] = MappingRepeatCycle()
            let token = UUID(); mappingTokens[code] = token
            scheduleRepeat(code, interval: binding.repeatIntervalMs, token: token)
        } else if binding.action == .swipe || binding.action == .gesture {
            let token = UUID(); mappingTokens[code] = token
            scheduleSwipe(code, binding: binding, started: ProcessInfo.processInfo.systemUptime, token: token)
        }
        if gamepad && (binding.action == .fpsView || binding.action == .skillAim) { scheduleGamepadAim() }
    }
    private func scheduleRepeat(_ keyCode: UInt16, interval: Int, token: UUID) {
        let work = DispatchWorkItem { [weak self] in
            guard let self, let value = self.mappedKeys[keyCode],
                  self.mappingTokens[keyCode] == token,
                  let action = self.mappedActions[keyCode], action == .repeatHold || action == .repeatToggle else { return }
            guard var cycle = self.repeatCycles[keyCode] else { return }
            let down = cycle.advance()
            self.repeatCycles[keyCode] = cycle
            self.transport?.send(["type": "mapped_touch", "id": value.id, "x": value.x, "y": value.y, "down": down])
            self.scheduleRepeat(keyCode, interval: interval, token: token)
        }
        mappingTimers[keyCode] = work
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(max(20, interval / 2)), execute: work)
    }
    private func scheduleSwipe(_ keyCode: UInt16, binding: MappingBinding, started: TimeInterval, token: UUID) {
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.mappingTokens[keyCode] == token, let current = self.mappedKeys[keyCode] else { return }
            let elapsed = max(0, ProcessInfo.processInfo.systemUptime - started)
            let point = MappingPathStep(binding: binding, elapsedMs: elapsed * 1000, previousX: current.x, previousY: current.y)
            let x = point.x, y = point.y
            self.mappedKeys[keyCode] = (current.id, x, y)
            if point.sendMove {
                self.transport?.send(["type": "mapped_touch", "id": current.id, "x": x, "y": y, "down": true])
            }
            if point.finished {
                self.stopMappedControl(keyCode)
            } else {
                self.scheduleSwipe(keyCode, binding: binding, started: started, token: token)
            }
        }
        mappingTimers[keyCode] = work
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(16), execute: work)
    }
    private func stopMappedControl(_ keyCode: UInt16) {
        if mouseAim?.key == keyCode { releaseMouseAim() }
        if gamepadAim?.key == keyCode { gamepadAimTimer?.cancel(); gamepadAimTimer = nil; gamepadAim = nil }
        mappingTimers.removeValue(forKey: keyCode)?.cancel()
        mappingTokens.removeValue(forKey: keyCode)
        let action = mappedActions.removeValue(forKey: keyCode)
        var cycle = repeatCycles.removeValue(forKey: keyCode)
        let wasDown = cycle?.stopNeedsUp() ?? false
        guard let value = mappedKeys.removeValue(forKey: keyCode) else { return }
        if action != .repeatHold && action != .repeatToggle || wasDown {
            transport?.send(["type": "mapped_touch", "id": value.id, "x": value.x, "y": value.y, "down": false])
        }
        if action?.holdsOwnershipBetweenTouches == true {
            transport?.send(["type": "mapping_hold", "id": Int(keyCode), "active": false])
        }
    }
    private func handleJoystickKeyDown(keyCode: UInt16, index: Int, joystick: JoystickBinding, direction: JoystickDirection) {
        if var state = activeJoysticks[index] {
            state.held.insert(direction)
            let target = joystick.target(for: state.held, aspectWidthOverHeight: imageAspect)
            state.x = target.x
            state.y = target.y
            activeJoysticks[index] = state
            activeJoystickKeys[keyCode] = (index, direction)
            transport?.noteInput()
            transport?.send(["type": "mapped_touch", "id": state.id, "x": target.x, "y": target.y, "down": true])
            return
        }
        guard let id = nextTouchID() else { return }
        let held: Set<JoystickDirection> = [direction]
        let target = joystick.target(for: held, aspectWidthOverHeight: imageAspect)
        activeJoysticks[index] = (id, target.x, target.y, held)
        activeJoystickKeys[keyCode] = (index, direction)
        transport?.noteInput()
        // Touch down at center first so dynamic/floating joysticks anchor cleanly.
        transport?.send(["type": "mapped_touch", "id": id, "x": joystick.x, "y": joystick.y, "down": true])
        if target.x != joystick.x || target.y != joystick.y {
            transport?.send(["type": "mapped_touch", "id": id, "x": target.x, "y": target.y, "down": true])
        }
    }
    private func handleJoystickKeyUp(keyCode: UInt16) -> Bool {
        guard let entry = activeJoystickKeys.removeValue(forKey: keyCode) else { return false }
        guard var state = activeJoysticks[entry.index] else { return true }
        state.held.remove(entry.direction)
        if state.held.isEmpty {
            activeJoysticks.removeValue(forKey: entry.index)
            transport?.send(["type": "mapped_touch", "id": state.id, "x": state.x, "y": state.y, "down": false])
        } else if let mapping = transport?.mapping, mapping.scheme.joysticks.indices.contains(entry.index) {
            let joystick = mapping.scheme.joysticks[entry.index]
            let target = joystick.target(for: state.held, aspectWidthOverHeight: imageAspect)
            state.x = target.x
            state.y = target.y
            activeJoysticks[entry.index] = state
            transport?.send(["type": "mapped_touch", "id": state.id, "x": target.x, "y": target.y, "down": true])
        }
        return true
    }
    override func keyUp(with event: NSEvent) {
        if consumedCancelKeys.remove(event.keyCode) != nil { return }
        mappingPhysicalKeys.remove(event.keyCode)
        if inputBlocked {
            _ = mappedKeys.removeValue(forKey: event.keyCode)
            _ = activeJoystickKeys.removeValue(forKey: event.keyCode)
            _ = pressedKeys.removeValue(forKey: event.keyCode)
            return
        }
        if let action = mappedActions[event.keyCode] {
            if action.stopsOnKeyUp { stopMappedControl(event.keyCode) }
            return
        }
        if handleJoystickKeyUp(keyCode: event.keyCode) { return }
        if let key = pressedKeys.removeValue(forKey: event.keyCode) { transport?.send(["type": "key", "key": key, "down": false]) }
    }
    func insertText(_ string: Any, replacementRange: NSRange) {
        guard !inputBlocked else {
            marked = NSMutableAttributedString(string: "")
            markedSelection = NSRange(location: 0, length: 0)
            needsDisplay = true
            return
        }
        let text = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        let wasComposing = hasMarkedText()
        marked = NSMutableAttributedString(string: "")
        markedSelection = NSRange(location: 0, length: 0)
        needsDisplay = true
        if !wasComposing, let pendingKey, text.unicodeScalars.count == 1, text.unicodeScalars.first?.isASCII == true {
            pressedKeys[pendingKey.0] = pendingKey.1
            transport?.send(["type": "key", "key": pendingKey.1, "down": true])
        } else if !text.isEmpty {
            transport?.send(["type": "text", "text": text])
        }
    }
    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        guard !inputBlocked else { return }
        let value = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        let markedFont = WbFontResolver.nsFont(family: WbType.body.family, weight: .regular, size: WbType.title.size)
        marked = NSMutableAttributedString(string: value, attributes: [.foregroundColor: NSColor(WbColor.textPrimary), .font: markedFont])
        markedSelection = selectedRange
        needsDisplay = true
    }
    func unmarkText() { marked = NSMutableAttributedString(string: ""); needsDisplay = true }
    func selectedRange() -> NSRange { hasMarkedText() ? markedSelection : NSRange(location: NSNotFound, length: 0) }
    func markedRange() -> NSRange { hasMarkedText() ? NSRange(location: 0, length: marked.length) : NSRange(location: NSNotFound, length: 0) }
    func hasMarkedText() -> Bool { marked.length > 0 }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        guard range.location != NSNotFound, NSMaxRange(range) <= marked.length else { return nil }
        actualRange?.pointee = range
        return marked.attributedSubstring(from: range)
    }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [.foregroundColor, .font] }
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        actualRange?.pointee = range
        let local = NSRect(x: 12, y: max(12, bounds.height - 46), width: 1, height: 24)
        return window?.convertToScreen(convert(local, to: nil)) ?? .zero
    }
    func characterIndex(for point: NSPoint) -> Int { NSNotFound }
    override func doCommand(by selector: Selector) { }
}

struct PlayerView: NSViewRepresentable {
    let transport: PlayerTransport
    func makeNSView(context: Context) -> ScreenView {
        let view = ScreenView()
        view.transport = transport
        transport.onReleasePlayerInput = { [weak view] in view?.releaseHeldInputs() }
        transport.onGamepadEvent = { [weak view] event in view?.receiveGamepadEvent(event) }
        transport.onGamepadReset = { [weak view] in view?.resetGamepadPhysicalState() }
        transport.mapping.onChange = { [weak view, weak transport] in
            guard let view else { return }
            if let mapping = transport?.mapping, !mapping.enabled || mapping.editing { view.releaseHeldInputs() }
            view.needsDisplay = true
        }
        transport.onFrame = { [weak view] image, rotation in view?.present(image, rotation: rotation) }
        return view
    }
    func updateNSView(_ view: ScreenView, context: Context) {
        view.setScriptPlaybackBlocked(transport.scriptPlaybackActive)
        view.setSyncState(role: transport.syncRole, locked: transport.syncFollowerLocked)
    }
}
