// SPDX-License-Identifier: GPL-3.0-only
import AppKit
import ApplicationServices
import Combine
import Foundation

/// App-wide monitor; the device must have a player bridge open to receive playback.
final class GlobalScriptShortcuts: ObservableObject {
    static let shared = GlobalScriptShortcuts()
    @Published private(set) var bindings = GlobalScriptBindings()
    private var path: URL?
    private var readable = false
    private var handlers: [String:(String)->Void] = [:]
    private var globalMonitor: Any?
    private var localMonitor: Any?
    private var activationObserver: NSObjectProtocol?
    var permissionGranted: Bool { AXIsProcessTrusted() }

    func configure(dataDirectory: String) {
        let url = URL(fileURLWithPath:dataDirectory, isDirectory:true).appendingPathComponent("global-script-shortcuts.json")
        guard path != url else { return }
        path = url; bindings = GlobalScriptBindings(); readable = false
        if !FileManager.default.fileExists(atPath:url.path) { readable = true }
        else if let data = try? Data(contentsOf:url), let saved = try? JSONDecoder().decode(GlobalScriptBindings.self, from:data), saved.validate() {
            bindings = saved; readable = true
        }
        installMonitors()
        if activationObserver == nil {
            activationObserver = NotificationCenter.default.addObserver(forName:NSApplication.didBecomeActiveNotification, object:nil, queue:.main) { [weak self] _ in self?.installMonitors() }
        }
    }
    func register(device: String, handler: @escaping (String)->Void) { handlers[device.lowercased()] = handler }
    func unregister(device: String) { handlers.removeValue(forKey:device.lowercased()) }
    func set(slot: Int?, device: String, script: String) -> Bool {
        guard readable, let path else { return false }
        let target = GlobalScriptTarget(device:device.lowercased(), script:script.lowercased())
        var next = bindings
        guard next.set(slot,target:target) else { return false }
        do {
            try FileManager.default.createDirectory(at:path.deletingLastPathComponent(),withIntermediateDirectories:true)
            try JSONEncoder().encode(next).write(to:path,options:.atomic)
            bindings = next
            if slot != nil && !AXIsProcessTrusted() {
                let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
                _ = AXIsProcessTrustedWithOptions(options)
            }
            return true
        } catch { return false }
    }
    private func installMonitors() {
        if globalMonitor == nil {
            globalMonitor = NSEvent.addGlobalMonitorForEvents(matching:.keyDown) { [weak self] event in
                DispatchQueue.main.async { self?.handle(event) }
            }
        }
        if localMonitor == nil {
            localMonitor = NSEvent.addLocalMonitorForEvents(matching:.keyDown) { [weak self] event in
                guard let self, NSApp.modalWindow == nil else { return event }
                return self.handle(event) ? nil : event
            }
        }
    }
    @discardableResult func handle(_ event: NSEvent) -> Bool {
        guard NSApp.modalWindow == nil, NSApp.keyWindow?.attachedSheet == nil else { return false }
        let flags = event.modifierFlags.intersection([.command,.control,.option,.shift])
        guard flags == [.control,.option,.shift], !event.isARepeat,
              let key = GlobalScriptBindings.slot(keyCode:event.keyCode),
              let target = bindings.slots[key], let handler = handlers[target.device] else { return false }
        handler(target.script)
        return true
    }
}
