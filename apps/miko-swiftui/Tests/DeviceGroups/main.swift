// SPDX-License-Identifier: GPL-3.0-only
import Foundation
import CoreGraphics

let root = FileManager.default.temporaryDirectory.appendingPathComponent("miko-groups-\(UUID().uuidString)")
try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: root) }
let groups = DeviceGroups(); groups.load(dataDirectory: root.path)
try groups.create(name: "Game", devices: ["a", "b"])
let id = groups.groups[0].id
do { try groups.create(name: "game", devices: []); fatalError("duplicate accepted") } catch { }
try groups.update(id: id, devices: ["b", "c"], add: false)
try groups.update(id: id, devices: ["c"], add: true)
try groups.rename(id: id, name: "Work")
let reloaded = DeviceGroups(); reloaded.load(dataDirectory: root.path)
precondition(reloaded.groups == [DeviceGroup(id: id, name: "Work", devices: ["a", "c"])])
let file = root.appendingPathComponent("device-groups.json")
let future = Data(#"{"version":99,"groups":[]}"#.utf8)
try future.write(to: file)
let unsupported = DeviceGroups(); unsupported.load(dataDirectory: root.path)
precondition(unsupported.error != nil)
do { try unsupported.create(name: "No", devices: []); fatalError("future data overwritten") } catch { }
let preserved = try Data(contentsOf: file)
precondition(preserved == future)

let specs = Array(repeating: DeviceWindowLayoutSpec(aspect: 0.6, side: 50, decorations: 30, minimumHeight: 180), count: 4)
for screen in [CGRect(x: -1600, y: 0, width: 1600, height: 900), CGRect(x: 0, y: -200, width: 1440, height: 900)] {
    let frames = DeviceWindowLayout.frames(specs: specs, visible: screen)!
    precondition(frames.count == 4 && frames.allSatisfy { screen.contains($0) })
    for a in frames.indices { for b in frames.indices where a < b { precondition(!frames[a].intersects(frames[b])) } }
}
precondition(DeviceWindowLayout.frames(specs: specs, visible: CGRect(x: 0, y: 0, width: 100, height: 100)) == nil)
print("Device group and window layout tests passed")
