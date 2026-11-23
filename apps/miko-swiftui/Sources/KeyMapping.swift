// SPDX-License-Identifier: GPL-3.0-only
import Foundation
import Combine

enum MappingKey {
    private static let specialKeyCodes: [UInt16: String] = [
        36: "ENTER",
        48: "TAB",
        76: "ENTER",
        123: "LEFT",
        124: "RIGHT",
        125: "DOWN",
        126: "UP",
    ]
    private static let specialNames: Set<String> = [
        "SPACE", "TAB", "ENTER", "UP", "DOWN", "LEFT", "RIGHT",
    ]

    static func normalize(keyCode: UInt16, charactersIgnoringModifiers: String?) -> String? {
        if let special = specialKeyCodes[keyCode] { return special }
        guard let raw = charactersIgnoringModifiers else { return nil }
        if raw == " " { return "SPACE" }
        guard raw.unicodeScalars.count == 1, let scalar = raw.unicodeScalars.first,
              (33...126).contains(scalar.value) else { return nil }
        return raw.uppercased()
    }

    static func isValid(_ key: String) -> Bool {
        if specialNames.contains(key) { return true }
        let bytes = Array(key.utf8)
        guard bytes.count == 1 else { return false }
        let b = bytes[0]
        return (33...126).contains(b) && !(97...122).contains(b)
    }

    static func isGamepad(_ key: String) -> Bool {
        ["PAD_SOUTH", "PAD_EAST", "PAD_WEST", "PAD_NORTH", "PAD_LEFTBUMPER", "PAD_RIGHTBUMPER",
         "PAD_BACK", "PAD_START", "PAD_GUIDE", "PAD_LEFTSTICK", "PAD_RIGHTSTICK", "PAD_UP", "PAD_DOWN",
         "PAD_LEFT", "PAD_RIGHT", "PAD_LEFTTRIGGER", "PAD_RIGHTTRIGGER"].contains(key)
    }

    static func normalizeCancel(keyCode: UInt16, charactersIgnoringModifiers: String?) -> String? {
        keyCode == 53 ? "ESC" : normalize(keyCode: keyCode, charactersIgnoringModifiers: charactersIgnoringModifiers)
    }

    static func isValidCancel(_ key: String) -> Bool { key == "ESC" || isValid(key) || isGamepad(key) }

    static func displayLabel(_ key: String) -> String {
        switch key {
        case "SPACE": return "SPC"
        case "UP": return "↑"
        case "DOWN": return "↓"
        case "LEFT": return "←"
        case "RIGHT": return "→"
        case "ENTER": return "ENT"
        default: return key
        }
    }
}

enum JoystickDirection: String, CaseIterable {
    case up, down, left, right
}

enum JoystickPreset: Equatable {
    case wasd
    case arrows

    var keys: (up: String, down: String, left: String, right: String) {
        switch self {
        case .wasd: return ("W", "S", "A", "D")
        case .arrows: return ("UP", "DOWN", "LEFT", "RIGHT")
        }
    }
}

enum MappingPlacementMode: Hashable {
    case none
    case tap
    case joystick
    case repeatTap
    case swipe
    case gesture
    case fpsView
    case skillAim
    case script

    var action: MappingAction {
        switch self {
        case .none, .tap, .joystick: return .tap
        case .repeatTap: return .repeatHold
        case .swipe: return .swipe
        case .gesture: return .gesture
        case .fpsView: return .fpsView
        case .skillAim: return .skillAim
        case .script: return .script
        }
    }
}

/// A final path position must be sent before its touch is released.
struct MappingPathStep {
    let x: Double
    let y: Double
    let finished: Bool
    let sendMove: Bool

    init(binding: MappingBinding, elapsedMs: Double, previousX: Double, previousY: Double) {
        let point = binding.pathPosition(elapsedMs: elapsedMs)
        x = point.x; y = point.y; finished = point.finished
        sendMove = point.x != previousX || point.y != previousY
    }
}

enum KeyMappingFailure: Int, Equatable {
    case unreadable = 0
    case bindingRequired = 1
    case conflict = 2
    case invalidImportFile = 3
    case cannotEnterEdit = 4

    var englishMessage: String {
        switch self {
        case .unreadable:
            return "The key map could not be read. Check the original file; it was not overwritten."
        case .bindingRequired:
            return "Bind a key to the selected position first"
        case .conflict:
            return "The key map changed elsewhere. Cancel the edit and open it again."
        case .invalidImportFile:
            return "The selected key map file is invalid."
        case .cannotEnterEdit:
            return "Cannot enter key-mapping edit mode right now."
        }
    }

    var nsError: NSError {
        NSError(domain: "MikoMapping", code: rawValue, userInfo: [NSLocalizedDescriptionKey: englishMessage])
    }
}

enum MappingTemplate: CaseIterable {
    case moba
    case shooter
    case arcade

    func scheme(for rotation: Int) -> (bindings: [MappingBinding], joysticks: [JoystickBinding]) {
        switch self {
        case .moba:
            return (
                bindings: [
                    MappingBinding(key: "J", x: 0.87, y: 0.78, rotation: rotation),
                    MappingBinding(key: "K", x: 0.77, y: 0.74, rotation: rotation),
                    MappingBinding(key: "L", x: 0.81, y: 0.58, rotation: rotation),
                    MappingBinding(key: "U", x: 0.89, y: 0.52, rotation: rotation),
                ],
                joysticks: [
                    JoystickBinding(up: "W", down: "S", left: "A", right: "D", x: 0.18, y: 0.72, radius: 0.13, rotation: rotation),
                ]
            )
        case .shooter:
            return (
                bindings: [
                    MappingBinding(key: "SPACE", x: 0.88, y: 0.76, rotation: rotation),
                    MappingBinding(key: "R", x: 0.78, y: 0.55, rotation: rotation),
                    MappingBinding(key: "F", x: 0.70, y: 0.68, rotation: rotation),
                    MappingBinding(key: "1", x: 0.44, y: 0.88, rotation: rotation),
                    MappingBinding(key: "2", x: 0.56, y: 0.88, rotation: rotation),
                ],
                joysticks: [
                    JoystickBinding(up: "W", down: "S", left: "A", right: "D", x: 0.18, y: 0.70, radius: 0.13, rotation: rotation),
                ]
            )
        case .arcade:
            return (
                bindings: [
                    MappingBinding(key: "Z", x: 0.76, y: 0.78, rotation: rotation),
                    MappingBinding(key: "X", x: 0.85, y: 0.66, rotation: rotation),
                    MappingBinding(key: "SPACE", x: 0.88, y: 0.82, rotation: rotation),
                ],
                joysticks: [
                    JoystickBinding(up: "UP", down: "DOWN", left: "LEFT", right: "RIGHT", x: 0.18, y: 0.74, radius: 0.12, rotation: rotation),
                ]
            )
        }
    }
}

enum MappingAction: String, Codable, CaseIterable {
    case tap, holdToggle, repeatHold, repeatToggle, swipe, gesture, fpsView, skillAim, script

    var stopsOnKeyUp: Bool { self == .tap || self == .repeatHold || self == .skillAim }
    var holdsOwnershipBetweenTouches: Bool { self == .repeatHold || self == .repeatToggle || self == .fpsView }
}

/// The contact position for FPS view and MOBA aiming; the caller owns the
/// paired down/up events and the mouse-capture lifetime.
struct MappingAimState {
    let originX: Double
    let originY: Double
    let sensitivity: Double
    let radius: Double
    private(set) var x: Double
    private(set) var y: Double

    init(binding: MappingBinding) {
        originX = binding.x; originY = binding.y
        x = binding.x; y = binding.y
        sensitivity = binding.sensitivity; radius = binding.aimRadius
    }

    mutating func move(dx: Double, dy: Double, width: Double, height: Double) {
        guard width > 0, height > 0 else { return }
        var sx = (x - originX) * width + dx * sensitivity
        var sy = (y - originY) * height + dy * sensitivity
        let limit = min(width, height) * radius
        let distance = hypot(sx, sy)
        if distance > limit && distance > 0 { sx *= limit / distance; sy *= limit / distance }
        x = min(1, max(0, originX + sx / width))
        y = min(1, max(0, originY + sy / height))
    }

    func atLimit(width: Double, height: Double) -> Bool {
        guard width > 0, height > 0 else { return false }
        return hypot((x - originX) * width, (y - originY) * height) >= min(width, height) * radius * 0.98
    }
}

enum MappingPressDecision: Equatable {
    case start, stop, ignore
    static func decide(action: MappingAction, active: Bool, physicalDown: Bool, isRepeat: Bool) -> Self {
        if physicalDown || isRepeat { return .ignore }
        if !active { return .start }
        return action == .swipe || action == .gesture ? .ignore : .stop
    }
}

struct MappingTracePoint: Codable, Equatable {
    var x: Double
    var y: Double
    var tMs: Int
}

/// One repeating contact. The first down is sent by the caller before the
/// cycle starts; stopping emits an up only when that contact is still down.
struct MappingRepeatCycle {
    private(set) var isDown = true
    mutating func advance() -> Bool {
        isDown.toggle()
        return isDown
    }
    mutating func stopNeedsUp() -> Bool {
        let result = isDown
        isDown = false
        return result
    }
}

struct MappingBinding: Codable, Equatable {
    var key: String
    var x: Double
    var y: Double
    var rotation: Int
    var action: MappingAction = .tap
    var repeatIntervalMs: Int = 120
    var endX: Double = 0.5
    var endY: Double = 0.5
    var durationMs: Int = 300
    var trace: [MappingTracePoint] = []
    var sensitivity: Double = 1
    var aimRadius: Double = 0.2
    var scriptID: String = ""
    var cancelKey: String?
    func cancelsSkill(with key: String) -> Bool { action == .skillAim && cancelKey == key }

    enum CodingKeys: String, CodingKey { case key, x, y, rotation, action, repeatIntervalMs, endX, endY, durationMs, trace, sensitivity, aimRadius, scriptID, cancelKey }
    init(key: String, x: Double, y: Double, rotation: Int, action: MappingAction = .tap, repeatIntervalMs: Int = 120) {
        self.key = key; self.x = x; self.y = y; self.rotation = rotation
        self.action = action; self.repeatIntervalMs = repeatIntervalMs
    }
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        key = try container.decode(String.self, forKey: .key)
        x = try container.decode(Double.self, forKey: .x)
        y = try container.decode(Double.self, forKey: .y)
        rotation = try container.decode(Int.self, forKey: .rotation)
        action = try container.decodeIfPresent(MappingAction.self, forKey: .action) ?? .tap
        repeatIntervalMs = try container.decodeIfPresent(Int.self, forKey: .repeatIntervalMs) ?? 120
        endX = try container.decodeIfPresent(Double.self, forKey: .endX) ?? 0.5
        endY = try container.decodeIfPresent(Double.self, forKey: .endY) ?? 0.5
        durationMs = try container.decodeIfPresent(Int.self, forKey: .durationMs) ?? 300
        trace = try container.decodeIfPresent([MappingTracePoint].self, forKey: .trace) ?? []
        sensitivity = try container.decodeIfPresent(Double.self, forKey: .sensitivity) ?? 1
        aimRadius = try container.decodeIfPresent(Double.self, forKey: .aimRadius) ?? 0.2
        scriptID = try container.decodeIfPresent(String.self, forKey: .scriptID) ?? ""
        cancelKey = try container.decodeIfPresent(String.self, forKey: .cancelKey)
    }
    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(key, forKey: .key); try container.encode(x, forKey: .x)
        try container.encode(y, forKey: .y); try container.encode(rotation, forKey: .rotation)
        if action != .tap {
            try container.encode(action, forKey: .action)
            try container.encode(repeatIntervalMs, forKey: .repeatIntervalMs)
            if action == .swipe {
                try container.encode(endX, forKey: .endX); try container.encode(endY, forKey: .endY)
                try container.encode(durationMs, forKey: .durationMs)
            }
            if action == .gesture { try container.encode(trace, forKey: .trace) }
            if action == .fpsView || action == .skillAim {
                try container.encode(sensitivity, forKey: .sensitivity)
                try container.encode(aimRadius, forKey: .aimRadius)
                if action == .skillAim {
                    try container.encode(endX, forKey: .endX); try container.encode(endY, forKey: .endY)
                    try container.encodeIfPresent(cancelKey, forKey: .cancelKey)
                }
            }
            if action == .script { try container.encode(scriptID, forKey: .scriptID) }
        }
    }

    /// Interpolates recorded points on the normalized display at elapsed time.
    func pathPosition(elapsedMs: Double) -> (x: Double, y: Double, finished: Bool) {
        if action == .gesture, let first = trace.first, let last = trace.last {
            if elapsedMs >= Double(last.tMs) { return (last.x, last.y, true) }
            var previous = first
            for point in trace.dropFirst() {
                if elapsedMs <= Double(point.tMs), point.tMs > previous.tMs {
                    let progress = max(0, min(1, (elapsedMs - Double(previous.tMs)) / Double(point.tMs - previous.tMs)))
                    return (previous.x + (point.x - previous.x) * progress, previous.y + (point.y - previous.y) * progress, false)
                }
                previous = point
            }
            return (last.x, last.y, false)
        }
        let progress = max(0, min(1, elapsedMs / Double(max(1, durationMs))))
        return (x + (endX - x) * progress, y + (endY - y) * progress, progress >= 1)
    }
}

struct JoystickBinding: Codable, Equatable {
    var up: String = "W"
    var down: String = "S"
    var left: String = "A"
    var right: String = "D"
    var x: Double
    var y: Double
    var radius: Double = 0.12
    var rotation: Int

    var keys: [String] { [up, down, left, right] }

    var preset: JoystickPreset? {
        if (up, down, left, right) == ("W", "S", "A", "D") { return .wasd }
        if (up, down, left, right) == ("UP", "DOWN", "LEFT", "RIGHT") { return .arrows }
        return nil
    }

    func direction(for key: String) -> JoystickDirection? {
        if key == up { return .up }
        if key == down { return .down }
        if key == left { return .left }
        if key == right { return .right }
        return nil
    }

    func target(for held: Set<JoystickDirection>, aspectWidthOverHeight aspect: Double) -> (x: Double, y: Double) {
        let dx = (held.contains(.right) ? 1.0 : 0.0) - (held.contains(.left) ? 1.0 : 0.0)
        let dy = (held.contains(.down) ? 1.0 : 0.0) - (held.contains(.up) ? 1.0 : 0.0)
        guard dx != 0 || dy != 0, aspect.isFinite, aspect > 0 else { return (x, y) }
        let length = hypot(dx, dy)
        let ux = dx / length
        let uy = dy / length
        let rx = aspect >= 1 ? radius / aspect : radius
        let ry = aspect >= 1 ? radius : radius * aspect
        return (min(1, max(0, x + ux * rx)), min(1, max(0, y + uy * ry)))
    }

    func analogTarget(x dx: Double, y dy: Double, aspectWidthOverHeight aspect: Double) -> (x: Double, y: Double) {
        guard dx.isFinite, dy.isFinite, aspect.isFinite, aspect > 0 else { return (x, y) }
        let magnitude = hypot(dx, dy)
        let scale = magnitude > 1 ? 1 / magnitude : 1
        let rx = aspect >= 1 ? radius / aspect : radius
        let ry = aspect >= 1 ? radius : radius * aspect
        return (min(1, max(0, x + dx * scale * rx)), min(1, max(0, y - dy * scale * ry)))
    }
}

struct MappingPoint: Equatable {
    var x: Double
    var y: Double
    var rotation: Int
}

struct MappingTraceTarget: Equatable {
    var key: String
    var rotation: Int
}

struct MappingScheme: Codable, Equatable {
    var version: Int = 1
    var bindings: [MappingBinding] = []
    var joysticks: [JoystickBinding] = []

    enum CodingKeys: String, CodingKey {
        case version, bindings, joysticks
    }

    init(version: Int = 1, bindings: [MappingBinding] = [], joysticks: [JoystickBinding] = []) {
        self.version = version
        self.bindings = bindings
        self.joysticks = joysticks
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        version = try container.decode(Int.self, forKey: .version)
        bindings = try container.decodeIfPresent([MappingBinding].self, forKey: .bindings) ?? []
        joysticks = try container.decodeIfPresent([JoystickBinding].self, forKey: .joysticks) ?? []
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(version, forKey: .version)
        try container.encode(bindings, forKey: .bindings)
        if !joysticks.isEmpty {
            try container.encode(joysticks, forKey: .joysticks)
        }
    }

    var isEmpty: Bool { bindings.isEmpty && joysticks.isEmpty }

    func validate() -> Bool {
        guard (1...6).contains(version), bindings.count <= 256, joysticks.count <= 16 else { return false }
        var seen = Set<String>()
        var bindingCounts: [Int: Int] = [:]
        var joystickCounts: [Int: Int] = [:]
        for b in bindings {
            guard (MappingKey.isValid(b.key) || (version >= 6 && MappingKey.isGamepad(b.key))), b.x.isFinite, b.y.isFinite,
                  (0...1).contains(b.x), (0...1).contains(b.y), (0...3).contains(b.rotation),
                  (40...5000).contains(b.repeatIntervalMs), (version >= 2 || b.action == .tap),
                  (version >= 3 || (b.action != .fpsView && b.action != .skillAim)),
                  (b.action != .script || (version >= 4 && UUID(uuidString: b.scriptID) != nil)),
                  (b.cancelKey == nil || (version >= 5 && b.action == .skillAim && MappingKey.isValidCancel(b.cancelKey ?? "") && (!MappingKey.isGamepad(b.cancelKey ?? "") || version >= 6) && b.cancelKey != b.key)),
                  b.sensitivity.isFinite, (0.1...5).contains(b.sensitivity),
                  b.aimRadius.isFinite, (0.05...0.45).contains(b.aimRadius),
                  b.endX.isFinite, b.endY.isFinite, (0...1).contains(b.endX), (0...1).contains(b.endY), (40...10000).contains(b.durationMs),
                  seen.insert("\(b.rotation):\(b.key)").inserted else { return false }
            if b.action == .gesture {
                guard (2...1024).contains(b.trace.count), b.trace.first?.tMs == 0,
                      b.trace.first?.x == b.x, b.trace.first?.y == b.y,
                      let last = b.trace.last, (1...10000).contains(last.tMs),
                      b.trace.allSatisfy({ $0.x.isFinite && $0.y.isFinite && (0...1).contains($0.x) && (0...1).contains($0.y) && (0...10000).contains($0.tMs) }),
                      zip(b.trace, b.trace.dropFirst()).allSatisfy({ pair in pair.0.tMs <= pair.1.tMs }) else { return false }
            } else if !b.trace.isEmpty { return false }
            bindingCounts[b.rotation, default: 0] += 1
            guard bindingCounts[b.rotation, default: 0] <= 64 else { return false }
        }
        for j in joysticks {
            guard j.x.isFinite, j.y.isFinite, (0...1).contains(j.x), (0...1).contains(j.y),
                  j.radius.isFinite, (0.05...0.35).contains(j.radius), (0...3).contains(j.rotation) else { return false }
            joystickCounts[j.rotation, default: 0] += 1
            guard joystickCounts[j.rotation, default: 0] <= 4 else { return false }
            for key in j.keys {
                guard MappingKey.isValid(key), seen.insert("\(j.rotation):\(key)").inserted else { return false }
            }
        }
        return true
    }
}

final class KeyMappingStore: ObservableObject {
    private struct AppAssociations: Codable {
        var enabled = false
        var profiles: [String: String] = [:]
    }
    static let defaultMarkerOpacity: Double = 0.85
    private static let markerOpacityKey = "miko.keymap.markerOpacity"

    @Published private(set) var scheme = MappingScheme() { didSet { onChange?() } }
    @Published private(set) var draft = MappingScheme() { didSet { onChange?() } }
    @Published private(set) var editing = false { didSet { if !editing { traceTarget = nil }; onChange?() } }
    @Published var enabled = false { didSet { onChange?() } }
    @Published var hints = true { didSet { onChange?() } }
    @Published var markerOpacity: Double {
        didSet {
            let clamped = min(1.0, max(0.15, markerOpacity))
            if clamped != markerOpacity {
                markerOpacity = clamped
                return
            }
            UserDefaults.standard.set(clamped, forKey: Self.markerOpacityKey)
            onChange?()
        }
    }
    @Published var placementMode: MappingPlacementMode = .tap {
        didSet {
            if placementMode == .none { clearSelection() }
            else if placementMode == .joystick { pendingPoint = nil }
            if placementMode != oldValue { onChange?() }
        }
    }
    var placementScriptID: String?
    @Published private(set) var pendingPoint: MappingPoint? { didSet { onChange?() } }
    @Published private(set) var selectedKey: String? { didSet { onChange?() } }
    @Published private(set) var selectedJoystickIndex: Int? { didSet { onChange?() } }
    @Published private(set) var currentRotation = 0
    @Published private(set) var traceTarget: MappingTraceTarget?
    @Published private(set) var profiles: [String] = []
    @Published private(set) var activeProfileName: String?
    @Published private(set) var foregroundPackage: String?
    @Published private(set) var foregroundIsLauncher: Bool? = nil
    @Published var appNames: [String: String] = [:]
    var currentProfilePackage: String? {
        associations.profiles.keys.sorted().first { associations.profiles[$0] == (activeProfileName ?? "") }
    }
    @Published private(set) var autoSwitchEnabled = false
    @Published private(set) var associatedProfile: String?
    @Published private(set) var associationError: String?
    private var associations = AppAssociations()
    private var manuallyOverriddenPackage: String?
    @Published private(set) var failure: KeyMappingFailure?
    var error: String? { failure?.englishMessage }
    var currentFileURL: URL? { location }
    var pending: Bool { pendingPoint != nil }
    var hasUnsavedChanges: Bool { editing && (draft != scheme || resettingCorrupted) }
    var onChange: (() -> Void)?
    private var baseLocation: URL?
    private var location: URL?
    private var baseline: Data?
    private var resettingCorrupted = false

    init() {
        let defaults = UserDefaults.standard
        if defaults.object(forKey: Self.markerOpacityKey) != nil {
            markerOpacity = min(1.0, max(0.15, defaults.double(forKey: Self.markerOpacityKey)))
        } else {
            markerOpacity = Self.defaultMarkerOpacity
        }
    }

    func load(path: String) {
        let base = URL(fileURLWithPath: path)
        baseLocation = base
        loadAssociations()
        refreshProfiles()
        if let stored = readActiveProfileName(),
           let profileURL = profileURL(for: stored),
           FileManager.default.fileExists(atPath: profileURL.path) {
            activeProfileName = stored
            loadFile(at: profileURL)
        } else {
            activeProfileName = nil
            loadFile(at: base)
        }
    }
    func selectProfile(_ name: String?, automatic: Bool = false) -> Bool {
        guard !hasUnsavedChanges, let baseLocation else { return false }
        let wasEditing = editing
        let targetURL: URL
        if let name {
            guard let url = profileURL(for: name),
                  FileManager.default.fileExists(atPath: url.path) else { return false }
            targetURL = url
        } else {
            targetURL = baseLocation
        }
        persistActiveProfileName(name)
        if !automatic { manuallyOverriddenPackage = foregroundPackage }
        activeProfileName = name
        loadFile(at: targetURL)
        if wasEditing {
            if failure == nil {
                draft = scheme
                placementMode = .tap
                pendingPoint = nil
                selectedKey = nil
                selectedJoystickIndex = nil
            } else {
                editing = false
            }
        }
        return true
    }

    private var associationsURL: URL? {
        baseLocation?.deletingPathExtension().appendingPathExtension("applications.json")
    }

    private func loadAssociations() {
        associations = AppAssociations(); associationError = nil
        guard let url = associationsURL, FileManager.default.fileExists(atPath: url.path) else {
            autoSwitchEnabled = UserDefaults.standard.bool(forKey: "miko.keymap.autoSwitch"); return
        }
        do {
            associations = try JSONDecoder().decode(AppAssociations.self, from: Data(contentsOf: url))
            if UserDefaults.standard.object(forKey: "miko.keymap.autoSwitch") == nil { UserDefaults.standard.set(associations.enabled, forKey: "miko.keymap.autoSwitch") }
            autoSwitchEnabled = UserDefaults.standard.bool(forKey: "miko.keymap.autoSwitch")
        } catch {
            autoSwitchEnabled = false
            associationError = "Could not read application associations: \(error.localizedDescription)"
        }
    }

    private func saveAssociations(_ value: AppAssociations) -> Bool {
        guard associationError == nil, let url = associationsURL else { return false }
        do {
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try JSONEncoder().encode(value).write(to: url, options: .atomic)
            associations = value; autoSwitchEnabled = value.enabled
            return true
        } catch {
            associationError = "Could not save application associations: \(error.localizedDescription)"
            return false
        }
    }

    func setAutoSwitch(_ enabled: Bool) -> Bool {
        var next = associations; next.enabled = enabled
        if enabled { manuallyOverriddenPackage = nil }
        guard saveAssociations(next) else { return false }
        UserDefaults.standard.set(enabled, forKey: "miko.keymap.autoSwitch")
        return true
    }

    func associateForegroundWithCurrentProfile() -> Bool {
        guard let package = foregroundPackage, foregroundIsLauncher == false else { return false }
        var next = associations
        next.profiles = next.profiles.filter { $0.value != (activeProfileName ?? "") }
        next.profiles[package] = activeProfileName ?? ""
        guard saveAssociations(next) else { return false }
        associatedProfile = activeProfileName ?? ""
        manuallyOverriddenPackage = nil
        return true
    }

    func removeForegroundAssociation() -> Bool {
        guard let package = currentProfilePackage else { return false }
        var next = associations; next.profiles.removeValue(forKey: package)
        guard saveAssociations(next) else { return false }
        associatedProfile = nil
        return true
    }

    func updateForeground(_ package: String, isLauncher: Bool? = false, beforeSwitch: () -> Void) {
        foregroundIsLauncher = isLauncher
        if foregroundPackage != package {
            foregroundPackage = package; manuallyOverriddenPackage = nil
        }
        associatedProfile = associations.profiles[package]
        guard UserDefaults.standard.bool(forKey: "miko.keymap.autoSwitch"), associationError == nil, !editing,
              manuallyOverriddenPackage != package, let profile = associations.profiles[package] else { return }
        let target: String? = profile.isEmpty ? nil : profile
        guard target != activeProfileName else { return }
        if let target, !profiles.contains(target) { return }
        beforeSwitch()
        _ = selectProfile(target, automatic: true)
    }
    func createProfile(named rawName: String, duplicateCurrent: Bool) -> Bool {
        guard !hasUnsavedChanges, let name = Self.sanitizedProfileName(rawName),
              let url = profileURL(for: name),
              !FileManager.default.fileExists(atPath: url.path) else { return false }
        let wasEditing = editing
        let content = duplicateCurrent ? (wasEditing ? draft : scheme) : MappingScheme()
        guard (try? writeScheme(content, to: url)) != nil else { return false }
        manuallyOverriddenPackage = foregroundPackage
        persistActiveProfileName(name)
        activeProfileName = name
        refreshProfiles()
        loadFile(at: url)
        if wasEditing {
            draft = scheme
            placementMode = .tap
            pendingPoint = nil
            selectedKey = nil
            selectedJoystickIndex = nil
        }
        return true
    }
    func saveAsProfile(named rawName: String) -> Bool {
        guard editing, !pending, draft.validate(),
              let name = Self.sanitizedProfileName(rawName),
              let url = profileURL(for: name),
              !FileManager.default.fileExists(atPath: url.path) else { return false }
        guard let bytes = try? writeScheme(draft, to: url) else { return false }
        manuallyOverriddenPackage = foregroundPackage
        persistActiveProfileName(name)
        activeProfileName = name
        location = url
        baseline = bytes
        scheme = draft
        enabled = !scheme.isEmpty
        placementMode = .tap
        pendingPoint = nil
        selectedKey = nil
        selectedJoystickIndex = nil
        resettingCorrupted = false
        editing = false
        failure = nil
        refreshProfiles()
        return true
    }
    func renameProfile(from oldName: String, to rawNewName: String) -> Bool {
        guard let newName = Self.sanitizedProfileName(rawNewName),
              oldName != newName,
              let src = profileURL(for: oldName),
              let dst = profileURL(for: newName),
              FileManager.default.fileExists(atPath: src.path),
              !FileManager.default.fileExists(atPath: dst.path) else { return false }
        do {
            try FileManager.default.moveItem(at: src, to: dst)
        } catch {
            return false
        }
        if activeProfileName == oldName {
            activeProfileName = newName
            location = dst
            persistActiveProfileName(newName)
        }
        if associations.profiles.values.contains(oldName) {
            var next = associations
            for (package, profile) in associations.profiles where profile == oldName { next.profiles[package] = newName }
            _ = saveAssociations(next)
        }
        refreshProfiles()
        return true
    }
    func deleteProfile(named name: String) -> Bool {
        if activeProfileName == name && hasUnsavedChanges { return false }
        guard let url = profileURL(for: name),
              FileManager.default.fileExists(atPath: url.path) else { return false }
        do {
            try FileManager.default.removeItem(at: url)
        } catch {
            return false
        }
        if associations.profiles.values.contains(name) {
            var next = associations
            next.profiles = next.profiles.filter { $0.value != name }
            _ = saveAssociations(next)
        }
        refreshProfiles()
        if activeProfileName == name {
            _ = selectProfile(nil)
        }
        return true
    }
    func applyTemplate(_ template: MappingTemplate, rotation: Int) {
        guard editing, (0...3).contains(rotation) else { return }
        let preset = template.scheme(for: rotation)
        let copy = MappingScheme(bindings: preset.bindings, joysticks: preset.joysticks)
        guard copy.validate() else { return }
        placementMode = .tap
        pendingPoint = nil
        selectedKey = nil
        selectedJoystickIndex = nil
        currentRotation = rotation
        draft = copy
    }
    func exportData() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        return try encoder.encode(editing ? draft : scheme)
    }
    func importScheme(from url: URL) throws {
        let data = try Data(contentsOf: url)
        let value = try JSONDecoder().decode(MappingScheme.self, from: data)
        guard value.validate() else { throw KeyMappingFailure.invalidImportFile.nsError }
        if !editing, !begin() { throw KeyMappingFailure.cannotEnterEdit.nsError }
        placementMode = .tap
        pendingPoint = nil
        selectedKey = nil
        selectedJoystickIndex = nil
        draft = value
    }
    func reloadCurrent() {
        guard !editing, let location else { return }
        loadFile(at: location)
    }
    func resetCorruptedForEditing() -> Bool {
        guard !editing, let location, failure == .unreadable else { return false }
        baseline = try? Data(contentsOf: location)
        scheme = MappingScheme()
        draft = MappingScheme()
        failure = nil
        enabled = false
        resettingCorrupted = true
        placementMode = .tap
        pendingPoint = nil
        selectedKey = nil
        selectedJoystickIndex = nil
        editing = true
        return true
    }
    func begin() -> Bool {
        guard location != nil, failure == nil else { return false }
        resettingCorrupted = false
        draft = scheme; placementMode = .tap; pendingPoint = nil; selectedKey = nil; selectedJoystickIndex = nil; editing = true; return true
    }
    func cancel() {
        let restoreCorrupted = resettingCorrupted
        resettingCorrupted = false
        placementMode = .tap; pendingPoint = nil; selectedKey = nil; selectedJoystickIndex = nil; editing = false
        if restoreCorrupted, let location { loadFile(at: location) }
    }
    func place(x: Double, y: Double, rotation: Int) {
        guard editing, Self.isValidPoint(x: x, y: y, rotation: rotation) else { return }
        selectedKey = nil
        selectedJoystickIndex = nil
        currentRotation = rotation
        pendingPoint = MappingPoint(x: x, y: y, rotation: rotation)
    }
    func select(key: String, rotation: Int) -> Bool {
        guard editing, draft.bindings.contains(where: { $0.key == key && $0.rotation == rotation }) else { return false }
        traceTarget = nil
        pendingPoint = nil
        selectedJoystickIndex = nil
        currentRotation = rotation
        selectedKey = key
        return true
    }
    func selectJoystick(at index: Int) -> Bool {
        guard editing, draft.joysticks.indices.contains(index) else { return false }
        traceTarget = nil
        pendingPoint = nil
        selectedKey = nil
        currentRotation = draft.joysticks[index].rotation
        selectedJoystickIndex = index
        return true
    }
    func clearSelection() {
        guard editing else { return }
        traceTarget = nil
        pendingPoint = nil
        selectedKey = nil
        selectedJoystickIndex = nil
    }
    func move(key: String, rotation: Int, x: Double, y: Double) -> Bool {
        guard editing, Self.isValidPoint(x: x, y: y, rotation: rotation),
              let index = draft.bindings.firstIndex(where: { $0.key == key && $0.rotation == rotation }) else { return false }
        var copy = draft
        if copy.bindings[index].action == .gesture {
            let dx = x - copy.bindings[index].x, dy = y - copy.bindings[index].y
            copy.bindings[index].trace = copy.bindings[index].trace.map { MappingTracePoint(x: $0.x + dx, y: $0.y + dy, tMs: $0.tMs) }
            if !copy.bindings[index].trace.isEmpty {
                copy.bindings[index].trace[0].x = x; copy.bindings[index].trace[0].y = y
            }
        }
        copy.bindings[index].x = x
        copy.bindings[index].y = y
        guard copy.validate() else { return false }
        pendingPoint = nil
        selectedJoystickIndex = nil
        selectedKey = key
        currentRotation = rotation
        draft = copy
        return true
    }
    func moveJoystick(at index: Int, x: Double, y: Double) -> Bool {
        guard editing, draft.joysticks.indices.contains(index) else { return false }
        let rotation = draft.joysticks[index].rotation
        guard Self.isValidPoint(x: x, y: y, rotation: rotation) else { return false }
        var copy = draft
        copy.joysticks[index].x = x
        copy.joysticks[index].y = y
        guard copy.validate() else { return false }
        pendingPoint = nil
        selectedKey = nil
        selectedJoystickIndex = index
        currentRotation = rotation
        draft = copy
        return true
    }
    func resizeJoystick(at index: Int, radius: Double) -> Bool {
        guard editing, draft.joysticks.indices.contains(index) else { return false }
        var copy = draft
        copy.joysticks[index].radius = min(0.35, max(0.05, radius))
        guard copy.validate() else { return false }
        draft = copy
        return true
    }
    func setJoystickPreset(at index: Int, preset: JoystickPreset) -> Bool {
        guard editing, draft.joysticks.indices.contains(index) else { return false }
        var copy = draft
        let keys = preset.keys
        copy.joysticks[index].up = keys.up
        copy.joysticks[index].down = keys.down
        copy.joysticks[index].left = keys.left
        copy.joysticks[index].right = keys.right
        guard copy.validate() else { return false }
        draft = copy
        return true
    }
    func setAction(key: String, rotation: Int, action: MappingAction, repeatIntervalMs: Int) -> Bool {
        guard editing, let index = draft.bindings.firstIndex(where: { $0.key == key && $0.rotation == rotation }) else { return false }
        traceTarget = nil
        var copy = draft
        copy.bindings[index].action = action
        if action != .skillAim { copy.bindings[index].cancelKey = nil }
        if action != .script { copy.bindings[index].scriptID = "" }
        copy.bindings[index].repeatIntervalMs = action == .tap ? 120 : repeatIntervalMs
        if action != .swipe && action != .skillAim {
            copy.bindings[index].endX = 0.5; copy.bindings[index].endY = 0.5
            copy.bindings[index].durationMs = 300
        }
        if action == .gesture {
            if copy.bindings[index].trace.isEmpty {
                let point = MappingTracePoint(x: copy.bindings[index].x, y: copy.bindings[index].y, tMs: 0)
                copy.bindings[index].trace = [point, MappingTracePoint(x: point.x, y: point.y, tMs: 300)]
            }
        } else {
            copy.bindings[index].trace = []
        }
        if action == .fpsView || action == .skillAim { copy.version = max(copy.version, 3) }
        else if action == .script { copy.version = max(copy.version, 4) }
        else if action != .tap { copy.version = max(copy.version, 2) }
        guard copy.validate() else { return false }
        draft = copy
        return true
    }
    func setScriptAction(key: String, rotation: Int, scriptID: String) -> Bool {
        guard editing, UUID(uuidString: scriptID) != nil,
              let index = draft.bindings.firstIndex(where: { $0.key == key && $0.rotation == rotation }) else { return false }
        var copy = draft
        copy.version = max(copy.version, 4); copy.bindings[index].action = .script; copy.bindings[index].scriptID = scriptID
        copy.bindings[index].cancelKey = nil
        copy.bindings[index].trace = []
        guard copy.validate() else { return false }
        draft = copy; return true
    }
    func setAim(key: String, rotation: Int, sensitivity: Double, radius: Double, cancelX: Double, cancelY: Double) -> Bool {
        guard editing, let index = draft.bindings.firstIndex(where: { $0.key == key && $0.rotation == rotation }),
              draft.bindings[index].action == .fpsView || draft.bindings[index].action == .skillAim else { return false }
        var copy = draft
        copy.bindings[index].sensitivity = sensitivity; copy.bindings[index].aimRadius = radius
        copy.bindings[index].endX = cancelX; copy.bindings[index].endY = cancelY
        guard copy.validate() else { return false }
        draft = copy; return true
    }
    func setSkillCancelKey(key: String, rotation: Int, cancelKey: String?) -> Bool {
        guard editing, let index = draft.bindings.firstIndex(where: { $0.key == key && $0.rotation == rotation }),
              draft.bindings[index].action == .skillAim else { return false }
        var copy = draft
        copy.bindings[index].cancelKey = cancelKey
        if let cancelKey { copy.version = max(copy.version, MappingKey.isGamepad(cancelKey) ? 6 : 5) }
        guard copy.validate() else { return false }
        draft = copy
        return true
    }
    func setSwipe(key: String, rotation: Int, endX: Double, endY: Double, durationMs: Int) -> Bool {
        guard editing, let index = draft.bindings.firstIndex(where: { $0.key == key && $0.rotation == rotation }), draft.bindings[index].action == .swipe else { return false }
        var copy = draft
        copy.bindings[index].endX = endX; copy.bindings[index].endY = endY
        copy.bindings[index].durationMs = durationMs
        guard copy.validate() else { return false }
        draft = copy
        return true
    }
    func setTrace(key: String, rotation: Int, points: [MappingTracePoint]) -> Bool {
        guard editing, let index = draft.bindings.firstIndex(where: { $0.key == key && $0.rotation == rotation }), draft.bindings[index].action == .gesture, let first = points.first else { return false }
        var copy = draft
        copy.bindings[index].x = first.x; copy.bindings[index].y = first.y
        copy.bindings[index].trace = points
        guard copy.validate() else { return false }
        draft = copy
        return true
    }
    func beginTrace(key: String, rotation: Int) -> Bool {
        guard editing, draft.bindings.contains(where: { $0.key == key && $0.rotation == rotation && $0.action == .gesture }) else { return false }
        traceTarget = MappingTraceTarget(key: key, rotation: rotation)
        return true
    }
    func cancelTrace() { traceTarget = nil }
    func addJoystick(x: Double, y: Double, rotation: Int, preset: JoystickPreset = .wasd) -> Bool {
        guard editing, Self.isValidPoint(x: x, y: y, rotation: rotation) else { return false }
        var copy = draft
        let keys = preset.keys
        copy.joysticks.append(JoystickBinding(up: keys.up, down: keys.down, left: keys.left, right: keys.right, x: x, y: y, rotation: rotation))
        guard copy.validate() else { return false }
        pendingPoint = nil
        selectedKey = nil
        currentRotation = rotation
        selectedJoystickIndex = copy.joysticks.count - 1
        draft = copy
        return true
    }
    func remove(key: String, rotation: Int) -> Bool {
        guard editing else { return false }
        var copy = draft
        let countBefore = copy.bindings.count
        copy.bindings.removeAll { $0.key == key && $0.rotation == rotation }
        guard copy.bindings.count != countBefore else { return false }
        if selectedKey == key { selectedKey = nil }
        draft = copy
        return true
    }
    func removeJoystick(at index: Int) -> Bool {
        guard editing, draft.joysticks.indices.contains(index) else { return false }
        var copy = draft
        copy.joysticks.remove(at: index)
        if let selected = selectedJoystickIndex {
            if selected == index { selectedJoystickIndex = nil }
            else if selected > index { selectedJoystickIndex = selected - 1 }
        }
        draft = copy
        return true
    }
    func assign(_ key: String) -> Bool {
        guard editing else { return false }
        var copy = draft
        if MappingKey.isGamepad(key) { copy.version = max(copy.version, 6) }
        if let point = pendingPoint {
            let action = placementMode.action
            var binding = MappingBinding(key: key, x: point.x, y: point.y, rotation: point.rotation, action: action)
            if action == .gesture {
                binding.trace = [MappingTracePoint(x: point.x, y: point.y, tMs: 0),
                                 MappingTracePoint(x: point.x, y: point.y, tMs: 300)]
            }
            if action == .script { binding.scriptID = placementScriptID ?? "" }
            if action == .fpsView || action == .skillAim { copy.version = max(copy.version, 3) }
            else if action == .script { copy.version = max(copy.version, 4) }
            else if action != .tap { copy.version = max(copy.version, 2) }
            if let i = copy.bindings.firstIndex(where: { $0.key == key && $0.rotation == point.rotation }) {
                copy.bindings[i] = binding
            } else {
                copy.bindings.append(binding)
            }
            guard copy.validate() else { return false }
            draft = copy; pendingPoint = nil; selectedKey = key; return true
        }
        if let existingKey = selectedKey,
           let existing = copy.bindings.first(where: { $0.key == existingKey && $0.rotation == currentRotation }) {
            copy.bindings.removeAll { ($0.key == existingKey || $0.key == key) && $0.rotation == existing.rotation }
            var rebound = existing
            rebound.key = key
            copy.bindings.append(rebound)
            guard copy.validate() else { return false }
            draft = copy; selectedKey = key; return true
        }
        return false
    }
    func clear(rotation: Int? = nil) {
        guard editing else { return }
        if let rotation {
            var copy = draft
            copy.bindings.removeAll { $0.rotation == rotation }
            copy.joysticks.removeAll { $0.rotation == rotation }
            draft = copy
        } else {
            draft = MappingScheme()
        }
        pendingPoint = nil
        selectedKey = nil
        selectedJoystickIndex = nil
    }
    func hasOtherRotationBindings(for rotation: Int) -> Bool {
        draft.bindings.contains { $0.rotation != rotation } || draft.joysticks.contains { $0.rotation != rotation }
    }
    func hasBindings(for rotation: Int) -> Bool {
        draft.bindings.contains { $0.rotation == rotation } || draft.joysticks.contains { $0.rotation == rotation }
    }
    func rotationChanged(to next: Int? = nil) {
        traceTarget = nil
        if let next, (0...3).contains(next) { currentRotation = next }
        pendingPoint = nil
        selectedKey = nil
        selectedJoystickIndex = nil
        onChange?()
    }
    func save() throws {
        guard editing, !pending, draft.validate(), let location else { throw KeyMappingFailure.bindingRequired.nsError }
        let current: Data?
        do { current = try Data(contentsOf: location) }
        catch let e as NSError where e.domain == NSCocoaErrorDomain && e.code == NSFileReadNoSuchFileError { current = nil }
        guard current == baseline else { throw KeyMappingFailure.conflict.nsError }
        let bytes = try writeScheme(draft, to: location)
        resettingCorrupted = false
        baseline = bytes; scheme = draft; enabled = !scheme.isEmpty; placementMode = .tap; pendingPoint = nil; selectedKey = nil; selectedJoystickIndex = nil; editing = false
    }
    var visibleBindings: [MappingBinding] { editing ? draft.bindings : (hints && enabled ? scheme.bindings : []) }
    var visibleJoysticks: [JoystickBinding] { editing ? draft.joysticks : (hints && enabled ? scheme.joysticks : []) }

    private func loadFile(at url: URL) {
        location = url
        do {
            let data = try Data(contentsOf: url)
            let value = try JSONDecoder().decode(MappingScheme.self, from: data)
            guard value.validate() else { throw CocoaError(.fileReadCorruptFile) }
            scheme = value; baseline = data; enabled = !value.isEmpty; failure = nil
        } catch let e as NSError where e.domain == NSCocoaErrorDomain && e.code == NSFileReadNoSuchFileError {
            scheme = MappingScheme(); baseline = nil; enabled = false; failure = nil
        } catch {
            failure = .unreadable
        }
    }

    private func writeScheme(_ value: MappingScheme, to url: URL) throws -> Data {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let bytes = try JSONEncoder().encode(value)
        try bytes.write(to: url, options: .atomic)
        return bytes
    }

    private var profilesDirectory: URL? {
        guard let baseLocation else { return nil }
        let stem = baseLocation.deletingPathExtension().lastPathComponent
        return baseLocation.deletingLastPathComponent().appendingPathComponent(stem, isDirectory: true)
    }

    private var activeStateURL: URL? {
        baseLocation?.deletingPathExtension().appendingPathExtension("active")
    }

    private func profileURL(for name: String) -> URL? {
        profilesDirectory?.appendingPathComponent("\(name).json")
    }

    private func readActiveProfileName() -> String? {
        guard let activeStateURL,
              let raw = try? String(contentsOf: activeStateURL, encoding: .utf8) else { return nil }
        return Self.sanitizedProfileName(raw)
    }

    private func persistActiveProfileName(_ name: String?) {
        guard let activeStateURL else { return }
        if let name {
            try? FileManager.default.createDirectory(at: activeStateURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? name.write(to: activeStateURL, atomically: true, encoding: .utf8)
        } else {
            try? FileManager.default.removeItem(at: activeStateURL)
        }
    }

    private func refreshProfiles() {
        guard let dir = profilesDirectory,
              let entries = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else {
            profiles = []
            return
        }
        profiles = entries
            .filter { $0.pathExtension == "json" }
            .compactMap { Self.sanitizedProfileName($0.deletingPathExtension().lastPathComponent) }
            .sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }
    }

    static func sanitizedProfileName(_ raw: String) -> String? {
        let name = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name.count <= 40, !name.hasPrefix(".") else { return nil }
        let forbidden = CharacterSet(charactersIn: "/\\:\0")
        guard name.rangeOfCharacter(from: forbidden) == nil else { return nil }
        let lower = name.lowercased()
        if lower == "default" || name == "默认" || name == "默认方案" { return nil }
        return name
    }

    private static func isValidPoint(x: Double, y: Double, rotation: Int) -> Bool {
        x.isFinite && y.isFinite && (0...1).contains(x) && (0...1).contains(y) && (0...3).contains(rotation)
    }
}
