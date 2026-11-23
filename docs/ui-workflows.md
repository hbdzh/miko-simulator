# 界面状态流程

在设备中心选择「界面流程」，添加动作、固定等待、元素等待、条件分支或有限重试节点。分支和重试中可以继续添加节点；坐标使用当前屏幕的 0–1 比例。录制脚本节点按 UUID 调用已有操作录制，并解析其中的脚本片段。打开播放器后，编辑器也可从该设备已加载的脚本中选择。

UI 树条件支持按文本、Resource ID 或二者同时精确匹配。点击元素时会重新读取界面并使用最新位置；多个元素匹配时停止并报告错误。图像条件可选择本地 PNG，或在编辑器中从设备截图拖选区域保存为 PNG，设置 0–1 的匹配阈值。模板按截图的原始像素尺寸匹配；分辨率、方向或界面缩放改变后可能需要重新截取。多个不重叠的匹配结果会报错。

勾选多台运行中的设备后，每台设备独立运行和判断界面。可选择启动时暂停，并使用暂停、继续、单步或停止；暂停在下一个节点生效。手动操作接管设备时会取消该设备的流程，并等待已经按下的触点和按键释放后交接。流程不会通过输入同步器广播条件判断或动作。

流程运行结果默认写入 miko 日志目录的 `flows/<运行编号>/report.json`，包括节点、耗时和错误；勾选截图后，节点截图保存在同一目录。截图可能包含屏幕上的个人信息，请按需开启和清理。编辑器保存前检查结构并使用原子写入；引用的模板路径和录制脚本需在运行机器上可用。

启动请求被拒绝或连接设备失败时，也会记录失败报告，节点标记为 `startup`，错误详情包含 `log_dir`；如果日志本身无法写入，错误详情会给出 `log_error`。有限重试可重复执行遇到可恢复错误的动作，例如暂时找不到引用的脚本；输入注入或释放失败会立即终止，不会自动重试。

设备连接短暂中断时，正在读取界面或尚未发送输入的节点会尝试在 30 秒内重新连接同一设备会话，然后继续；元素等待仍受该节点的超时限制。已开始发送输入的动作结果可能不确定，会停止并记录错误，不会自动重复。设备停止或重新启动后需重新启动流程。

CLI 可读取编辑器保存的 JSON：

```sh
miko flow run workflow.json --device Phone --device Tablet
miko flow status Phone
miko flow control Phone pause   # resume、step、stop 也可使用
miko jobs list --all
miko jobs wait JOB_ID
miko jobs cancel JOB_ID
```

`flow run` 返回各设备的任务编号，任务在 daemon 中独立运行；如果部分设备未能启动，错误数据包含各设备的结果，包括已启动任务的编号。MCP 可用 `command_run` 调用 `flow.validate`、`flow.run`、`flow.status`、`flow.control` 和 `jobs.get`，并通过 `jobs_list`、`jobs_cancel` 查询或取消任务。具体字段见[控制协议](control-protocol.md)。

下面是一个等待并点击控件的流程示例：

```json
{
  "name": "打开下一步",
  "nodes": [
    {"type":"wait_element", "locator":{"type":"ui", "text":"下一步", "resource_id":null}, "present":true, "timeout_ms":10000, "interval_ms":500},
    {"type":"action", "action":{"type":"tap_element", "locator":{"type":"ui", "text":"下一步", "resource_id":null}}}
  ]
}
```
