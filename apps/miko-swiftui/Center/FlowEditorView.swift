// SPDX-License-Identifier: GPL-3.0-only
import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

private struct FlowDraftNode: Identifiable {
    let id = UUID()
    var type = "wait"
    var x = "0.5", y = "0.5", endX = "0.7", endY = "0.5", duration = "300"
    var value = "", locatorType = "ui", resourceID = "", template = "", threshold = "0.9"
    var present = true, timeout = "10000", interval = "500", attempts = "3"
    var thenNodes: [FlowDraftNode] = [], elseNodes: [FlowDraftNode] = []

    func number(_ value: String) throws -> Double {
        guard let result = Double(value), result.isFinite else { throw FlowDraftError.invalidNumber }
        return result
    }
    func integer(_ value: String) throws -> Int {
        guard let result = Int(value), result >= 0 else { throw FlowDraftError.invalidNumber }
        return result
    }
    func locator() throws -> [String: Any] {
        if locatorType == "template" { return ["type":"template", "path":template, "threshold":try number(threshold)] }
        return ["type":"ui", "text":value.isEmpty ? NSNull() : value as Any,
                "resource_id":resourceID.isEmpty ? NSNull() : resourceID as Any]
    }
    func json() throws -> [String: Any] {
        func point(_ x: String, _ y: String) throws -> [String: Double] { ["x":try number(x), "y":try number(y)] }
        switch type {
        case "tap": return ["type":"action", "action":["type":"tap", "point":try point(x,y)]]
        case "tap_element": return ["type":"action", "action":["type":"tap_element", "locator":try locator()]]
        case "swipe": return ["type":"action", "action":["type":"swipe", "from":try point(x,y), "to":try point(endX,endY), "duration_ms":try integer(duration)]]
        case "text": return ["type":"action", "action":["type":"text", "text":value]]
        case "key": return ["type":"action", "action":["type":"key", "key":value]]
        case "script": return ["type":"action", "action":["type":"script", "id":value]]
        case "wait_element": return ["type":"wait_element", "locator":try locator(), "present":present, "timeout_ms":try integer(timeout), "interval_ms":try integer(interval)]
        case "branch": return ["type":"branch", "locator":try locator(), "then_nodes":try thenNodes.map { try $0.json() }, "else_nodes":try elseNodes.map { try $0.json() }]
        case "retry": return ["type":"retry", "nodes":try thenNodes.map { try $0.json() }, "max_attempts":try integer(attempts), "timeout_ms":try integer(timeout), "interval_ms":try integer(interval)]
        default: return ["type":"wait", "ms":try integer(duration)]
        }
    }
    init(type: String = "wait") { self.type = type }
    init(json: [String: Any]) throws {
        let outer = json["type"] as? String ?? ""
        let action = json["action"] as? [String: Any] ?? [:]
        type = outer == "action" ? (action["type"] as? String ?? "") : outer
        guard ["tap","tap_element","swipe","text","key","script","wait","wait_element","branch","retry"].contains(type) else { throw FlowDraftError.invalidFile }
        func str(_ value: Any?, _ fallback: String) -> String { value.map { "\($0)" } ?? fallback }
        let from = (action["point"] ?? action["from"]) as? [String: Any] ?? [:]
        let to = action["to"] as? [String: Any] ?? [:]
        x = str(from["x"], x); y = str(from["y"], y); endX = str(to["x"], endX); endY = str(to["y"], endY)
        duration = str(action["duration_ms"] ?? json["ms"], duration)
        value = str(action["text"] ?? action["key"] ?? action["id"], value)
        let loc = (action["locator"] ?? json["locator"]) as? [String: Any] ?? [:]
        locatorType = loc["type"] as? String ?? "ui"
        if let text = loc["text"] as? String { value = text }
        resourceID = loc["resource_id"] as? String ?? ""
        template = loc["path"] as? String ?? ""; threshold = str(loc["threshold"],threshold)
        present = json["present"] as? Bool ?? true
        timeout = str(json["timeout_ms"],timeout); interval = str(json["interval_ms"],interval); attempts = str(json["max_attempts"],attempts)
        thenNodes = try ((json["then_nodes"] ?? json["nodes"]) as? [[String:Any]] ?? []).map { try FlowDraftNode(json: $0) }
        elseNodes = try (json["else_nodes"] as? [[String:Any]] ?? []).map { try FlowDraftNode(json: $0) }
    }
}
private enum FlowDraftError: LocalizedError {
    case invalidNumber, invalidFile
    var errorDescription: String? { self == .invalidNumber ? "Invalid numeric value" : "Invalid workflow file" }
}

struct FlowEditorView: View {
    @ObservedObject var model: CenterModel
    let initialDevices: Set<String>
    @Environment(\.dismiss) private var dismiss
    @State private var name = "Workflow"
    @State private var nodes: [FlowDraftNode] = []
    @State private var selected = Set<String>()
    @State private var statuses: [String:String] = [:]
    @State private var message = ""
    @State private var screenshots = false
    @State private var paused = false
    private let timer = Timer.publish(every: 2, on: .main, in: .common).autoconnect()
    private func t(_ zh: String, _ en: String) -> String { model.text(zh,en) }
    private var running: [Device] { model.devices.filter(\.running) }
    private func flow() throws -> [String:Any] { ["name":name,"nodes":try nodes.map { try $0.json() }] }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(t("界面状态流程", "UI workflows")).font(.title2.bold())
                Spacer()
                Button(t("打开", "Open"), action: open)
                Button(t("保存", "Save"), action: save)
                Button(t("关闭", "Close")) { dismiss() }
            }
            HStack { Text(t("流程名称", "Name")); TextField(t("流程名称", "Name"), text:$name) }
            ScrollView {
                VStack(alignment:.leading, spacing: 10) {
                    FlowNodesEditor(model:model, nodes:$nodes, device:running.first(where: { selected.contains($0.id) })?.id)
                    Divider()
                    Text(t("运行设备", "Devices")).font(.headline)
                    ForEach(running) { device in
                        HStack {
                            Toggle(device.name, isOn:Binding(get:{selected.contains(device.id)},set:{ if $0 { selected.insert(device.id) } else { selected.remove(device.id) } }))
                                .toggleStyle(.checkbox)
                            Spacer()
                            Text(statuses[device.id] ?? "").font(.caption).foregroundStyle(.secondary)
                            ForEach(["pause","resume","step","stop"], id:\.self) { command in
                                Button(label(command)) { control(device.id, command) }.disabled(statuses[device.id] == nil)
                            }
                        }
                    }
                    Toggle(t("启动后暂停，便于单步调试", "Start paused for single-step debugging"),isOn:$paused)
                    Toggle(t("为每个节点保存截图", "Save a screenshot for each node"),isOn:$screenshots)
                }.padding(4)
            }
            HStack {
                Text(message).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                Spacer()
                Button(t("运行所选设备", "Run on selected devices"), action:run).disabled(selected.isEmpty || nodes.isEmpty)
            }
        }
        .padding(20).frame(minWidth:780,minHeight:620)
        .onAppear { selected = initialDevices.intersection(Set(running.map(\.id))); refresh() }
        .onReceive(timer) { _ in refresh() }
    }
    private func label(_ command:String) -> String {
        switch command { case "pause": return t("暂停","Pause"); case "resume": return t("继续","Resume"); case "step": return t("单步","Step"); default:return t("停止","Stop") }
    }
    private func refresh() {
        for device in running {
            model.call("flow.status",["device":device.id]) { result in
                if case .success(let raw) = result, let status = raw as? [String:Any] {
                    statuses[device.id] = "\((status["paused"] as? Bool == true) ? t("已暂停", "Paused") : t("运行中", "Running")) · \(status["node"] as? String ?? "")"
                } else { statuses.removeValue(forKey:device.id) }
            }
        }
    }
    private func control(_ device:String,_ command:String) {
        model.call("flow.control",["device":device,"command":command]) { result in
            if case .failure(let error) = result { message = error.localizedDescription }
            refresh()
        }
    }
    private func run() {
        do {
            let payload = try flow()
            for device in selected {
                statuses[device] = t("正在启动…", "Starting…")
                model.call("flow.run",["device":device,"flow":payload,"start_paused":paused,"screenshots":screenshots], onProgress: { text,_,_ in statuses[device] = text }) { result in
                    switch result {
                    case .success(let data): message = "\(device): \(data)"
                    case .failure(let error): message = "\(device): \(error.localizedDescription)"
                    }
                    statuses.removeValue(forKey:device)
                }
            }
        } catch { message = error.localizedDescription }
    }
    private func open() {
        let panel = NSOpenPanel(); panel.allowedContentTypes = [.json]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let value = try JSONSerialization.jsonObject(with: Data(contentsOf:url)) as? [String:Any]
            guard let value, let title = value["name"] as? String, let rows = value["nodes"] as? [[String:Any]] else { throw FlowDraftError.invalidFile }
            let imported = try rows.map { try FlowDraftNode(json:$0) }
            name = title; nodes = imported; message = ""
        } catch { message = error.localizedDescription }
    }
    private func save() {
        do {
            let payload = try flow()
            let bytes = try JSONSerialization.data(withJSONObject:payload,options:[.prettyPrinted,.sortedKeys])
            model.call("flow.validate",["flow":payload]) { result in
                switch result {
                case .failure(let error): message = error.localizedDescription
                case .success:
                    let panel = NSSavePanel(); panel.allowedContentTypes = [.json]; panel.nameFieldStringValue = "workflow.json"
                    guard panel.runModal() == .OK, let url = panel.url else { return }
                    do { try bytes.write(to:url,options:.atomic); message = t("已保存", "Saved") }
                    catch { message = error.localizedDescription }
                }
            }
        } catch { message = error.localizedDescription }
    }
}

private struct FlowNodesEditor: View {
    @ObservedObject var model: CenterModel
    @Binding var nodes: [FlowDraftNode]
    let device: String?
    private func t(_ zh:String,_ en:String)->String { model.text(zh,en) }
    var body: some View {
        VStack(alignment:.leading, spacing:8) {
            ForEach($nodes) { $node in
                FlowNodeEditor(model:model,node:$node,device:device,remove:{ nodes.removeAll { $0.id == node.id } }, move:{ delta in
                    guard let index = nodes.firstIndex(where: { $0.id == node.id }) else { return }
                    let target = index + delta
                    if nodes.indices.contains(target) { nodes.swapAt(index,target) }
                })
            }
            Button { nodes.append(FlowDraftNode()) } label: { Label(t("添加节点","Add node"),systemImage:"plus") }
        }
    }
}

private struct FlowNodeEditor: View {
    @ObservedObject var model: CenterModel
    @Binding var node: FlowDraftNode
    let device: String?
    let remove: () -> Void
    let move: (Int) -> Void
    @State private var capture = false
    private func t(_ zh:String,_ en:String)->String { model.text(zh,en) }
    private var types: [(String,String)] { [
        ("wait",t("固定等待","Wait")),("tap",t("点击坐标","Tap point")),("tap_element",t("点击元素","Tap element")),
        ("swipe",t("滑动","Swipe")),("text",t("输入文本","Text")),("key",t("按键","Key")),
        ("script",t("调用录制脚本","Run recording")),("wait_element",t("等待元素","Wait for element")),
        ("branch",t("条件分支","Branch")),("retry",t("有限重试","Retry"))] }
    var body: some View {
        VStack(alignment:.leading,spacing:8) {
            HStack {
                Picker(t("节点类型","Node type"),selection:$node.type) { ForEach(types,id:\.0) { type in Text(type.1).tag(type.0) } }.labelsHidden().frame(width:200)
                Spacer()
                Button("↑") { move(-1) }; Button("↓") { move(1) }
                Button(role:.destructive,action:remove) { Image(systemName:"trash") }
            }
            if ["tap","swipe"].contains(node.type) {
                HStack { field("X",$node.x); field("Y",$node.y)
                    if node.type == "swipe" { field(t("终点 X","End X"),$node.endX); field(t("终点 Y","End Y"),$node.endY) }
                }
                Text(t("坐标为 0–1 的屏幕比例。","Coordinates are normalized to 0–1.")).font(.caption).foregroundStyle(.secondary)
            }
            if ["wait","swipe"].contains(node.type) { field(t("时长（毫秒）","Duration (ms)"),$node.duration) }
            if ["text","key","script"].contains(node.type) {
                TextField(node.type == "script" ? t("录制脚本 UUID","Recording UUID") : node.type == "key" ? t("按键名称，例如 Enter","Key name, e.g. Enter") : t("输入文本","Text"),text:$node.value)
                if node.type == "script", let device, let player = model.players[device], !player.transport.scripts.isEmpty {
                    Picker(t("选择录制脚本","Choose recording"),selection:$node.value) {
                        Text(t("选择脚本","Choose a recording")).tag("")
                        ForEach(player.transport.scripts) { script in Text(script.name).tag(script.id) }
                    }
                }
            }
            if ["tap_element","wait_element","branch"].contains(node.type) { locator }
            if node.type == "wait_element" {
                Toggle(t("等待出现（关闭则等待消失）","Wait for appearance (off: disappearance)"),isOn:$node.present)
            }
            if ["wait_element","retry"].contains(node.type) {
                HStack { field(t("超时（毫秒）","Timeout (ms)"),$node.timeout); field(t("间隔（毫秒）","Interval (ms)"),$node.interval)
                    if node.type == "retry" { field(t("最大次数","Max attempts"),$node.attempts) }
                }
            }
            if ["branch","retry"].contains(node.type) {
                Text(node.type == "branch" ? t("存在时执行","When present") : t("重试内容","Retry body")).font(.caption.bold())
                AnyView(FlowNodesEditor(model:model,nodes:$node.thenNodes,device:device)).padding(.leading,16)
                if node.type == "branch" {
                    Text(t("不存在时执行","When absent")).font(.caption.bold())
                    AnyView(FlowNodesEditor(model:model,nodes:$node.elseNodes,device:device)).padding(.leading,16)
                }
            }
        }
        .padding(10).background(Color.primary.opacity(0.035),in:RoundedRectangle(cornerRadius:8))
        .sheet(isPresented:$capture) { FlowTemplateCapture(model:model,device:device,path:$node.template) }
    }
    private func field(_ title:String,_ value:Binding<String>) -> some View {
        HStack { Text(title).font(.caption); TextField(title,text:value).frame(minWidth:60,maxWidth:110) }
    }
    private var locator: some View {
        VStack(alignment:.leading) {
            Picker(t("识别方式","Recognition"),selection:$node.locatorType) {
                Text(t("Android 控件树","Android UI tree")).tag("ui")
                Text(t("图像模板","Image template")).tag("template")
            }.pickerStyle(.segmented)
            if node.locatorType == "ui" {
                HStack { TextField(t("文本（可选）","Text (optional)"),text:$node.value); TextField(t("Resource ID（可选）","Resource ID (optional)"),text:$node.resourceID) }
            } else {
                HStack {
                    TextField(t("参考 PNG 路径","Reference PNG path"),text:$node.template)
                    Button(t("选取…","Choose…")) {
                        let panel=NSOpenPanel(); panel.allowedContentTypes=[.png]
                        if panel.runModal() == .OK, let url=panel.url { node.template=url.path }
                    }
                    Button(t("截取…","Capture…")) { capture=true }.disabled(device == nil)
                }
                field(t("匹配阈值 0–1","Threshold 0–1"),$node.threshold)
            }
        }
    }
}

private struct FlowTemplateCapture: View {
    @ObservedObject var model: CenterModel
    let device: String?
    @Binding var path: String
    @Environment(\.dismiss) private var dismiss
    @State private var image: NSImage?
    @State private var origin: CGPoint?
    @State private var current: CGPoint?
    @State private var selectionSize: CGSize = .zero
    @State private var error = ""
    private func t(_ zh:String,_ en:String)->String { model.text(zh,en) }
    var body: some View {
        VStack(spacing:12) {
            Text(t("拖动选择参考区域", "Drag to select a reference region")).font(.headline)
            if let image {
                GeometryReader { geometry in
                    let size = fitted(image.size,geometry.size)
                    ZStack(alignment:.topLeading) {
                        Image(nsImage:image).resizable().frame(width:size.width,height:size.height)
                        if let origin, let current {
                            Path { $0.addRect(rect(origin,current)) }.stroke(Color.accentColor,lineWidth:2)
                        }
                    }
                    .frame(width:size.width,height:size.height)
                    .contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance:0).onChanged { event in
                        selectionSize = size; origin = clamped(event.startLocation,size); current = clamped(event.location,size)
                    })
                    .frame(maxWidth:.infinity,maxHeight:.infinity)
                }
            } else { ProgressView() }
            Text(error).foregroundStyle(.red).font(.caption)
            HStack { Button(t("取消","Cancel")) { dismiss() }; Spacer(); Button(t("保存区域…","Save region…"),action:save).disabled(origin == nil || current == nil) }
        }.padding(20).frame(width:700,height:650).onAppear(perform:load)
    }
    private func fitted(_ image:CGSize,_ box:CGSize)->CGSize {
        let scale=min(box.width/image.width,box.height/image.height); return CGSize(width:image.width*scale,height:image.height*scale)
    }
    private func clamped(_ p:CGPoint,_ size:CGSize)->CGPoint { CGPoint(x:min(max(0,p.x),size.width),y:min(max(0,p.y),size.height)) }
    private func rect(_ a:CGPoint,_ b:CGPoint)->CGRect { CGRect(x:min(a.x,b.x),y:min(a.y,b.y),width:abs(a.x-b.x),height:abs(a.y-b.y)) }
    private func load() {
        guard let device else { return }
        let url=URL(fileURLWithPath:NSTemporaryDirectory()).appendingPathComponent("miko-flow-\(UUID().uuidString).png")
        model.call("media.screenshot",["device":device,"path":url.path]) { result in
            defer { try? FileManager.default.removeItem(at:url) }
            switch result { case .success: image=NSImage(contentsOf:url); if image == nil { error=t("无法读取截图","Could not read screenshot") }
            case .failure(let e): error=e.localizedDescription }
        }
    }
    private func save() {
        guard let image, let origin, let current, let cg=image.cgImage(forProposedRect:nil,context:nil,hints:nil) else { return }
        // The image fits the GeometryReader without letterboxing inside its gesture area.
        let shown=selectionSize, selection=rect(origin,current)
        guard shown.width > 0, shown.height > 0 else { return }
        let crop=CGRect(x:selection.minX/shown.width*CGFloat(cg.width), y:selection.minY/shown.height*CGFloat(cg.height),width:selection.width/shown.width*CGFloat(cg.width),height:selection.height/shown.height*CGFloat(cg.height)).integral
        guard crop.width > 0, crop.height > 0, let part=cg.cropping(to:crop), let data=NSBitmapImageRep(cgImage:part).representation(using:.png,properties:[:]) else { error=t("请选择有效区域","Select a valid region"); return }
        let panel=NSSavePanel(); panel.allowedContentTypes=[.png]; panel.nameFieldStringValue="template.png"
        guard panel.runModal() == .OK, let url=panel.url else { return }
        do { try data.write(to:url,options:.atomic); path=url.path; dismiss() } catch { self.error=error.localizedDescription }
    }
}
