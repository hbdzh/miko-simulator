// SPDX-License-Identifier: GPL-3.0-only

/// Local camera edits are compared with the last acknowledged device value.
struct DeviceCameraDraft: Equatable {
    var back: String
    var front: String
    var backFile: String = ""
    var frontFile: String = ""

    static func decode(back: String, front: String) -> DeviceCameraDraft {
        func source(_ value: String) -> (String, String) {
            for (prefix, kind) in [("imagefile:", "image"), ("videofile:", "video")] {
                if value.hasPrefix(prefix) { return (kind, String(value.dropFirst(prefix.count))) }
            }
            return (value, "")
        }
        let b = source(back), f = source(front)
        return DeviceCameraDraft(back: b.0, front: f.0, backFile: b.1, frontFile: f.1)
    }
}

struct DeviceCameraEdits {
    var draft = DeviceCameraDraft(back: "default", front: "default")
    private(set) var saved: DeviceCameraDraft?
    var hasChanges: Bool { saved != nil && draft != saved }

    mutating func load(_ value: DeviceCameraDraft) { saved = value; draft = value }
    mutating func revert() { if let saved { draft = saved } }
    mutating func acknowledge(_ value: DeviceCameraDraft, submitted: DeviceCameraDraft) {
        saved = value
        if draft == submitted { draft = value }
    }
}
