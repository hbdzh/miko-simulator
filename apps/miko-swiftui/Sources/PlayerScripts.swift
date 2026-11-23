// SPDX-License-Identifier: GPL-3.0-only
import AppKit
import Combine
import Foundation
import SwiftUI
import UniformTypeIdentifiers

// MARK: - Script models (bridge JSON, docs/player-scripts.md)

/// One script in the player's list (`ScriptSummary` on the bridge side).
struct PlayerScriptSummary: Identifiable, Codable, Equatable {
    var id: String
    var name: String
    var duration_ms: Int
    var event_count: Int
    var action_count: Int?
    var created_at: String
    var display: PlayerScriptDisplay?

    var duration: TimeInterval { Double(max(0, duration_ms)) / 1000 }

    /// Decodes a summary object from the bridge's JSON notification.
    static func decode(_ value: Any?) -> PlayerScriptSummary? {
        guard let object = value as? [String: Any],
              let data = try? JSONSerialization.data(withJSONObject: object) else { return nil }
        return try? JSONDecoder().decode(PlayerScriptSummary.self, from: data)
    }
}

struct PlayerScriptDisplay: Codable, Equatable {
    var width: Int
    var height: Int
}

struct PlayerScriptActionRow: Decodable, Identifiable {
    struct Point: Decodable { let x: Double; let y: Double }
    let index: Int
    let kind: String
    let detail: String
    let start_ms: Int
    let end_ms: Int
    let wait_ms: Int?
    let point: Point?
    let endpoint: Point?
    var id: Int { index }

    func title(language: Int) -> String {
        switch kind {
        case "gesture": return detail == "tap" ? t("点击", "Tap", language) : t("滑动或手势", "Swipe or gesture", language)
        case "key": return t("按键：", "Key: ", language) + detail
        case "text": return t("输入文本", "Enter text", language)
        case "paste": return t("粘贴文本", "Paste text", language)
        case "tool_key": return t("设备按键：", "Device key: ", language) + detail
        case "script_call": return t("调用脚本：", "Run script: ", language) + detail
        default: return t("未知动作", "Unknown action", language)
        }
    }
}

/// Playback defaults for one run (`PlaybackOptions` on the bridge side).
/// `mode` is `count`, `duration` or `infinite`; irrelevant fields are sent as 0,
/// which is what the v1 contract defines.
enum PlaybackMode: String, Codable, CaseIterable {
    case count
    case duration
    case infinite
}

struct PlaybackOptions: Codable, Equatable {
    var mode: PlaybackMode = .count
    var count: Int = 1
    var duration_ms: Int = 0
    var interval_ms: Int = 0
    var speed: Double = 1

    var jsonObject: [String: Any] {
        ["mode": mode.rawValue,
         "count": max(0, count),
         "duration_ms": max(0, duration_ms),
         "interval_ms": max(0, interval_ms),
         "speed": speed]
    }
}

// MARK: - Formatting and copy

enum PlayerScriptFormat {
    static func duration(_ milliseconds: Int) -> String {
        let total = max(0, milliseconds) / 1000
        let hours = total / 3600
        let minutes = (total % 3600) / 60
        let seconds = total % 60
        if hours > 0 { return String(format: "%d:%02d:%02d", hours, minutes, seconds) }
        return String(format: "%d:%02d", minutes, seconds)
    }

    static func elapsed(_ interval: TimeInterval) -> String {
        duration(Int(max(0, interval) * 1000))
    }

    static func eventCount(_ count: Int) -> String {
        String(format: "%d", count)
    }

    static func createdAt(_ iso: String) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        var date = formatter.date(from: iso)
        if date == nil {
            formatter.formatOptions = [.withInternetDateTime]
            date = formatter.date(from: iso)
        }
        guard let date else { return iso }
        let display = DateFormatter()
        display.dateStyle = .short
        display.timeStyle = .short
        return display.string(from: date)
    }

    static func resolution(_ display: PlayerScriptDisplay?, language: Int) -> String {
        guard let display, display.width > 0, display.height > 0 else {
            return PlayerScriptText.unknown(language: language)
        }
        return "\(display.width)×\(display.height)"
    }

    static func suggestedRecordingName(language: Int) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd HH.mm"
        return t("操作录制", "Operation recording", language) + " " + formatter.string(from: Date())
    }
}

/// Player-script copy in both languages; the bridge sends stable codes plus an
/// English fallback, so common codes get a bilingual label here.
enum PlayerScriptText {
    static func recordingState(_ state: PlayerTransport.ScriptRecordingState, language: Int) -> String {
        switch state {
        case .idle: return t("未在录制", "Not recording", language)
        case .recording: return t("正在录制操作", "Recording actions", language)
        case .paused: return t("录制已暂停", "Recording paused", language)
        }
    }

    static func playbackState(_ state: PlayerTransport.ScriptPlaybackState, language: Int) -> String {
        switch state {
        case .idle: return t("未在回放", "Not playing", language)
        case .playing: return t("正在回放操作", "Playing actions", language)
        case .paused: return t("回放已暂停", "Playback paused", language)
        }
    }

    static func loop(loop: Int, total: Int, mode: PlaybackMode, language: Int) -> String {
        if total > 0 {
            return String(format: t("第 %d/%d 轮", "Round %d/%d", language), max(1, loop), total)
        }
        if mode == .infinite {
            return t("无限循环", "Looping", language) + " · " + String(format: t("第 %d 轮", "Round %d", language), max(1, loop))
        }
        return String(format: t("第 %d 轮", "Round %d", language), max(1, loop))
    }

    static func mode(_ mode: PlaybackMode, language: Int) -> String {
        switch mode {
        case .count: return t("按次数", "By count", language)
        case .duration: return t("按时长", "By duration", language)
        case .infinite: return t("无限循环", "Loop forever", language)
        }
    }

    static func unknown(language: Int) -> String {
        t("未知", "Unknown", language)
    }

    /// Maps the bridge's error codes to bilingual copy; unknown messages pass
    /// through (they are already the daemon/bridge English fallback).
    static func error(code: String?, message: String?, language: Int) -> String {
        switch message {
        case "The script changed elsewhere; reopen it before editing", "The open script changed; reopen it before editing":
            return t("录制已发生变化，请重新读取后再编辑。", "The recording changed. Reload it before editing.", language)
        case "cannot change a wait inside overlapping actions":
            return t("无法修改重叠动作期间的等待。", "Cannot change a wait inside overlapping actions.", language)
        case "the recording contains no events":
            return t("不能删除最后一个动作。", "The last action cannot be deleted.", language)
        default:
            break
        }
        switch code {
        case "script.busy":
            return t("操作录制正忙，请先停止当前录制或回放。",
                     "An operation recording is busy. Stop the current recording or playback first.",
                     language)
        case "script.not_found", "not_found":
            return t("找不到该操作录制。", "The operation recording was not found.", language)
        case "script.recording":
            return t("正在录制，无法开始回放。", "Recording is in progress; playback cannot start.", language)
        case "script.playing":
            return t("正在回放，无法开始录制。", "Playback is in progress; recording cannot start.", language)
        case "script.invalid", "invalid_params", "usage":
            if let message, !message.isEmpty {
                return t("操作录制的数据无效：", "The operation recording is invalid: ", language) + message
            }
            return t("操作录制的数据无效。", "The operation recording is invalid.", language)
        case "script.io":
            if let message, !message.isEmpty {
                return t("无法读写操作录制文件：", "Could not access the operation recording file: ", language) + message
            }
            return t("无法读写操作录制文件。", "Could not access the operation recording file.", language)
        case "script.not_recording":
            return t("当前没有正在进行的操作录制。", "No operation recording is running.", language)
        case "script.not_playing":
            return t("当前没有正在回放的操作录制。", "No operation recording playback is running.", language)
        case "script.failed":
            if let message, !message.isEmpty { return message }
            return t("操作录制失败，请重试。", "The operation recording failed. Try again.", language)
        case "script.full":
            return t("操作录制数量或时长超出限制。", "The operation recording exceeds the supported limit.", language)
        default:
            if let message, !message.isEmpty { return message }
            return t("操作录制失败，请重试。", "The operation recording failed. Try again.", language)
        }
    }
}

// MARK: - Native prompts (NSAlert / NSSavePanel)

enum PlayerScriptPrompts {
    static func askName(defaultValue: String, language: Int) -> String? {
        let alert = NSAlert()
        alert.messageText = t("保存操作录制", "Save operation recording", language)
        alert.informativeText = t("输入录制名称（1–80 个字符）。",
                                  "Enter a recording name (1–80 characters).", language)
        alert.addButton(withTitle: t("保存", "Save", language))
        alert.addButton(withTitle: t("取消", "Cancel", language))
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24))
        field.stringValue = defaultValue
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name.count <= 80 else { return nil }
        return name
    }

    static func askRename(current: String, language: Int) -> String? {
        let alert = NSAlert()
        alert.messageText = t("重命名操作录制", "Rename operation recording", language)
        alert.informativeText = t("输入新名称（1–80 个字符）。",
                                  "Enter a new name (1–80 characters).", language)
        alert.addButton(withTitle: t("重命名", "Rename", language))
        alert.addButton(withTitle: t("取消", "Cancel", language))
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24))
        field.stringValue = current
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name.count <= 80 else { return nil }
        return name
    }

    static func confirmCompatibility(_ script: PlayerScriptSummary, transport: PlayerTransport) -> Bool {
        guard let display = script.display, display.width > 0, display.height > 0,
              transport.frameWidth > 0, transport.frameHeight > 0,
              display.width != transport.frameWidth || display.height != transport.frameHeight else { return true }
        let alert = NSAlert()
        alert.messageText = t("设备分辨率不同", "Device resolution differs", transport.language)
        alert.informativeText = String(format: t("录制时为 %d×%d，当前设备为 %d×%d。坐标会按当前画面换算。", "Recorded at %d×%d; this device is %d×%d. Coordinates are mapped to the current display.", transport.language), display.width, display.height, transport.frameWidth, transport.frameHeight)
        alert.addButton(withTitle: t("回放", "Play", transport.language))
        alert.addButton(withTitle: t("取消", "Cancel", transport.language))
        return alert.runModal() == .alertFirstButtonReturn
    }

    static func confirmDelete(name: String, language: Int) -> Bool {
        let alert = NSAlert()
        alert.messageText = t("删除操作录制？", "Delete operation recording?", language)
        alert.informativeText = String(format: t("「%@」将被删除，无法撤销。",
                                                 "“%@” will be deleted. This cannot be undone.", language),
                                       name)
        alert.addButton(withTitle: t("删除", "Delete", language))
        alert.addButton(withTitle: t("取消", "Cancel", language))
        alert.alertStyle = .warning
        return alert.runModal() == .alertFirstButtonReturn
    }
}

// MARK: - Coordinator and window

/// Owns the non-modal operation-recording window for one player window.
final class PlayerScriptsCoordinator: ObservableObject {
    private var controller: PlayerScriptsWindowController?

    func present(transport: PlayerTransport, perform: ((PlayerScriptAction, @escaping (Bool) -> Void) -> Void)? = nil) {
        if let controller {
            controller.show()
            return
        }
        let controller = PlayerScriptsWindowController(transport: transport, perform: perform)
        self.controller = controller
        controller.show()
    }

    func close() {
        controller?.close()
        controller = nil
    }
}

enum PlayerScriptAction {
    case record
    case play(PlayerScriptSummary, PlaybackOptions)
}

/// A native toolbar keeps the window focused on the list of saved recordings.
final class PlayerScriptsWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate {
    private let transport: PlayerTransport
    private let perform: ((PlayerScriptAction, @escaping (Bool) -> Void) -> Void)?
    private var languageCancellable: AnyCancellable?
    private let toolbar = NSToolbar(identifier: "miko.recordings")

    init(transport: PlayerTransport, perform: ((PlayerScriptAction, @escaping (Bool) -> Void) -> Void)? = nil) {
        self.transport = transport
        self.perform = perform
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 740, height: 480),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable],
                              backing: .buffered, defer: false)
        window.title = t("操作录制", "Operation recordings", transport.language)
        window.minSize = NSSize(width: 540, height: 340)
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.styleMask.insert(.fullSizeContentView)
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.isOpaque = false
        window.backgroundColor = .clear
        super.init(window: window)
        window.delegate = self
        let background = NSVisualEffectView()
        background.material = .underWindowBackground
        background.blendingMode = .behindWindow
        background.state = .followsWindowActiveState
        let hosting = NSHostingView(rootView: PlayerScriptsView(transport: transport, perform: { [weak self] action in
            self?.performAction(action)
        }))
        window.contentView = background
        hosting.translatesAutoresizingMaskIntoConstraints = false
        background.addSubview(hosting)
        NSLayoutConstraint.activate([NSLayoutConstraint.Attribute.leading, .trailing, .top, .bottom].map {
            NSLayoutConstraint(item: hosting, attribute: $0, relatedBy: .equal,
                               toItem: window.contentLayoutGuide, attribute: $0,
                               multiplier: 1, constant: 0)
        })
        toolbar.delegate = self
        toolbar.displayMode = .iconAndLabel
        window.toolbar = toolbar
        window.toolbarStyle = .unifiedCompact
        window.center()
        languageCancellable = transport.$language.sink { [weak self] language in
            self?.window?.title = t("操作录制", "Operation recordings", language)
            self?.toolbar.items.forEach { item in
                switch item.itemIdentifier.rawValue {
                case "miko.recordings.new": item.label = t("新建录制", "New recording", language)
                case "miko.recordings.import": item.label = t("导入", "Import", language)
                default: return
                }
                item.paletteLabel = item.label
            }
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
    func show() {
        transport.scriptList()
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] { [.flexibleSpace, .init("miko.recordings.new"), .space, .init("miko.recordings.import")] }
    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] { toolbarAllowedItemIdentifiers(toolbar) }
    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier,
                 willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let item = NSToolbarItem(itemIdentifier: id)
        item.target = self
        switch id.rawValue {
        case "miko.recordings.new":
            item.label = t("新建录制", "New recording", transport.language)
            item.image = NSImage(systemSymbolName: "plus", accessibilityDescription: item.label)
            item.action = #selector(startRecording)
        case "miko.recordings.import":
            item.label = t("导入", "Import", transport.language)
            item.image = NSImage(systemSymbolName: "square.and.arrow.down", accessibilityDescription: item.label)
            item.action = #selector(importScript)
        default: return nil
        }
        item.paletteLabel = item.label
        return item
    }
    @objc private func startRecording() { performAction(.record) }
    @objc private func importScript() {
        guard transport.linkState.isRunning else { return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.json]
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        transport.scriptImport(path: url.path)
    }
    private func performAction(_ action: PlayerScriptAction) {
        let completed: (Bool) -> Void = { [weak self] success in
            if success { self?.window?.orderOut(nil) }
        }
        if let perform { perform(action, completed); return }
        guard transport.linkState.isRunning else {
            let alert = NSAlert()
            alert.messageText = t("请先启动设备", "Start a device first", transport.language)
            alert.runModal()
            return
        }
        switch action {
        case .record: transport.scriptRecordStart()
        case .play(let script, let options):
            guard PlayerScriptPrompts.confirmCompatibility(script, transport: transport) else { completed(false); return }
            transport.scriptPlayStart(id: script.id, options: options)
        }
        completed(true)
    }
}

struct PlayerScriptsView: View {
    @ObservedObject var transport: PlayerTransport
    var perform: ((PlayerScriptAction) -> Void)? = nil
    @State private var search = ""
    @State private var selection: String?
    @State private var optionsFor: String?
    @State private var editingScript: PlayerScriptSummary?
    @AppStorage("miko.script.playback.mode") private var playbackMode = PlaybackMode.count.rawValue
    @AppStorage("miko.script.playback.count") private var playbackCount = 1
    @AppStorage("miko.script.playback.seconds") private var playbackSeconds = 60
    @AppStorage("miko.script.playback.interval") private var playbackInterval = 0
    @AppStorage("miko.script.playback.speed") private var playbackSpeed = 1.0
    private var language: Int { transport.language }
    private var filteredScripts: [PlayerScriptSummary] {
        let query = search.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return query.isEmpty ? transport.scripts : transport.scripts.filter { $0.name.lowercased().contains(query) }
    }
    private var options: PlaybackOptions {
        let mode = PlaybackMode(rawValue: playbackMode) ?? .count
        var result = PlaybackOptions()
        result.mode = mode
        result.count = mode == .count ? min(max(1, playbackCount), 1000) : 0
        result.duration_ms = mode == .duration ? min(max(1, playbackSeconds), 86400) * 1000 : 0
        result.interval_ms = min(max(0, playbackInterval), 86400000)
        result.speed = playbackSpeed
        return result
    }
    var body: some View {
        VStack(spacing: 0) {
            if let error = transport.scriptError {
                HStack {
                    Label(error, systemImage: WbIcon.warning).foregroundStyle(WbColor.negative)
                    Spacer()
                    Button(t("关闭", "Dismiss", language)) { transport.clearScriptError() }
                }.padding(12)
            }
            NativeSearchField(text: $search, placeholder: t("搜索录制", "Search recordings", language), identifier: "recordings-search")
                .frame(height: 30)
                .padding(12)
            if filteredScripts.isEmpty {
                VStack(spacing: 8) {
                    Spacer()
                    Image(systemName: "list.bullet.rectangle").font(.system(size: 22)).foregroundStyle(.secondary)
                    Text(transport.scripts.isEmpty ? t("暂无操作录制", "No recordings yet", language) : t("没有匹配的录制", "No matching recordings", language))
                    if transport.scripts.isEmpty {
                        Text(t("点击“新建录制”开始。", "Choose New recording to get started.", language)).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(selection: $selection) {
                    ForEach(filteredScripts) { script in
                        row(script).tag(script.id)
                    }
                }.listStyle(.inset).scrollContentBackground(.hidden)
            }
            Divider()
            HStack {
                Text(String(format: t("共 %d 条录制", "%d recordings", language), filteredScripts.count))
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
            }.padding(.horizontal, 14).padding(.vertical, 9)
        }
        .onAppear { transport.scriptList() }
        .onReceive(NotificationCenter.default.publisher(for: .playerScriptsChanged)) { _ in
            if transport.linkState.isRunning { transport.scriptList() }
        }
        .sheet(item: $editingScript) { script in
            ScriptActionsEditor(transport: transport, script: script)
        }
    }
    private func row(_ script: PlayerScriptSummary) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "cursorarrow.motionlines").foregroundStyle(.secondary).frame(width: 20)
            VStack(alignment: .leading, spacing: 3) {
                Text(script.name).fontWeight(.medium).lineLimit(1)
                Text(PlayerScriptFormat.createdAt(script.created_at)).font(.caption).foregroundStyle(.secondary)
            }.frame(maxWidth: .infinity, alignment: .leading)
            Text(PlayerScriptFormat.duration(script.duration_ms)).monospacedDigit().frame(width: 54, alignment: .trailing)
            Group {
                if let count = script.action_count {
                    Text(String(format: t("%d 次操作", "%d actions", language), count))
                } else {
                    Text(String(format: t("%d 个事件", "%d events", language), script.event_count))
                }
            }
                .font(.caption).foregroundStyle(.secondary).frame(width: 80, alignment: .trailing)
            Button { perform?(.play(script, options)) } label: { Image(systemName: "play.fill") }
                .buttonStyle(.borderless).help(t("回放", "Play", language))
                .accessibilityLabel(t("回放", "Play", language) + " " + script.name)
            Button { optionsFor = script.id } label: { Image(systemName: "slider.horizontal.3") }
                .buttonStyle(.borderless).help(t("回放设置", "Playback settings", language))
                .accessibilityLabel(t("回放设置", "Playback settings", language) + " " + script.name)
                .popover(isPresented: Binding(get: { optionsFor == script.id }, set: { if !$0 { optionsFor = nil } })) {
                    PlaybackOptionsPopover(language: language, mode: $playbackMode, count: $playbackCount,
                                           seconds: $playbackSeconds, interval: $playbackInterval, speed: $playbackSpeed)
                }
            Menu {
                Button(t("编辑动作…", "Edit actions…", language)) { editingScript = script }
                ScriptShortcutMenu(store: transport.shortcuts, script: script.id, language: language, onError: { transport.onStatus?(t("快捷键已占用或无法保存。", "Shortcut is in use or could not be saved.", language)) })
                if let device = transport.device {
                    GlobalScriptShortcutMenu(store: .shared, device: device, script: script.id, language: language) { text in transport.onStatus?(text) }
                }
                Button(t("重命名…", "Rename…", language)) { rename(script) }
                Button(t("导出…", "Export…", language)) { exportScript(script) }
                Divider()
                Button(t("删除", "Delete", language), role: .destructive) { delete(script) }
            } label: { Image(systemName: "ellipsis") }
            .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
            .help(t("更多操作", "More actions", language))
        }
        .padding(.vertical, 5)
        .contextMenu {
            Button(t("回放", "Play", language)) { perform?(.play(script, options)) }
            Button(t("编辑动作…", "Edit actions…", language)) { editingScript = script }
            Button(t("重命名…", "Rename…", language)) { rename(script) }
            Button(t("导出…", "Export…", language)) { exportScript(script) }
            Divider()
            Button(t("删除", "Delete", language), role: .destructive) { delete(script) }
        }
    }
    private func rename(_ script: PlayerScriptSummary) {
        guard let name = PlayerScriptPrompts.askRename(current: script.name, language: language) else { return }
        transport.scriptRename(id: script.id, name: name)
    }
    private func delete(_ script: PlayerScriptSummary) {
        guard PlayerScriptPrompts.confirmDelete(name: script.name, language: language) else { return }
        transport.scriptDelete(id: script.id)
    }
    private func exportScript(_ script: PlayerScriptSummary) {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.json]
        panel.nameFieldStringValue = t("操作录制.json", "operation-recording.json", language)
        guard panel.runModal() == .OK, let url = panel.url else { return }
        transport.scriptExport(id: script.id, path: url.path)
    }

}

private struct ScriptShortcutMenu: View {
    @ObservedObject var store: GameShortcutsStore
    let script: String
    let language: Int
    let onError: () -> Void
    var body: some View {
        Menu(t("应用内快捷键", "In-app shortcut", language)) {
            ForEach(1...9, id: \.self) { slot in
                Button {
                    if !store.setScriptSlot(slot, script: script) { onError() }
                } label: {
                    if store.scriptSlots[String(slot)] == script { Label("⌃⌥\(slot)", systemImage: "checkmark") }
                    else { Text("⌃⌥\(slot)") }
                }
            }
            Button(t("移除快捷键", "Remove shortcut", language)) {
                if !store.setScriptSlot(nil, script: script) { onError() }
            }
        }
    }
}

private struct GlobalScriptShortcutMenu: View {
    @ObservedObject var store: GlobalScriptShortcuts
    let device: String
    let script: String
    let language: Int
    let onStatus: (String) -> Void
    var body: some View {
        Menu(t("全局快捷键", "Global shortcut", language)) {
            ForEach(1...9, id: \.self) { slot in
                Button {
                    if !store.set(slot: slot, device: device, script: script) {
                        onStatus(t("全局快捷键已占用或无法保存。", "Global shortcut is in use or could not be saved.", language))
                    } else if !store.permissionGranted {
                        onStatus(t("已保存快捷键；请在系统设置中允许辅助功能。", "Shortcut saved; allow Accessibility in System Settings.", language))
                    }
                } label: {
                    if store.bindings.slots[String(slot)] == GlobalScriptTarget(device: device.lowercased(), script: script.lowercased()) {
                        Label("⌃⌥⇧\(slot)", systemImage: "checkmark")
                    } else { Text("⌃⌥⇧\(slot)") }
                }
            }
            Button(t("移除全局快捷键", "Remove global shortcut", language)) {
                if !store.set(slot: nil, device: device, script: script) {
                    onStatus(t("无法保存全局快捷键。", "Could not save the global shortcut.", language))
                }
            }
        }
    }
}

private struct ScriptActionsEditor: View {
    @ObservedObject var transport: PlayerTransport
    let script: PlayerScriptSummary
    @Environment(\.dismiss) private var dismiss
    private var language: Int { transport.language }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(t("编辑动作", "Edit actions", language)).font(.headline)
            Text(script.name).foregroundStyle(.secondary)
            if let error = transport.scriptError {
                HStack {
                    Text(error).foregroundStyle(.red)
                    Spacer()
                    Button(t("重新读取", "Reload", language)) { transport.scriptActionsGet(id: script.id) }
                }
            }
            if transport.scriptEditorID == script.id {
                List {
                    ForEach(transport.scriptActions) { action in
                        ScriptActionEditorRow(transport: transport, scriptID: script.id, action: action)
                            .listRowBackground(transport.scriptPlaybackID == script.id && transport.scriptPlaybackAction == action.index ? Color.accentColor.opacity(0.18) : Color.clear)
                    }.onMove { source, destination in
                        guard let from = source.first else { return }
                        let to = destination > from ? destination - 1 : destination
                        if from != to { transport.scriptActionMove(id: script.id, from: from, to: to) }
                    }
                }
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            Text(t("拖拽调整顺序；重叠动作会一起移动，不能单独修改时长。修改立即保存。", "Drag to reorder; overlapping actions move together and cannot be resized individually. Edits save immediately.", language))
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Menu(t("添加脚本片段", "Add script fragment", language)) {
                    ForEach(transport.scripts.filter { $0.id != script.id }) { fragment in
                        Button(fragment.name) { transport.scriptActionCall(id: script.id, fragment: fragment.id) }
                    }
                }.disabled(transport.scripts.allSatisfy { $0.id == script.id })
                Spacer(); Button(t("完成", "Done", language)) { dismiss() }
            }
        }
        .padding(20).frame(minWidth: 570, minHeight: 400)
        .onAppear { transport.scriptActionsGet(id: script.id) }
    }
}

private struct ScriptActionEditorRow: View {
    @ObservedObject var transport: PlayerTransport
    let scriptID: String
    let action: PlayerScriptActionRow
    @State private var waitText = ""
    @State private var confirmDelete = false
    @State private var xText = ""
    @State private var yText = ""
    @State private var endXText = ""
    @State private var endYText = ""
    @State private var durationText = ""
    private var language: Int { transport.language }
    private var wait: Int? {
        guard let value = Int(waitText), (0...86_400_000).contains(value) else { return nil }
        return value
    }
    var body: some View {
        VStack(alignment: .leading) {
        HStack {
            VStack(alignment: .leading) {
                Text(action.kind == "script_call" ? t("调用脚本：", "Run script: ", language) + (transport.scripts.first(where: { $0.id == action.detail })?.name ?? action.detail) : action.title(language: language))
                Text("\(action.start_ms)–\(action.end_ms) ms").font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            if action.wait_ms != nil {
                Text(t("等待", "Wait", language)).font(.caption)
                TextField("ms", text: $waitText).frame(width: 70)
                Text("ms").font(.caption)
                Button(t("应用", "Apply", language)) {
                    if let wait { transport.scriptActionWait(id: scriptID, index: action.index, milliseconds: wait) }
                }.disabled(wait == nil || wait == action.wait_ms)
            }
            Button(role: .destructive) { confirmDelete = true } label: {
                Image(systemName: "trash")
            }.help(t("删除动作", "Delete action", language))
        }
        if action.point != nil {
            HStack {
                Text(t("起点", "From", language)); TextField("X", text: $xText).frame(width: 48); TextField("Y", text: $yText).frame(width: 48)
                Text(t("终点", "To", language)); TextField("X", text: $endXText).frame(width: 48); TextField("Y", text: $endYText).frame(width: 48)
                Text(t("时长", "Duration", language)); TextField("ms", text: $durationText).frame(width: 65); Text("ms")
                Button(t("应用", "Apply", language)) {
                    if let x = Double(xText), let y = Double(yText), let endX = Double(endXText), let endY = Double(endYText), let duration = Int(durationText), (0...86_400_000).contains(duration) {
                        transport.scriptActionGesture(id: scriptID, index: action.index, x: x, y: y, endX: endX, endY: endY, duration: duration)
                    }
                }
            }.font(.caption)
        }
        }
        .confirmationDialog(t("删除这个动作？", "Delete this action?", language), isPresented: $confirmDelete) {
            Button(t("删除", "Delete", language), role: .destructive) {
                transport.scriptActionDelete(id: scriptID, index: action.index)
            }
            Button(t("取消", "Cancel", language), role: .cancel) { }
        }
        .onAppear { loadFields() }
        .onChange(of: action.wait_ms) { value in waitText = value.map(String.init) ?? "" }
        .onChange(of: action.point?.x) { _ in loadFields() }
        .onChange(of: action.point?.y) { _ in loadFields() }
        .onChange(of: action.endpoint?.x) { _ in loadFields() }
        .onChange(of: action.endpoint?.y) { _ in loadFields() }
        .onChange(of: action.end_ms - action.start_ms) { _ in loadFields() }
    }

    private func loadFields() {
        waitText = action.wait_ms.map(String.init) ?? ""
        if let point = action.point { xText = String(format: "%.4f", point.x); yText = String(format: "%.4f", point.y) }
        if let point = action.endpoint { endXText = String(format: "%.4f", point.x); endYText = String(format: "%.4f", point.y) }
        durationText = String(action.end_ms - action.start_ms)
    }
}

struct PlaybackOptionsPopover: View {
    let language: Int
    @Binding var mode: String
    @Binding var count: Int
    @Binding var seconds: Int
    @Binding var interval: Int
    @Binding var speed: Double
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(t("回放设置", "Playback settings", language)).font(.headline)
            Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 12) {
                GridRow {
                    Text(t("模式", "Mode", language)).foregroundStyle(.secondary)
                    Picker("", selection: $mode) {
                        ForEach(PlaybackMode.allCases, id: \.self) { item in
                            Text(PlayerScriptText.mode(item, language: language)).tag(item.rawValue)
                        }
                    }.labelsHidden().accessibilityLabel(t("模式", "Mode", language)).frame(width: 145, alignment: .trailing)
                }
                if mode == PlaybackMode.count.rawValue {
                    GridRow {
                        Text(t("次数", "Loops", language)).foregroundStyle(.secondary)
                        Stepper(value: $count, in: 1...1000) {
                            Text("\(count)").monospacedDigit().frame(maxWidth: .infinity, alignment: .trailing)
                        }.accessibilityLabel(t("次数", "Loops", language)).frame(width: 145)
                    }
                } else if mode == PlaybackMode.duration.rawValue {
                    GridRow {
                        Text(t("时长", "Duration", language)).foregroundStyle(.secondary)
                        Stepper(value: $seconds, in: 1...86400) {
                            Text(String(format: t("%d 秒", "%d s", language), seconds)).monospacedDigit()
                                .frame(maxWidth: .infinity, alignment: .trailing)
                        }.accessibilityLabel(t("时长", "Duration", language)).frame(width: 145)
                    }
                }
                GridRow {
                    Text(t("循环间隔", "Loop interval", language)).foregroundStyle(.secondary)
                    HStack(spacing: 6) {
                        TextField("", value: $interval, format: .number).textFieldStyle(.roundedBorder)
                            .accessibilityLabel(t("循环间隔（毫秒）", "Loop interval (milliseconds)", language))
                            .multilineTextAlignment(.trailing)
                        Text(t("毫秒", "ms", language)).foregroundStyle(.secondary)
                    }.frame(width: 145)
                }
                GridRow {
                    Text(t("倍速", "Speed", language)).foregroundStyle(.secondary)
                    Picker("", selection: $speed) {
                        ForEach([0.25, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0], id: \.self) { value in
                            Text(String(format: "%.2g×", value)).tag(value)
                        }
                    }.labelsHidden().accessibilityLabel(t("倍速", "Speed", language)).frame(width: 145, alignment: .trailing)
                }
            }
            .font(.system(size: 13))
        }
        .padding(18).frame(width: 310)
        .modifier(PlayerGlassSurface())
    }
}

extension Notification.Name {
    static let playerScriptsChanged = Notification.Name("miko.playerScriptsChanged")
}
