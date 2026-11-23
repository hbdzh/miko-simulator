// SPDX-License-Identifier: GPL-3.0-only
import SwiftUI
import Combine

struct GamepadPanel: View {
    @ObservedObject var transport: PlayerTransport
    @ObservedObject private var mapping: KeyMappingStore
    @State private var tuningController: Int?
    @State private var tuningPackage: String?
    @State private var tuning = GamepadTuning()
    private let refreshTimer = Timer.publish(every: 0.4, on: .main, in: .common).autoconnect()

    init(transport: PlayerTransport) {
        self.transport = transport
        _mapping = ObservedObject(wrappedValue: transport.mapping)
    }

    private var language: Int { transport.language }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(t("游戏手柄", "Game controllers", language)).font(.headline)
                Spacer()
                if transport.gamepadState == .connected {
                    Button(t("断开", "Disconnect", language)) { transport.onToggleGamepad?() }
                } else if transport.gamepadRecovering {
                    Button(t("取消自动重连", "Cancel reconnection", language)) { transport.onToggleGamepad?() }
                }
            }
            Picker(t("模式", "Mode", language), selection: $transport.gamepadMode) {
                Text(t("原生直通", "Native", language)).tag(GamepadInputMode.native)
                Text(t("映射触控", "Touch mapping", language)).tag(GamepadInputMode.touch)
            }.wbDropdownStyle()
            .disabled(transport.gamepadState == .connected || transport.gamepadState == .connecting || transport.gamepadRecovering)
            if transport.gamepadState == .connected {
                Text(t("连接时，测试操作也会发送到模拟器。", "When connected, test input is also sent to the emulator.", language))
                    .font(.caption).foregroundStyle(.secondary)
            }
            if transport.gamepadRecovering && transport.gamepadState == .disconnected {
                Text(t("手柄已断开，等待重新连接。", "Controller disconnected; waiting for it to reconnect.", language))
                    .font(.caption).foregroundStyle(.secondary)
            }
            if let error = transport.gamepadListError {
                Text(error).foregroundStyle(.secondary)
            } else if transport.gamepads.isEmpty {
                Text(t("没有检测到游戏手柄", "No game controllers detected", language)).foregroundStyle(.secondary)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        ForEach(transport.gamepads) { gamepad in
                            VStack(alignment: .leading, spacing: 6) {
                                HStack {
                                    Text(gamepad.name).fontWeight(.medium)
                                    Spacer()
                                    Button(t("调校", "Tune", language)) {
                                        tuningController = nil
                                        tuningPackage = mapping.foregroundPackage
                                        tuning = GamepadTuningStore.load(controller: gamepad, package: tuningPackage)
                                        tuningController = gamepad.id
                                    }
                                    if gamepad.boundDevice == nil {
                                        Button(t("连接", "Connect", language)) { transport.onSelectGamepad?(gamepad.id) }
                                            .disabled(transport.gamepadState == .connecting || transport.gamepadState == .connected || transport.gamepadRecovering)
                                    }
                                }
                                if tuningController == gamepad.id {
                                    tuningControls(for: gamepad)
                                }
                                if let device = gamepad.boundDevice {
                                    Text(t("已绑定：", "Bound to: ", language) + device).font(.caption).foregroundStyle(.secondary)
                                } else {
                                    Text(t("未绑定", "Unbound", language)).font(.caption).foregroundStyle(.secondary)
                                }
                                Text(powerDescription(gamepad)).font(.caption).foregroundStyle(.secondary)
                                Text(t("震动支持取决于手柄", "Vibration depends on the controller", language))
                                    .font(.caption).foregroundStyle(.secondary)
                                if gamepad.controls.isEmpty {
                                    Text(t("没有可显示的控件", "No controls available", language)).font(.caption).foregroundStyle(.secondary)
                                } else {
                                    ForEach(gamepad.controls) { control in
                                        HStack {
                                            Text(control.name)
                                            Spacer()
                                            Text(String(format: "%.2f", control.value)).monospacedDigit()
                                        }
                                        .font(.caption)
                                        .foregroundStyle(abs(control.value) > 0.05 ? Color.primary : Color.secondary)
                                    }
                                }
                            }
                            Divider()
                        }
                    }
                }.frame(maxHeight: 360)
            }
        }
        .padding(16)
        .frame(width: 300, alignment: .leading)
        .onAppear {
            transport.gamepadPanelOpen = true
            transport.onRefreshGamepads?()
            transport.onRefreshGamepadStatus?()
        }
        .onDisappear { transport.gamepadPanelOpen = false }
        .onReceive(refreshTimer) { _ in transport.onRefreshGamepads?() }
        .onChange(of: tuning) { value in
            guard let gamepad = transport.gamepads.first(where: { $0.id == tuningController }) else { return }
            GamepadTuningStore.save(value, controller: gamepad, package: tuningPackage)
        }
    }

    private func powerDescription(_ gamepad: GamepadController) -> String {
        switch gamepad.powerState {
        case "wired": return t("有线供电", "Wired power", language)
        case "charged": return t("电量已充满", "Battery charged", language)
        case "available":
            return gamepad.powerPercent.map { t("电量 ", "Battery ", language) + "\($0)%" } ?? t("电量未知", "Battery unknown", language)
        case "charging":
            return t("充电中", "Charging", language) + (gamepad.powerPercent.map { " \($0)%" } ?? "")
        case "discharging":
            return gamepad.powerPercent.map { t("电量 ", "Battery ", language) + "\($0)%" } ?? t("电量未知", "Battery unknown", language)
        default: return t("电量不可用", "Battery unavailable", language)
        }
    }

    @ViewBuilder private func tuningControls(for gamepad: GamepadController) -> some View {
        let names = Set(gamepad.controls.map(\.name))
        VStack(alignment: .leading, spacing: 6) {
            Text(t("应用：", "App: ", language) + (tuningPackage ?? t("默认", "Default", language)))
                .font(.caption).foregroundStyle(.secondary)
            Text(t("调校在下次连接时生效。", "Tuning applies on the next connection.", language))
                .font(.caption).foregroundStyle(.secondary)
            if !names.isDisjoint(with: ["LeftStickX", "LeftStickY"]) {
                Text(t("左摇杆", "Left stick", language)).font(.caption.bold())
                tuningSlider(t("死区", "Deadzone", language), value: $tuning.left.deadzone, range: 0...0.9)
                tuningSlider(t("灵敏度", "Sensitivity", language), value: $tuning.left.sensitivity, range: 0.1...5)
                HStack {
                    if names.contains("LeftStickX") { Toggle(t("反转 X", "Invert X", language), isOn: $tuning.left.invertX) }
                    if names.contains("LeftStickY") { Toggle(t("反转 Y", "Invert Y", language), isOn: $tuning.left.invertY) }
                }.font(.caption)
            }
            if !names.isDisjoint(with: ["RightStickX", "RightStickY"]) {
                Text(t("右摇杆", "Right stick", language)).font(.caption.bold())
                tuningSlider(t("死区", "Deadzone", language), value: $tuning.right.deadzone, range: 0...0.9)
                tuningSlider(t("灵敏度", "Sensitivity", language), value: $tuning.right.sensitivity, range: 0.1...5)
                HStack {
                    if names.contains("RightStickX") { Toggle(t("反转 X", "Invert X", language), isOn: $tuning.right.invertX) }
                    if names.contains("RightStickY") { Toggle(t("反转 Y", "Invert Y", language), isOn: $tuning.right.invertY) }
                }.font(.caption)
            }
            if !names.isDisjoint(with: ["LeftTrigger2", "RightTrigger2"]) {
                tuningSlider(t("扳机阈值", "Trigger threshold", language), value: $tuning.triggerThreshold, range: 0...0.9)
            }
            Button(t("恢复默认设置", "Restore defaults", language)) { tuning = GamepadTuning() }
        }.padding(.vertical, 6)
    }

    private func tuningSlider(_ label: String, value: Binding<Double>, range: ClosedRange<Double>) -> some View {
        HStack {
            Text(label).frame(width: 65, alignment: .leading)
            Slider(value: value, in: range)
            Text(String(format: "%.2f", value.wrappedValue)).monospacedDigit().frame(width: 35)
        }.font(.caption)
    }
}
