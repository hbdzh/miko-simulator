// SPDX-License-Identifier: GPL-3.0-only
import Combine
import Foundation

struct DeviceGroup: Codable, Identifiable, Equatable {
    let id: String
    var name: String
    var devices: Set<String>
}

/// GUI-only metadata in the miko data directory; device/AVD files are untouched.
final class DeviceGroups: ObservableObject {
    private struct Document: Codable {
        var version: Int = 1
        var groups: [DeviceGroup] = []
    }
    @Published private(set) var groups: [DeviceGroup] = []
    @Published private(set) var error: String?
    private var file: URL?

    func load(dataDirectory: String) {
        let url = URL(fileURLWithPath: dataDirectory, isDirectory: true).appendingPathComponent("device-groups.json")
        if file == url { return }
        file = url; groups = []; error = nil
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        do {
            let document = try JSONDecoder().decode(Document.self, from: Data(contentsOf: url))
            guard document.version == 1 else { throw GroupError("Unsupported device group version") }
            guard Set(document.groups.map(\.id)).count == document.groups.count,
                  document.groups.allSatisfy({ !$0.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }) else {
                throw GroupError("Invalid device groups")
            }
            groups = document.groups
        } catch { self.error = error.localizedDescription }
    }

    func create(name: String, devices: Set<String>) throws {
        let clean = try validated(name: name, excluding: nil)
        var next = groups
        next.append(DeviceGroup(id: UUID().uuidString, name: clean, devices: devices))
        try save(next)
    }

    func rename(id: String, name: String) throws {
        let clean = try validated(name: name, excluding: id)
        var next = groups
        guard let index = next.firstIndex(where: { $0.id == id }) else { throw GroupError("Group not found") }
        next[index].name = clean
        try save(next)
    }

    func update(id: String, devices: Set<String>, add: Bool) throws {
        var next = groups
        guard let index = next.firstIndex(where: { $0.id == id }) else { throw GroupError("Group not found") }
        if add { next[index].devices.formUnion(devices) }
        else { next[index].devices.subtract(devices) }
        try save(next)
    }

    func delete(id: String) throws { try save(groups.filter { $0.id != id }) }

    private func validated(name: String, excluding id: String?) throws -> String {
        let clean = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty && clean.count <= 80 else { throw GroupError("Group name must contain 1–80 characters") }
        guard !groups.contains(where: { $0.id != id && $0.name.caseInsensitiveCompare(clean) == .orderedSame }) else {
            throw GroupError("A group with this name already exists")
        }
        return clean
    }

    private func save(_ next: [DeviceGroup]) throws {
        guard error == nil else { throw GroupError("Existing device groups could not be read; they were not overwritten") }
        guard let file else { throw GroupError("Device data directory is not available") }
        do {
            try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
            let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(Document(groups: next)).write(to: file, options: .atomic)
            groups = next
        } catch { throw GroupError(error.localizedDescription) }
    }
}

private struct GroupError: LocalizedError {
    let errorDescription: String?
    init(_ message: String) { errorDescription = message }
}
