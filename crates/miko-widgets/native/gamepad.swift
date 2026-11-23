// SPDX-License-Identifier: Apache-2.0
import Foundation
import GameController
import CoreHaptics
import IOKit

// The daemon owns this process; all controller callbacks and commands run on
// the main queue so a physical device has one input and feedback owner.
struct HIDIdentity: Hashable {
    let name: String
    let manufacturer: String
    let vendor: Int
    let product: Int

    func matches(_ other: HIDIdentity) -> Bool { vendor == other.vendor && product == other.product }
}

final class Device {
    let id: Int
    let controller: GCController
    var previous: [String: Float] = [:]
    var identity: HIDIdentity?
    var name: String { controller.vendorName ?? controller.productCategory }
    var engine: CHHapticEngine?
    var player: (any CHHapticPatternPlayer)?
    init(_ id: Int, _ controller: GCController) { self.id = id; self.controller = controller }
    func values() -> [String: Float] {
        guard let p = controller.extendedGamepad else { return [:] }
        var v: [String: Float] = [
            "South": p.buttonA.value, "East": p.buttonB.value, "West": p.buttonX.value, "North": p.buttonY.value,
            "LeftTrigger": p.leftShoulder.value, "RightTrigger": p.rightShoulder.value,
            "LeftTrigger2": p.leftTrigger.value, "RightTrigger2": p.rightTrigger.value,
            "LeftStickX": p.leftThumbstick.xAxis.value, "LeftStickY": p.leftThumbstick.yAxis.value,
            "RightStickX": p.rightThumbstick.xAxis.value, "RightStickY": p.rightThumbstick.yAxis.value,
            "DPadUp": p.dpad.up.value, "DPadDown": p.dpad.down.value,
            "DPadLeft": p.dpad.left.value, "DPadRight": p.dpad.right.value,
            "Start": p.buttonMenu.value,
        ]
        if let b = p.buttonOptions { v["Select"] = b.value }
        if let b = p.buttonHome { v["Mode"] = b.value }
        if let b = p.leftThumbstickButton { v["LeftThumb"] = b.value }
        if let b = p.rightThumbstickButton { v["RightThumb"] = b.value }
        return v
    }
    func power() -> [String: Any] {
        guard let battery = controller.battery else { return ["state":"unknown"] }
        if battery.batteryState == .full { return ["state":"charged"] }
        guard battery.batteryLevel.isFinite, (0...1).contains(battery.batteryLevel) else { return ["state":"unknown"] }
        let percent = Int((battery.batteryLevel * 100).rounded())
        switch battery.batteryState {
        case .charging: return ["state":"charging", "percent":percent]
        case .discharging: return ["state":"discharging", "percent":percent]
        default: return ["state":"unknown"]
        }
    }
    func vibrate(_ active: Bool) throws {
        if !active { try player?.stop(atTime: CHHapticTimeImmediate); player = nil; return }
        try player?.stop(atTime: CHHapticTimeImmediate)
        player = nil
        guard let haptics = controller.haptics else { throw NSError(domain:"miko.gamepad", code:1) }
        let engine = self.engine ?? haptics.createEngine(withLocality: .default)
        guard let engine else { throw NSError(domain:"miko.gamepad", code:2) }
        self.engine = engine
        try engine.start()
        let event = CHHapticEvent(eventType:.hapticContinuous,
            parameters:[CHHapticEventParameter(parameterID:.hapticIntensity, value:0.7)],
            relativeTime:0, duration:10)
        let pattern = try CHHapticPattern(events:[event], parameters:[])
        let player = try engine.makePlayer(with:pattern)
        try player.start(atTime:CHHapticTimeImmediate)
        self.player = player
    }
}
var devices: [ObjectIdentifier: Device] = [:]
var nextID = 0
var lastIdentityScan = TimeInterval.zero

func normalizedName(_ name: String) -> String {
    name.lowercased().filter { $0.isLetter || $0.isNumber }
}

func hidIdentities() -> Set<HIDIdentity> {
    var iterator: io_iterator_t = 0
    guard IOServiceGetMatchingServices(kIOMainPortDefault, IOServiceMatching("IOHIDDevice"), &iterator) == KERN_SUCCESS else { return [] }
    defer { IOObjectRelease(iterator) }
    var found = Set<HIDIdentity>()
    while case let service = IOIteratorNext(iterator), service != 0 {
        defer { IOObjectRelease(service) }
        guard let hid = IOHIDDeviceCreate(kCFAllocatorDefault, service),
              let name = IOHIDDeviceGetProperty(hid, "Product" as CFString) as? String,
              let page = IOHIDDeviceGetProperty(hid, "PrimaryUsagePage" as CFString) as? Int, page == 1,
              let usage = IOHIDDeviceGetProperty(hid, "PrimaryUsage" as CFString) as? Int, [4, 5, 8].contains(usage),
              let vendor = IOHIDDeviceGetProperty(hid, "VendorID" as CFString) as? Int,
              let product = IOHIDDeviceGetProperty(hid, "ProductID" as CFString) as? Int,
              (1...65535).contains(vendor), (1...65535).contains(product),
              GCController.supportsHIDDevice(hid) else { continue }
        let manufacturer = IOHIDDeviceGetProperty(hid, "Manufacturer" as CFString) as? String ?? ""
        found.insert(HIDIdentity(name: normalizedName(name), manufacturer: normalizedName(manufacturer), vendor: vendor, product: product))
    }
    return found
}

func resolveIdentities() {
    let unresolved = devices.values.filter { $0.identity == nil }
    guard !unresolved.isEmpty else { return }
    lastIdentityScan = ProcessInfo.processInfo.systemUptime
    let identities = hidIdentities()
    let known = devices.values.compactMap(\.identity)
    let candidates = unresolved.map { device in
        let names = [normalizedName(device.name), normalizedName(device.controller.productCategory)]
        return (device, identities.filter { names.contains($0.name) || names.contains($0.manufacturer + $0.name) })
    }
    for (device, matches) in candidates {
        if matches.count == 1, let match = matches.first,
           !known.contains(where: { $0.matches(match) }),
           !candidates.contains(where: { $0.0.id != device.id && $0.1.contains { $0.matches(match) } }) {
            device.identity = match
        }
    }
    // A custom controller name may have no HID-name match. Use the remaining
    // model only when both inventories contain a single controller.
    if devices.count == 1, identities.count == 1 {
        unresolved.first?.identity = identities.first
    }
}
func send(_ value: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject:value) else { return }
    FileHandle.standardOutput.write(data + Data([10]))
}
func emitChanges(_ device: Device) {
    let now = device.values()
    for (name, value) in now where device.previous[name] != value {
        send(["type":"event", "id":device.id, "control":name, "value":value])
    }
    device.previous = now
}
func refresh() {
    let connected = GCController.controllers()
    let active = Set(connected.map(ObjectIdentifier.init))
    let previous = devices
    let disconnected = devices.filter { !active.contains($0.key) }
    for key in disconnected.keys { devices.removeValue(forKey: key) }
    for controller in connected {
        let key = ObjectIdentifier(controller)
        guard devices[key] == nil else { continue }
        let device = Device(nextID, controller); nextID += 1
        devices[key] = device
        controller.handlerQueue = .main
        controller.extendedGamepad?.valueChangedHandler = { [weak device] _, _ in
            guard let device else { return }
            emitChanges(device)
        }
        device.previous = device.values()
    }
    if active != Set(previous.keys) || ProcessInfo.processInfo.systemUptime - lastIdentityScan >= 1 {
        resolveIdentities()
    }
    for device in disconnected.values {
        try? device.vibrate(false)
        var event: [String: Any] = ["type":"disconnected", "id":device.id, "ambiguous":true]
        if let identity = device.identity {
            event["vendor"] = identity.vendor
            event["product"] = identity.product
            event["ambiguous"] = (Array(previous.values) + Array(devices.values)).contains { other in
                guard other.id != device.id else { return false }
                if let peer = other.identity { return identity.matches(peer) }
                return normalizedName(other.name) == normalizedName(device.name)
                    || other.controller.productCategory == device.controller.productCategory
            }
        }
        send(event)
    }
}
func list() {
    refresh()
    let list: [[String: Any]] = devices.values.sorted { $0.id < $1.id }.map { d in
        var result: [String: Any] = ["id":d.id, "name":d.name,
            "power":d.power(), "controls":d.values().map { ["name":$0.key, "value":$0.value] }]
        if let identity = d.identity {
            result["vendor"] = identity.vendor
            result["product"] = identity.product
        }
        return result
    }
    send(["type":"controllers", "controllers":list])
}
DispatchQueue.global(qos:.userInteractive).async {
    while let line = readLine(), let data = line.data(using:.utf8),
          let command = (try? JSONSerialization.jsonObject(with:data)) as? [String: Any] {
        DispatchQueue.main.async {
            switch command["type"] as? String {
            case "list": list()
            case "vibrate":
                if let id = command["id"] as? Int, let active = command["active"] as? Bool,
                   let device = devices.values.first(where: { $0.id == id }) {
                    do { try device.vibrate(active); send(["type":"vibration", "id":id, "ok":true]) }
                    catch { send(["type":"vibration", "id":id, "ok":false, "message":error.localizedDescription]) }
                }
            default: break
            }
        }
    }
    DispatchQueue.main.async { for device in devices.values { try? device.vibrate(false) }; exit(0) }
}
let timer = Timer(timeInterval:0.25, repeats:true) { _ in refresh() }
RunLoop.main.add(timer, forMode:.common)
refresh()
send(["type":"ready"])
RunLoop.main.run()
