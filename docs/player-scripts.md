# 播放器操作录制（v1）

状态：播放器支持动作编辑、组合和快捷键；也可通过界面状态流程从 CLI/MCP 调用录制脚本。

该功能以类似 MuMu 的方式记录用户归一化输入及时间，将时间线保存为 JSON，并在播放器桥中本地回放。脚本是数据，不是代码。

## 范围（v1）

- 入口仅为播放器窗口。
- 记录触摸、映射触摸、按键按下/抬起、文本、粘贴和工具键。
- 不记录旋转、录屏、截图、文件操作、复制、剪切、全选、应用检查和卸载。
- 回放可设置次数、时长、无限循环、循环间隔和速度（0.25x–4x）。
- 关闭播放器窗口会停止录制和回放，桥进程随之退出。
- 脚本存放在 `<Miko data>/scripts/`，使用可移植 JSON 文件。
- v1 不修改 daemon、CLI、MCP 或控制协议。

## 脚本格式

```json
{
  "format": "miko.operation-recording",
  "version": 3,
  "id": "uuid",
  "name": "每日签到",
  "created_at": "2026-11-12T01:00:00.000Z",
  "device": "Phone 6.1 FHD+",
  "display": { "width": 576, "height": 1280 },
  "playback": {
    "mode": "count",
    "count": 1,
    "duration_ms": 0,
    "interval_ms": 0,
    "speed": 1.0
  },
  "actions": [
    { "type": "gesture", "id": null, "samples": [
      { "t": 0, "order": 0, "value": { "x": 0.5, "y": 0.4, "down": true } },
      { "t": 80, "order": 1, "value": { "x": 0.5, "y": 0.4, "down": false } }
    ] },
    { "type": "key", "key": "ENTER", "samples": [
      { "t": 300, "order": 2, "value": true },
      { "t": 340, "order": 3, "value": false }
    ] },
    { "t": 600, "order": 4, "type": "text", "text": "hello" },
    { "t": 900, "order": 5, "type": "tool_key", "key": "back" }
  ]
}
```

规则：

- `t` 是从开始计算的毫秒数；重叠动作按时间和 `order` 恢复事件顺序。手势包含触点轨迹，按键动作包含按下与抬起样本。
- 版本 2 的 `events` 在导入时转换为动作，导入不会修改源文件；新保存的版本 3 文件只包含 `actions`。含脚本片段引用的文件使用版本 4。编辑器保留动作列表，校验和回放按需将动作展开为输入事件。
- 旧版 miko 只认识版本 2，不能读取新保存的版本 3 文件；如果需要退回旧版，应保留原来的版本 2 导出文件。
- 触摸坐标使用显示屏幕的归一化坐标（0…1），与桥的 `touch` 命令相同；`mapped_touch` 的 id 为 3…9。
- 名称为 1…80 字符；每个脚本包含 1…200,000 个事件，最长 24 小时。
- `playback` 是上次使用的默认值；界面可在每次运行时覆盖。
- 脚本片段以脚本 ID 引用同一脚本库中的其他录制。每次调用执行一遍片段时间线，循环和速度由外层脚本控制。删除引用的片段会使组合回放报错。导出不会打包被引用的片段，普通导入会生成新 ID，因此跨脚本库转移组合后需要重新添加片段引用。回放前展开片段，嵌套深度超过 10 直接报错。
- 显示尺寸仅用于兼容性提示，归一化坐标仍映射到当前屏幕。
- 导入或读取高于当前程序支持版本的脚本时会提示升级；保存不会覆盖无法读取的同 ID 脚本。保存前先校验，再通过同目录临时文件替换。

## 核心 crate API（`crates/miko-automation`）

纯 Rust，不依赖界面或 daemon。桥使用的公开 API：

- `Script`、`Event`（由 `type` 标记）、`TimedEvent`、`DisplayInfo`、`PlaybackOptions`、`PlaybackMode`、`ScriptSummary`。
- `validate(&Script) -> MikoResult<()>`。
- `Recorder`：`new(now)`、`record(event, now)`、`pause(now)`、`resume(now)`、`is_paused()`、`event_count()`、`finish(name, device, display, playback, now)`。结束时补充仍按住的触摸和按键的释放事件并验证。
- `ScriptStore::new(data_dir)`：`list()`、`load(id)`、`save(&script)`、`delete(id)`、`rename(id, name)`、`import(path)`、`export(id, path)`。
- 错误使用 `miko_core::MikoError`（`usage` / `not found` 语义）。

## 播放器桥协议扩展

播放器桥是 Swift 播放器启动的 Rust 进程：stdin 接收 NDJSON 命令，stderr 发送 JSON 通知。桥不依赖特定界面，以便未来 Windows/HarmonyOS 界面复用。

### 命令（stdin）

```text
{"type":"script_record_start"}
{"type":"script_record_pause"}
{"type":"script_record_resume"}
{"type":"script_record_stop","name":"..."}
{"type":"script_record_cancel"}
{"type":"script_list"}
{"type":"script_delete","id":"..."}
{"type":"script_rename","id":"...","name":"..."}
{"type":"script_actions_get","id":"..."}
{"type":"script_action_delete","id":"...","revision":"...","index":0}
{"type":"script_action_wait","id":"...","revision":"...","index":0,"wait_ms":500}
{"type":"script_action_move","id":"...","revision":"...","from":0,"to":1}
{"type":"script_action_gesture","id":"...","revision":"...","index":0,"x":0.2,"y":0.3,"end_x":0.7,"end_y":0.8,"duration_ms":500}
{"type":"script_action_call","id":"...","revision":"...","script":"fragment-uuid"}
{"type":"script_import","path":"/abs/path.json"}
{"type":"script_export","id":"...","path":"/abs/path.json"}
{"type":"script_play_start","id":"...","options":{...PlaybackOptions...}}
{"type":"script_play_pause"}
{"type":"script_play_resume"}
{"type":"script_play_stop"}
```

规则：

- 录制和回放互斥，冲突命令返回 `script.busy`。
- 回放开始时先释放此前按住的输入。回放期间，新的手动按下或其他操作会停止回放并释放脚本输入，然后执行手动操作；此前输入残留的抬起事件不会中断回放。
- v1 不允许旋转：不会录制旋转，包含旋转的脚本无效。
- 停止、取消、暂停或结束时释放仍按住的按键和触摸。播放器窗口失去焦点时会停止回放并释放输入。
- 回放输入注入失败时停止回放、尝试释放已按住的输入，并报告失败；只有本轮输入成功注入后才报告回放结束。
- 动作查询返回一次性编辑版本；每次成功修改后返回新版本。版本过期或读取后脚本内容发生变化时拒绝修改，需重新读取。修改等待不会切断重叠的手势或按键；不允许删除脚本中最后一个动作。

### 通知（stderr，`MIKO_SCRIPT:` + JSON）

```text
{"op":"record","state":"recording"|"paused"|"progress"|"saved"|"stopped"|"failed","event_count":..., "name":..., "message":..., "script":{summary}}
{"op":"list","scripts":[{summary...}]}
{"op":"actions","id":"...","revision":"...","name":"...","actions":[{"index":0,"kind":"key","detail":"ENTER","start_ms":300,"end_ms":340,"wait_ms":300}]}
{"op":"play","state":"playing"|"paused"|"finished"|"stopped"|"failed","loop":1,"count":3,"name":...,"message":...}
{"op":"error","code":"script.busy","message":"..."}
```

`message` 为英文回退文本（Swift 界面可翻译常见错误码）。摘要包含 `id,name,duration_ms,event_count,action_count,created_at,display`。`event_count` 统计全部录制样本；`action_count` 将一次触摸手势或按键归为一个动作，每条文本、粘贴或工具键命令各算一个动作。回放时间线由动作展开，保留每个样本的时间与顺序。

### 回放引擎

回放状态属于桥主循环，与用户输入使用同一控制器，因而共享按住和释放处理。约每 5 毫秒推进到期事件，同时保持对画面和暂停、继续、停止命令的响应。事件时间除以 `speed`，循环间隔使用真实时间。开始、循环、暂停、继续、停止和结束时发送 `play` 通知。

## Swift 界面

- 播放器侧栏提供「操作录制」，以非模态窗口打开，录制时仍可使用播放器。
- 窗口提供紧凑列表、原生新建录制工具栏操作、直接播放、按需显示的播放设置，以及重命名、删除和导入/导出；播放设置保存在本机。
- 录制列表的更多菜单提供动作列表，可删除、拖拽排序、修改动作前等待及手势起点、终点和时长，并可在末尾添加其他脚本片段。重叠动作会作为一组移动，不能单独修改时长；修改立即保存。
- 录制列表可分配播放器窗口内的 `⌃⌥1` 至 `⌃⌥9`，或全局的 `⌃⌥⇧1` 至 `⌃⌥⇧9`；键位方案也可绑定脚本。全局快捷键在 miko 运行且目标播放器打开时生效，同一组合只能绑定一个设备和脚本。切到其他应用时使用全局快捷键需要在系统设置中允许辅助功能，快捷键也可能被当前应用接收。回放期间动作列表高亮当前动作，失败时保留失败位置。
- 运行中的设备可作为录制和播放目标；必要时打开播放器，停止录制后直接保存为默认名称。
- Android 屏幕顶部的浮动控件在录制时显示用时和事件数，播放时显示当前循环，并提供暂停、停止。窗口和设备视图尺寸不变，控件只占少量屏幕空间；桥会合并录制计数更新。
- 关闭播放器会随桥退出自然停止所有操作；所有文案通过现有播放器本地化工具提供中英文。
