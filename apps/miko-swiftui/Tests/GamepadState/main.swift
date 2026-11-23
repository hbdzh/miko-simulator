// SPDX-License-Identifier: GPL-3.0-only
import Foundation

precondition(GamepadState.connecting.receivingProgress("") == .connecting)
precondition(GamepadState.connecting.receivingProgress("job-123") == .connecting)
precondition(GamepadState.connecting.receivingProgress("starting") == .connecting)
precondition(GamepadState.connecting.receivingProgress("miko.gamepad.ready") == .connected)
precondition(GamepadState.disconnected.receivingProgress("miko.gamepad.ready") == .disconnected)
precondition(GamepadState.failed.receivingProgress("miko.gamepad.ready") == .failed)
precondition(GamepadState.connected.receivingProgress("miko.gamepad.disconnected") == .disconnected)
precondition(GamepadState.disconnected.receivingProgress("miko.gamepad.reconnecting") == .connecting)
precondition(GamepadState.disconnected.receivingProgress("miko.gamepad.ready", recovering: true) == .connected)
precondition(GamepadState.connecting.receivingProgress("miko.gamepad.ready") == .connected)
let parsed = GamepadController.parseList(["controllers": [["id": 4, "name": "Pad", "bound_device": "Phone", "controls": [["name": "South", "value": 1.0]]]]])
precondition(parsed?.first?.boundDevice == "Phone")
precondition(parsed?.first?.controls.first?.value == 1.0)
precondition(GamepadController.parseList(["controllers": []])?.isEmpty == true)
precondition(GamepadController.parseList([:]) == nil)
let suiteName = "miko.gamepad.tests." + UUID().uuidString
let defaults = UserDefaults(suiteName: suiteName)!
let controller = parsed!.first!
var tuned = GamepadTuning()
tuned.left.deadzone = 0.3
tuned.right.invertX = true
GamepadTuningStore.save(tuned, controller: controller, package: "com.example.game", defaults: defaults)
precondition(GamepadTuningStore.load(controller: controller, package: "com.example.game", defaults: defaults) == tuned)
precondition(GamepadTuningStore.load(controller: controller, package: "com.other.game", defaults: defaults) == GamepadTuning())
var modelDefault = GamepadTuning()
modelDefault.triggerThreshold = 0.2
GamepadTuningStore.save(modelDefault, controller: controller, package: nil, defaults: defaults)
precondition(GamepadTuningStore.load(controller: controller, package: "com.other.game", defaults: defaults) == modelDefault)
precondition(GamepadTuningStore.load(controller: controller, package: "com.example.game", defaults: defaults) == tuned)
defaults.removePersistentDomain(forName: suiteName)
print("Gamepad state tests passed")
