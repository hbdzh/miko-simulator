// SPDX-License-Identifier: GPL-3.0-only
import Foundation

enum GamepadInputMode: String, CaseIterable {
    case native, touch
}

struct GamepadControl: Equatable, Identifiable {
    let name: String
    let value: Double
    var id: String { name }
}

struct GamepadController: Equatable, Identifiable {
    let id: Int
    let name: String
    let vendor: Int?
    let product: Int?
    let boundDevice: String?
    let powerState: String
    let powerPercent: Int?
    let controls: [GamepadControl]

    static func parseList(_ value: Any) -> [GamepadController]? {
        guard let root = value as? [String: Any], let list = root["controllers"] as? [[String: Any]] else { return nil }
        return list.compactMap { item in
            guard let id = item["id"] as? Int, let name = item["name"] as? String else { return nil }
            let controls = (item["controls"] as? [[String: Any]] ?? []).compactMap { control -> GamepadControl? in
                guard let name = control["name"] as? String, let value = control["value"] as? Double else { return nil }
                return GamepadControl(name: name, value: value)
            }
            let power = item["power"] as? [String: Any]
            let percent = power?["percent"] as? Int
            return GamepadController(id: id, name: name, vendor: item["vendor"] as? Int, product: item["product"] as? Int,
                                     boundDevice: item["bound_device"] as? String,
                                     powerState: power?["state"] as? String ?? "unknown",
                                     powerPercent: percent.flatMap { (0...100).contains($0) ? $0 : nil }, controls: controls)
        }
    }
}

struct GamepadStickTuning: Codable, Equatable {
    var deadzone = 0.0
    var sensitivity = 1.0
    var invertX = false
    var invertY = false

    var parameters: [String: Any] {
        ["deadzone": deadzone, "sensitivity": sensitivity, "invert_x": invertX, "invert_y": invertY]
    }
}

struct GamepadTuning: Codable, Equatable {
    var left = GamepadStickTuning()
    var right = GamepadStickTuning()
    var triggerThreshold = 0.0

    var parameters: [String: Any] {
        ["left": left.parameters, "right": right.parameters, "trigger_threshold": triggerThreshold]
    }
}

enum GamepadTuningStore {
    static func key(controller: GamepadController, package: String?) -> String {
        let identity = [controller.name, controller.vendor.map(String.init) ?? "", controller.product.map(String.init) ?? "", package ?? ""]
        let encoded = (try? JSONEncoder().encode(identity))?.base64EncodedString() ?? ""
        return "miko.gamepad.tuning." + encoded
    }

    static func load(controller: GamepadController, package: String?, defaults: UserDefaults = .standard) -> GamepadTuning {
        func read(_ package: String?) -> GamepadTuning? {
            guard let data = defaults.data(forKey: key(controller: controller, package: package)),
                  let tuning = try? JSONDecoder().decode(GamepadTuning.self, from: data), valid(tuning) else { return nil }
            return tuning
        }
        return read(package) ?? (package == nil ? nil : read(nil)) ?? GamepadTuning()
    }

    static func save(_ tuning: GamepadTuning, controller: GamepadController, package: String?, defaults: UserDefaults = .standard) {
        guard valid(tuning), let data = try? JSONEncoder().encode(tuning) else { return }
        defaults.set(data, forKey: key(controller: controller, package: package))
    }

    private static func valid(_ tuning: GamepadTuning) -> Bool {
        [tuning.left, tuning.right].allSatisfy { $0.deadzone.isFinite && (0...0.95).contains($0.deadzone)
            && $0.sensitivity.isFinite && (0.1...5).contains($0.sensitivity) }
            && tuning.triggerThreshold.isFinite && (0...0.95).contains(tuning.triggerThreshold)
    }
}

enum GamepadState: Equatable {
    case disconnected, connecting, connected, failed

    /// A job identifier and ordinary progress do not confirm Android readiness.
    func receivingProgress(_ message: String, recovering: Bool = false) -> GamepadState {
        switch message {
        case "miko.gamepad.ready" where self == .connecting: return .connected
        case "miko.gamepad.ready" where self == .disconnected && recovering: return .connected
        case "miko.gamepad.disconnected" where self == .connected || self == .connecting: return .disconnected
        case "miko.gamepad.reconnecting" where self == .disconnected: return .connecting
        default: return self
        }
    }
}
