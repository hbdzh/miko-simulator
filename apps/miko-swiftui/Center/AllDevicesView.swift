// SPDX-License-Identifier: GPL-3.0-only
import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// The same device silhouette is used in the sidebar, cards and overview.
enum DeviceArtworkState {
    case stopped, starting, running

    init(deviceState: String) {
        switch deviceState {
        case "running": self = .running
        case "starting", "booting": self = .starting
        default: self = .stopped
        }
    }
}

struct DeviceArtwork: View {
    let state: DeviceArtworkState
    let frame: CGImage?
    let width: CGFloat
    let height: CGFloat

    private var bodyRadius: CGFloat { width * 0.19 }
    private var inset: CGFloat { max(1.5, width * 0.065) }
    private var screenRadius: CGFloat { width * 0.135 }
    private var statusColor: Color {
        switch state {
        case .running: return Color(nsColor: .systemGreen)
        case .starting: return Color(nsColor: .systemOrange)
        case .stopped: return Color(nsColor: .systemGray)
        }
    }

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: bodyRadius, style: .continuous)
                .fill(LinearGradient(colors: [Color(white: 0.38), Color(white: 0.13), Color(white: 0.26)],
                                     startPoint: .topLeading, endPoint: .bottomTrailing))
            RoundedRectangle(cornerRadius: bodyRadius, style: .continuous)
                .strokeBorder(Color.white.opacity(0.22), lineWidth: max(0.5, width * 0.015))
            screen
                .frame(width: width - inset * 2, height: height - inset * 2)
                .clipShape(RoundedRectangle(cornerRadius: screenRadius, style: .continuous))
            Circle()
                .fill(Color.black.opacity(0.72))
                .frame(width: max(1.5, width * 0.065), height: max(1.5, width * 0.065))
                .frame(maxHeight: .infinity, alignment: .top)
                .padding(.top, inset + width * 0.08)
        }
        .frame(width: width, height: height)
        .overlay(alignment: .bottomTrailing) {
            Circle().fill(statusColor)
                .frame(width: max(5, width * 0.14), height: max(5, width * 0.14))
                .overlay(Circle().strokeBorder(Color(nsColor: .windowBackgroundColor), lineWidth: max(1, width * 0.025)))
                .padding(.trailing, max(0.5, width * 0.015))
                .padding(.bottom, max(0.5, width * 0.015))
        }
        .accessibilityHidden(true)
    }

    @ViewBuilder private var screen: some View {
        if state == .running, let frame {
            Image(decorative: frame, scale: 1).resizable().scaledToFill()
        } else {
            ZStack {
                LinearGradient(colors: state == .running
                               ? [Color(red: 0.25, green: 0.32, blue: 0.40), Color(red: 0.12, green: 0.16, blue: 0.22)]
                               : [Color(white: 0.17), Color(white: 0.075)],
                               startPoint: .topLeading, endPoint: .bottomTrailing)
                if state == .stopped {
                    LinearGradient(colors: [.white.opacity(0.09), .clear, .clear],
                                   startPoint: .topLeading, endPoint: .bottomTrailing)
                } else if state == .starting {
                    ProgressView().controlSize(.mini).tint(.white.opacity(0.8))
                        .scaleEffect(min(1, width / 40))
                }
            }
        }
    }
}

/// Compact previews use the existing player frame when one is available.
struct DevicePreview: View {
    @ObservedObject var model: CenterModel
    let device: Device
    let width: CGFloat
    let height: CGFloat

    @ViewBuilder var body: some View {
        if device.running {
            TimelineView(.periodic(from: .now, by: 2)) { _ in preview }
        } else {
            preview
        }
    }

    private var preview: some View {
        DeviceArtwork(state: DeviceArtworkState(deviceState: device.status.state),
                      frame: device.running ? (model.players[device.id]?.transport.lastFrame ?? model.deviceThumbnails[device.id]) : nil,
                      width: width, height: height)
    }
}

/// The library remains a device-management surface; choosing a device opens
/// the existing overview without starting it or opening its player.
struct AllDevicesView: View {
    @ObservedObject var model: CenterModel
    @ObservedObject private var groups: DeviceGroups
    @State private var budget: DeviceBudget?
    @State private var budgetError = false
    @State private var budgetLoading = false
    @State private var budgetRefreshPending = false
    @State private var budgetGeneration = 0
    @AppStorage(BackgroundFrameMode.defaultsKey) private var backgroundFrameMode = BackgroundFrameMode.saveResources.rawValue
    @State private var selectedDevices = Set<String>()
    @State private var startInterval = 2
    @State private var selectedGroup: String?
    @State private var showFlowEditor = false
    init(model: CenterModel) {
        self.model = model
        self.groups = model.deviceGroups
    }
    private var visibleDevices: [Device] {
        guard let id = selectedGroup, let group = groups.groups.first(where: { $0.id == id }) else { return model.filteredDevices }
        return model.filteredDevices.filter { group.devices.contains($0.id) }
    }
    private let budgetTimer = Timer.publish(every: 30, on: .main, in: .common).autoconnect()
    private func t(_ zh: String, _ en: String) -> String { model.text(zh, en) }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text(t("全部设备", "All devices")).font(.title2.weight(.semibold))
                    Text(String(format: t("%d 台设备", "%d devices"), visibleDevices.count))
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button(t("界面流程", "UI workflows")) { showFlowEditor = true }
                    .buttonStyle(WbPillButtonStyle())
                if #available(macOS 26.0, *) {
                    viewPicker
                        .controlSize(.large)
                        .clipShape(Capsule())
                        .glassEffect(.regular, in: Capsule())
                    newDeviceButton
                        .buttonStyle(.glass)
                        .buttonBorderShape(.capsule)
                        .controlSize(.large)
                } else {
                    viewPicker
                    newDeviceButton
                }
            }
            budgetSummary
            groupControls
            batchControls
            if visibleDevices.isEmpty {
                VStack(spacing: 8) {
                    Text(t("没有匹配的设备", "No matching devices")).foregroundStyle(.secondary)
                    Button(t("显示全部设备", "Show all devices")) { model.deviceQuery = ""; selectedGroup = nil }
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    if model.gridView {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 260, maximum: 420), spacing: 14)], spacing: 14) {
                            ForEach(visibleDevices) { device in deviceCard(device) }
                        }
                    } else {
                        LazyVStack(spacing: 8) {
                            ForEach(visibleDevices) { device in deviceRow(device) }
                        }
                    }
                }
            }
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .accessibilityIdentifier("all-devices-view")
        .sheet(isPresented: $showFlowEditor) {
            FlowEditorView(model: model, initialDevices: selectedDevices)
        }
        .onAppear { refreshBudget() }
        .onDisappear { budgetGeneration += 1; budgetLoading = false; budgetRefreshPending = false }
        .onReceive(budgetTimer) { _ in refreshBudget() }
        .onChange(of: model.devices.map { "\($0.id):\($0.status.state)" }) { _ in refreshBudget() }
        .onChange(of: backgroundFrameMode) { value in
            let mode = BackgroundFrameMode(rawValue: value) ?? .saveResources
            for player in model.players.values { player.transport.setBackgroundFrameMode(mode) }
        }
    }

    private func refreshBudget() {
        if budgetLoading { budgetRefreshPending = true; return }
        budgetLoading = true
        budgetGeneration += 1
        let generation = budgetGeneration
        model.call("host.budget") { result in
            guard generation == budgetGeneration else { return }
            budgetLoading = false
            if case .success(let value) = result, let next = try? model.decode(value, as: DeviceBudget.self) {
                budget = next; budgetError = false
            } else {
                budget = nil; budgetError = true
            }
            if budgetRefreshPending { budgetRefreshPending = false; refreshBudget() }
        }
    }

    private var budgetSummary: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Label(t("设备内存预算", "Device memory budget"), systemImage: "memorychip")
                    .font(.subheadline.weight(.semibold))
                Spacer()
                if let budget {
                    Text("\(DeviceBudget.gigabytes(budget.used_mb)) / \(DeviceBudget.gigabytes(budget.budget_mb))")
                        .font(.subheadline.monospacedDigit())
                } else if budgetLoading {
                    ProgressView().controlSize(.small)
                } else if budgetError {
                    Button(t("重试", "Retry")) { refreshBudget() }
                        .buttonStyle(WbPillButtonStyle())
                }
            }
            if let budget {
                ProgressView(value: budget.fraction).tint(budget.used_mb > budget.budget_mb ? .orange : .accentColor)
                Text(t("预算为 Mac 内存的 \(budget.budget_percent)%；预算占用取预估值与最近测量值的较大者。CPU 和内存约每 30 秒采样，CPU 100% 相当于一个逻辑核心。", "Budget is \(budget.budget_percent)% of Mac memory; budget usage is the greater of estimated and last measured values. CPU and memory are sampled about every 30 seconds; 100% CPU is one logical core."))
                    .font(.caption).foregroundStyle(.secondary)
                if !model.devices.isEmpty {
                    DisclosureGroup(t("查看各设备占用", "Device usage")) {
                        ScrollView {
                            VStack(spacing: 6) {
                                ForEach(model.devices) { device in
                                    HStack {
                                        Text(device.name).lineLimit(1)
                                        Text(device.statusText(language: model.language)).foregroundStyle(.secondary)
                                        Spacer()
                                        if let usage = budget.devices.first(where: { $0.id == device.id }) {
                                            Text(usage.cpu_percent.map { "CPU \($0)%" } ?? t("CPU 等待采样", "CPU awaiting sample"))
                                                .monospacedDigit()
                                            Text(usage.measured_mb.map { t("实测 ", "Measured ") + DeviceBudget.gigabytes($0) } ?? t("等待采样", "Awaiting sample"))
                                                .monospacedDigit()
                                            Text(t("预算 ", "Budget ") + DeviceBudget.gigabytes(usage.countedMB))
                                                .foregroundStyle(.secondary).monospacedDigit()
                                        }
                                        if device.running {
                                            Button(t("休眠", "Hibernate")) { model.hibernate(device.id) }
                                                .buttonStyle(WbPillButtonStyle())
                                                .disabled(model.busy.contains(device.id) || model.busyHolder(for: device.id) != nil)
                                            Button(t("停止", "Stop")) { model.stop(device.id) }
                                                .buttonStyle(WbPillButtonStyle())
                                                .disabled(model.busy.contains(device.id) || model.busyHolder(for: device.id) != nil)
                                        }
                                    }.font(.caption)
                                }
                            }.padding(.top, 6)
                        }.frame(maxHeight: 130)
                    }.font(.caption)
                }
            } else if budgetError {
                Text(t("无法读取设备内存预算。", "Could not load the device memory budget."))
                    .font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Text(t("后台画面", "Background display"))
                Picker(t("后台画面", "Background display"), selection: $backgroundFrameMode) {
                    Text(t("保持实时画面", "Keep live")) .tag(BackgroundFrameMode.realTime.rawValue)
                    Text(t("后台节省资源", "Save resources")) .tag(BackgroundFrameMode.saveResources.rawValue)
                }
                .labelsHidden().pickerStyle(.segmented).fixedSize()
                Spacer()
            }.font(.caption)
            Text(t("节省资源时，后台可见窗口最高显示 5 帧/秒，隐藏窗口暂停显示；仅减少播放器画面开销。", "In resource-saving mode, visible background windows display up to 5 fps and hidden windows pause display; only player display overhead is reduced."))
                .font(.caption).foregroundStyle(.secondary)
            Text(t("多开建议：新建设备可选择 agent-lean（720 × 1280、2 核、2 GB 内存、关闭音频和摄像头）；不会自动修改已有配置。", "For multiple devices, consider the agent-lean profile (720 × 1280, 2 cores, 2 GB RAM, audio and cameras off) when creating devices. Existing settings are not changed."))
                .font(.caption).foregroundStyle(.secondary)
        }
        .padding(12)
        .background(Color.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(WbColor.borderSubtle))
        .accessibilityIdentifier("device-memory-budget")
    }

    private var viewPicker: some View {
        Picker(t("显示方式", "View"), selection: $model.gridView) {
            Image(systemName: "list.bullet").accessibilityLabel(t("列表", "List")).tag(false)
            Image(systemName: "square.grid.2x2").accessibilityLabel(t("网格", "Grid")).tag(true)
        }
        .pickerStyle(.segmented).labelsHidden()
        .accessibilityLabel(t("设备显示方式", "Device view"))
    }

    private var newDeviceButton: some View {
        Button(t("新建设备", "New device"), systemImage: "plus") { model.present(.newDevice) }
    }

    private var batchControls: some View {
        BatchControls(model: model, batch: model.batchOperations, selected: $selectedDevices, startInterval: $startInterval, visibleDevices: visibleDevices)
    }

    private var groupControls: some View {
        HStack {
            Picker(t("设备分组", "Device group"), selection: $selectedGroup) {
                Text(t("全部设备", "All devices")).tag(String?.none)
                ForEach(groups.groups) { group in Text(group.name).tag(Optional(group.id)) }
            }.frame(maxWidth: 260)
            Button(t("新建分组…", "New group…")) {
                if let name = groupName(title: t("新建设备分组", "New device group"), current: "") {
                    do { try groups.create(name: name, devices: selectedDevices) } catch { model.showError(groupError(error.localizedDescription)) }
                }
            }
            if let id = selectedGroup, let group = groups.groups.first(where: { $0.id == id }) {
                Menu(t("编辑分组", "Edit group")) {
                    Button(t("添加所选设备", "Add selected devices")) { changeGroup(id, add: true) }.disabled(selectedDevices.isEmpty)
                    Button(t("移除所选设备", "Remove selected devices")) { changeGroup(id, add: false) }.disabled(selectedDevices.isEmpty)
                    Button(t("重命名…", "Rename…")) {
                        if let name = groupName(title: t("重命名分组", "Rename group"), current: group.name) {
                            do { try groups.rename(id: id, name: name) } catch { model.showError(groupError(error.localizedDescription)) }
                        }
                    }
                    Button(t("删除分组", "Delete group"), role: .destructive) {
                        do { try groups.delete(id: id); selectedGroup = nil } catch { model.showError(groupError(error.localizedDescription)) }
                    }
                }
            }
            Button(t("排列窗口", "Arrange windows")) { model.arrangePlayerWindows() }
                .disabled(!model.hasPlayers)
            if let error = groups.error { Text(groupError(error)).font(.caption).foregroundStyle(.red) }
            Spacer()
        }
    }

    private func changeGroup(_ id: String, add: Bool) {
        do { try groups.update(id: id, devices: selectedDevices, add: add) } catch { model.showError(groupError(error.localizedDescription)) }
    }

    private func groupError(_ message: String) -> String {
        switch message {
        case "Unsupported device group version": return t("设备分组版本过高，请升级 miko。", message)
        case "Invalid device groups": return t("设备分组文件无效，原文件未修改。", message)
        case "Group not found": return t("找不到设备分组。", message)
        case "Group name must contain 1–80 characters": return t("分组名称须为 1–80 个字符。", message)
        case "A group with this name already exists": return t("已存在同名分组。", message)
        case "Existing device groups could not be read; they were not overwritten": return t("无法读取已有分组，原文件未覆盖。", message)
        case "Device data directory is not available": return t("设备数据目录尚不可用。", message)
        default: return t("设备分组操作失败：", "Device group operation failed: ") + message
        }
    }

    private func groupName(title: String, current: String) -> String? {
        let alert = NSAlert(); alert.messageText = title
        alert.addButton(withTitle: t("保存", "Save")); alert.addButton(withTitle: t("取消", "Cancel"))
        let field = NSTextField(string: current); field.sizeToFit(); field.frame.size.width = 260; alert.accessoryView = field
        alert.window.initialFirstResponder = field
        return alert.runModal() == .alertFirstButtonReturn ? field.stringValue : nil
    }

    private func selectionControl(_ device: Device) -> some View {
        Toggle(t("选择设备", "Select device") + " " + device.name, isOn: Binding(
            get: { selectedDevices.contains(device.id) },
            set: { if $0 { selectedDevices.insert(device.id) } else { selectedDevices.remove(device.id) } }
        ))
        .toggleStyle(.checkbox).labelsHidden()
        .accessibilityLabel(t("选择设备：", "Select device: ") + device.name)
    }

    private func deviceRow(_ device: Device) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 12) {
                selectionControl(device)
                deviceLink(device, thumbnailWidth: 32, thumbnailHeight: 56)
                Spacer(minLength: 8)
                deviceAction(device)
                DeviceMoreButton(model: model, device: device).frame(width: 30, height: 28)
            }
            BatchResultLine(batch: model.batchOperations, id: device.id, language: model.language)
        }
        .padding(.horizontal, 14).padding(.vertical, 14)
        .background(Color.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(WbColor.borderSubtle))
        .onAppear { model.loadCardHardware(device.id) }
    }

    private func deviceCard(_ device: Device) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top) {
                selectionControl(device)
                deviceLink(device, thumbnailWidth: 42, thumbnailHeight: 72)
            }
            HStack {
                deviceAction(device)
                Spacer()
                DeviceMoreButton(model: model, device: device).frame(width: 30, height: 28)
            }
            BatchResultLine(batch: model.batchOperations, id: device.id, language: model.language)
        }
        .padding(14).frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(WbColor.borderSubtle))
        .onAppear { model.loadCardHardware(device.id) }
    }

    private func deviceLink(_ device: Device, thumbnailWidth: CGFloat, thumbnailHeight: CGFloat) -> some View {
        Button { model.select(device.id) } label: {
            HStack(spacing: 12) {
                DevicePreview(model: model, device: device, width: thumbnailWidth, height: thumbnailHeight)
                VStack(alignment: .leading, spacing: 6) {
                    Text(device.name).font(.system(size: 14, weight: .semibold)).lineLimit(1)
                    Text(device.statusText(language: model.language) + " · " + device.androidVersionShort)
                        .font(.system(size: 13)).foregroundStyle(.secondary).lineLimit(1)
                    if let hardware = model.cardHardware[device.id] {
                        Text(hardware.summary(language: model.language)).font(.caption).lineSpacing(2).foregroundStyle(.secondary).lineLimit(2)
                    }
                    if let app = model.appSearchMatches[device.id] {
                        Text(t("匹配应用：\(app)", "Matching app: \(app)"))
                            .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }.contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(t("查看设备：\(device.name)", "View device: \(device.name)"))
    }

    @ViewBuilder private func deviceAction(_ device: Device) -> some View {
        switch DeviceOverviewPrimaryAction(state: device.status.state) {
        case .start:
            Button(t("启动", "Start"), systemImage: "play") { model.start(device.id) }
                .buttonStyle(.borderedProminent).disabled(model.busy.contains(device.id))
        case .stop:
            Button(t("停止", "Stop"), systemImage: "stop") { model.stop(device.id) }
                .disabled(model.busy.contains(device.id))
        case .cancelStart:
            Button(t("取消启动", "Cancel start")) { model.stop(device.id) }
                .disabled(model.busy.contains(device.id))
        case .progress:
            ProgressView().controlSize(.small)
        }
    }
}

private struct BatchControls: View {
    @ObservedObject var model: CenterModel
    @ObservedObject var batch: BatchDeviceOperations
    @Binding var selected: Set<String>
    @Binding var startInterval: Int
    let visibleDevices: [Device]
    private func t(_ zh: String, _ en: String) -> String { model.text(zh, en) }
    private var selectedCount: Int { model.devices.filter { selected.contains($0.id) }.count }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Button(t("全选当前列表", "Select visible")) { selected.formUnion(visibleDevices.map(\.id)) }
                Button(t("清除选择", "Clear selection")) { selected.removeAll() }.disabled(selected.isEmpty)
                Text(String(format: t("已选 %d 台", "%d selected"), selectedCount)).font(.caption).foregroundStyle(.secondary)
                Spacer()
                if batch.running {
                    Button(batch.cancelling ? t("正在取消…", "Cancelling…") : t("取消批量任务", "Cancel batch")) { batch.cancel() }
                        .disabled(batch.cancelling)
                }
            }
            HStack {
                Button(t("批量启动", "Start selected")) { model.beginBatch(.start(interval: TimeInterval(startInterval)), ids: selected) }
                    .disabled(selectedCount == 0 || batch.running)
                Stepper(value: $startInterval, in: 0...30) {
                    Text(t("启动间隔 \(startInterval) 秒", "Start interval: \(startInterval) s")).font(.caption)
                }.fixedSize()
                Button(t("批量停止", "Stop selected")) { model.beginBatch(.stop, ids: selected) }
                    .disabled(selectedCount == 0 || batch.running)
                Button(t("批量安装 APK…", "Install APK on selected…")) { chooseAPK() }
                    .disabled(selectedCount == 0 || batch.running)
                Button(t("批量克隆", "Clone selected")) { model.beginBatch(.clone, ids: selected) }
                    .disabled(selectedCount == 0 || batch.running)
                Spacer()
            }
        }
        .padding(12)
        .background(Color.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
    }

    private func chooseAPK() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        if let type = UTType(filenameExtension: "apk") { panel.allowedContentTypes = [type] }
        guard panel.runModal() == .OK, let url = panel.url else { return }
        model.beginBatch(.install(path: url.path), ids: selected)
    }
}

private struct BatchResultLine: View {
    @ObservedObject var batch: BatchDeviceOperations
    let id: String
    let language: Int
    private func t(_ zh: String, _ en: String) -> String { language == 0 ? en : zh }
    var body: some View {
        if let result = batch.results.first(where: { $0.id == id }) {
            HStack(spacing: 6) {
                switch result.status {
                case .queued:
                    Text(t("等待中", "Queued"))
                case .running(let message, let fraction):
                    if let fraction { ProgressView(value: min(1, max(0, fraction))).frame(width: 70) }
                    else { ProgressView().controlSize(.mini) }
                    Text(message.isEmpty ? t("进行中", "Running") : OperationMessages.progress(message, language: language)).lineLimit(2)
                case .succeeded:
                    Label(t("完成", "Completed"), systemImage: "checkmark.circle").foregroundStyle(.green)
                case .failed(let message):
                    Label(message, systemImage: "exclamationmark.triangle").foregroundStyle(.red).lineLimit(3)
                    if !batch.running { Button(t("重试", "Retry")) { batch.retry(id) } }
                case .cancelled:
                    Text(t("已取消", "Cancelled")).foregroundStyle(.secondary)
                    if !batch.running { Button(t("重试", "Retry")) { batch.retry(id) } }
                }
                Spacer(minLength: 0)
            }.font(.caption)
        }
    }
}
