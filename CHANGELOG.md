# Changelog

## [0.14.0](https://github.com/hbdzh/miko-simulator/compare/v0.13.1...v0.14.0) (2026-11-24)


### Features

* **automation:** add the operation-recording script library ([b826340](https://github.com/hbdzh/miko-simulator/commit/b826340180f5ee3beab5d7379d6e4922d5e24249))
* **bridge:** expose app inspection and uninstall to the player ([9d05063](https://github.com/hbdzh/miko-simulator/commit/9d0506313ccf2961fd29d43a5ba6d6ef867e7706))
* bundle Magisk for offline full root ([8afa02e](https://github.com/hbdzh/miko-simulator/commit/8afa02ea5dad7c8dc424cb324b4d13697db69b94))
* **center:** consolidate native device management and tools ([4a03a09](https://github.com/hbdzh/miko-simulator/commit/4a03a098c4f7f544a7ee39abf003a0b7b091bb14))
* **center:** make device details an on-demand inspector ([19d6af6](https://github.com/hbdzh/miko-simulator/commit/19d6af6aea98a73b1265a5ae05fb740cbe97c4c4))
* **center:** refine device card and list layout ([6ae5f5c](https://github.com/hbdzh/miko-simulator/commit/6ae5f5c4a095cce27cfa3e7273777e1d9d48915e))
* **center:** refine device management and native tools ([871f6d0](https://github.com/hbdzh/miko-simulator/commit/871f6d030e33e58142baa1512ba1c532d35f1baa))
* **center:** refine device management and native tools ([95abdae](https://github.com/hbdzh/miko-simulator/commit/95abdae0ff5fb8f68af48d6f65cfbd69bfd48762))
* **core:** extend rooted device pools and network simulation ([38cc062](https://github.com/hbdzh/miko-simulator/commit/38cc062cc55841fee2bd51bb7018af66a2c329af))
* **daemon,widgets,root:** integrate network shaping, identity, gamepad, flash and audio recording ([d3eedbc](https://github.com/hbdzh/miko-simulator/commit/d3eedbc9cd071f57e2638ee7ee8d0b926f167c41))
* **daemon:** manage images and validate quick boot saves ([5f67ae4](https://github.com/hbdzh/miko-simulator/commit/5f67ae43ad6cd04c97f6666a7ace556993f3aad0))
* **gui:** add an installable SwiftUI test package ([ad96cc0](https://github.com/hbdzh/miko-simulator/commit/ad96cc02e6e6621d8b89b587de5d9ebf72b1f5a9))
* **gui:** add SwiftUI prototype ([b46a2f3](https://github.com/hbdzh/miko-simulator/commit/b46a2f32da6002d589b99c1c7aa4a1541850915a))
* **gui:** add the player operation-recording window and HUD ([36f35e5](https://github.com/hbdzh/miko-simulator/commit/36f35e519054428f86408e00a4ce4a63ee8d47d3))
* **gui:** adopt the refreshed palette and shared design kit ([8b93795](https://github.com/hbdzh/miko-simulator/commit/8b937958fe976b3d08c47df68495a1c7103626a9))
* **gui:** choose the SDK install location in setup ([0c22527](https://github.com/hbdzh/miko-simulator/commit/0c225276be81d8663ab14b01c9f89b80cd33343a))
* **gui:** finish the token migration and resolve the bundled fonts ([0a6ab34](https://github.com/hbdzh/miko-simulator/commit/0a6ab34a52d01e24b26cab9414f5ac44456cf4ba))
* **gui:** generate and apply the Workbench design tokens ([9e59b75](https://github.com/hbdzh/miko-simulator/commit/9e59b75cc8a0755c838fe47420457369b37114de))
* **gui:** improve key mapping and device management ([ed45037](https://github.com/hbdzh/miko-simulator/commit/ed4503795cd4ddc05169d648273f7d36ecb7be54))
* **gui:** improve setup, panels and settings ([8015314](https://github.com/hbdzh/miko-simulator/commit/8015314b8e0a27d1504d5a56d2731c91fc88c297))
* **gui:** keyboard reachability and basic accessibility ([30279e3](https://github.com/hbdzh/miko-simulator/commit/30279e3af4a8448df6ded54eac8f0ca0c8bcb5fe))
* **gui:** native menus, single instance and compact multi-window layout ([dbec801](https://github.com/hbdzh/miko-simulator/commit/dbec801cf0d631e9125a0c4a651b0909517b03eb))
* **gui:** player window states, gestures, shortcuts and app uninstall ([dff2601](https://github.com/hbdzh/miko-simulator/commit/dff2601efc73d4228cb7905f849e8570418cd1c2))
* **gui:** reconnect, task centre, command palette and shortcuts ([ec8c063](https://github.com/hbdzh/miko-simulator/commit/ec8c063dd09f5001369c71af56a153be4026285a))
* **gui:** refine device center and player workflows ([a26e002](https://github.com/hbdzh/miko-simulator/commit/a26e00219b84b647678a6c2d89febe385218481f))
* **gui:** refine device center and simulator windows ([119693f](https://github.com/hbdzh/miko-simulator/commit/119693f6a7f163657a686deba40892ec6fa18b0d))
* **gui:** refresh the device centre, settings and sheets ([f60535d](https://github.com/hbdzh/miko-simulator/commit/f60535dd9c5eaa9e6c142f702f8472f170da5907))
* **gui:** root timeline, snapshot dialogs and advanced tools ([ba69547](https://github.com/hbdzh/miko-simulator/commit/ba69547fd3c4e3e305e2daebdf4f965e853c12b3))
* **player-bridge:** record and replay operation recordings ([53a503d](https://github.com/hbdzh/miko-simulator/commit/53a503d0cef7c7e3aa982c00dd75c6f20cbde3d2))
* **player:** drop the shortcuts and uninstall sidebar buttons ([4cf2721](https://github.com/hbdzh/miko-simulator/commit/4cf272151b7388088c7abe96779de23ce9c1eb23))
* **player:** move advanced tools into device settings ([e2b3c47](https://github.com/hbdzh/miko-simulator/commit/e2b3c47f68b740741ec7a71993ff8c4b1d6233f3))
* **player:** refine the emulator window chrome ([0df285a](https://github.com/hbdzh/miko-simulator/commit/0df285a8aa55001bbd8e089e08c2b18f6f0a1fcf))
* **player:** remove the Logcat strip from the device window ([0c06fe9](https://github.com/hbdzh/miko-simulator/commit/0c06fe9c53bd967fe00a898598879aefbd6cc5e6))
* prepare miko simulator 0.10.0 ([ab07ca0](https://github.com/hbdzh/miko-simulator/commit/ab07ca05030701d6d4a7fbcaa9676bd360816561))
* **release:** default to the SwiftUI centre and keep the Slint fallback ([2b92a8d](https://github.com/hbdzh/miko-simulator/commit/2b92a8d7e9ce82da55077389bd81787eab691fe4))
* rename desktop application to miko simulator ([d72ea24](https://github.com/hbdzh/miko-simulator/commit/d72ea245e98cab473acba5c63f71eafd2b986e92))
* require bundled emulator runtime ([c948c60](https://github.com/hbdzh/miko-simulator/commit/c948c605550716d6863cc450d4a28590b96dba99))
* **sdk:** allow a custom writable SDK install root ([6bec82d](https://github.com/hbdzh/miko-simulator/commit/6bec82de805d6599bfc53a1c9e2185028890f3dc))
* **sync,gui:** add bidirectional folder sync and dedicated sync panel ([e80c7b4](https://github.com/hbdzh/miko-simulator/commit/e80c7b4bf1526002bec82bf54a28561916bdb8df))
* 增强手柄映射与设备界面，升级至 0.13.0 ([b9a8417](https://github.com/hbdzh/miko-simulator/commit/b9a8417cf99fafe2002a6c520e9febef9cab544e))
* 完善多设备管理与界面自动化，升级至 0.12.0 ([0adf786](https://github.com/hbdzh/miko-simulator/commit/0adf7865aa88a09a06bf53820a56c10d56060c15))
* 支持摄像头文件输入并修复横屏同步，升级至 0.13.1 ([b242d8a](https://github.com/hbdzh/miko-simulator/commit/b242d8a32b1b3d3f10e7d231278af0a65c5332cf))
* 更新设备管理并准备 0.11.0 版本 ([a12bbde](https://github.com/hbdzh/miko-simulator/commit/a12bbde2e57af96a860bd1449b21e4da9095efe8))


### Bug Fixes

* **bridge:** satisfy clippy in the diagnostics test ([e7f8a1f](https://github.com/hbdzh/miko-simulator/commit/e7f8a1f100c0c2ccbb7c835f63a86e9e48222196))
* **center:** support older macOS SDKs ([6e08a96](https://github.com/hbdzh/miko-simulator/commit/6e08a9624fd34409e8a96b3a1bcc28c9588ab915))
* **core:** improve device lifecycle and cancellation ([975fd38](https://github.com/hbdzh/miko-simulator/commit/975fd3833fdecf2a3594e454be767825834c1cbd))
* correct release artifact paths and audit runtime ([907c647](https://github.com/hbdzh/miko-simulator/commit/907c6471ef0538c746763a55683f1a635f4ce1a2))
* **gui:** classify root patch failures from the error text ([6283a29](https://github.com/hbdzh/miko-simulator/commit/6283a290a2049c22b180bafc8ce7fcdcf37dada1))
* **gui:** close the English-mode localization leaks ([ed5ac9e](https://github.com/hbdzh/miko-simulator/commit/ed5ac9e7d524f1008ab2fd755b7f8aa8eb5b0624))
* **gui:** isolate input RPCs from screenshot transport flow control ([c8f7dab](https://github.com/hbdzh/miko-simulator/commit/c8f7dab144f7d88a047e330d86cd8a8442747337))
* **gui:** make the device list selection and rows native ([94ed32c](https://github.com/hbdzh/miko-simulator/commit/94ed32c101911465758167273a88b4a6b98be9d2))
* **gui:** pass clippy for the SwiftUI bridge ([5a4d8eb](https://github.com/hbdzh/miko-simulator/commit/5a4d8eba7ac4a9abd447d190e716458c518b2ef2))
* **gui:** refine native windows and verify device deletion ([7fab010](https://github.com/hbdzh/miko-simulator/commit/7fab01047e111253a54510c92eedba3be98928ec))
* **gui:** restore native macOS button styles ([b32b04f](https://github.com/hbdzh/miko-simulator/commit/b32b04f392837e1993dba291a5d62e25e370d409))
* **gui:** restore native settings sidebar selection and clean up the command palette ([60c9f96](https://github.com/hbdzh/miko-simulator/commit/60c9f9653a98e1dcd5cec7c3daf514f6508a8790))
* **gui:** show the detected external SDK, not the Jidr install root ([cf3d05e](https://github.com/hbdzh/miko-simulator/commit/cf3d05efb11398e6f1658c1ee51ba30a74e964f0))
* **gui:** simplify empty library and restore app branding ([443fe25](https://github.com/hbdzh/miko-simulator/commit/443fe259bf46cecb0b7b3d0d931c649d45742ab3))
* **gui:** update remaining product names ([0303d04](https://github.com/hbdzh/miko-simulator/commit/0303d0411fcd8e2fec537000561810b2180888fb))
* **packaging:** include stopped-device artwork in app bundles ([0cf7f52](https://github.com/hbdzh/miko-simulator/commit/0cf7f52761d8e3d25d11e8b62c02902525bfd8a0))
* **perf:** preserve failed probes and await isolated cleanup ([07172b7](https://github.com/hbdzh/miko-simulator/commit/07172b7c04b8232bd1e8d74add0a52893e03beb1))
* **root:** extract ramdisks from Android boot images ([dad2045](https://github.com/hbdzh/miko-simulator/commit/dad20452b0cb8301411e3a2e0805f78947cdf73f))
* **sdk:** improve migration integrity checks ([18b1722](https://github.com/hbdzh/miko-simulator/commit/18b17221d995e4efa504bb0c1503136bde29576e))
* **widgets:** reject incomplete snapshots ([e60bc90](https://github.com/hbdzh/miko-simulator/commit/e60bc905816597d4025396c2eec1efc52d332a9a))


### Performance Improvements

* **center:** cache loaded images and trim frame-header allocations ([b5a79df](https://github.com/hbdzh/miko-simulator/commit/b5a79dff74b447030663e1fe6f18f017366422c6))
* **center:** coalesce device refreshes and parse off the main thread ([8cd0e0d](https://github.com/hbdzh/miko-simulator/commit/8cd0e0dd861fb1689dd91113a5df046a275eddaa))
* **gui:** schedule input independently of frame reception ([53073ae](https://github.com/hbdzh/miko-simulator/commit/53073ae1ae0a2810933e6053ec3e32aa8ecc9eea))
* **gui:** share frame buffers and skip work under backpressure ([b314318](https://github.com/hbdzh/miko-simulator/commit/b314318cbeaa9cbdca84f7db82623e52e280e87b))
* **player:** skip frame image work while the window is hidden ([a4fd3fd](https://github.com/hbdzh/miko-simulator/commit/a4fd3fda5283967540c54c3447c0808f389fbac3))

## 0.13.1 — miko 模拟器 (2026-11-24)

### 新增

- 前后摄像头可分别使用本地图片或视频作为输入，可在设备工具、CLI 和 MCP 中设置，下次启动生效。

### 修复

- 修复部分本地摄像头文件路径无法正确传递给模拟器的问题。
- 修复横屏平板在等比同步时，跟随设备的触控坐标可能偏移的问题。
- 明确指纹模拟中的已登记和未登记指纹操作。

## 0.13.0 — miko 模拟器 (2026-11-23)

### 新增

- 新增手柄触控映射，可将手柄按键绑定到触点、连点、滑动、视角与技能操作，并按手柄和应用调整摇杆、扳机参数。
- 原生手柄支持 Android `uinput` 和震动反馈；支持的环境不可用时回退至 `hid`，手柄电量和反馈取决于设备及系统支持。
- 多设备同步支持转发手柄触控映射和原生手柄输入，并显示输入冲突与受影响设备的错误。

### 优化

- 重新整理高级键位侧栏，可直接选择动作类型、调整按键行为和方案，并在编辑时切换到画面操作。
- 重排全部设备页面，收拢内存预算、设备分组和批量操作，支持从设备卡片直接打开配置。
- 重构界面状态流程编辑器，将节点编排与运行、调试配置分开，支持多设备选择及节点步骤展示。
- 统一设备中心的卡片、下拉框和分段控件样式，改进手柄连接状态、断连恢复和输入释放处理。

### 说明

- 部分手柄、震动、快速启动及多设备同步行为仍需在真实设备和游戏中验证。

## 0.10.0 — miko 模拟器 (2026-11-20)

### 环境与安装

- 发行包内置模拟器运行时和 platform-tools，无需安装 Android Studio、手动下载工具或选择外部 SDK。
- 系统镜像按需下载：首次设置完成后，可在创建设备时选择已有镜像或下载所需镜像。
- 可在首次设置时选择存储位置，设备、私有 SDK、缓存和日志统一保存在 miko 的数据目录中。
- DMG 不包含 Android 系统镜像；下载新镜像需要联网。

### 新增

- 更新 miko 应用图标，并同步更新 MCP 扩展图标。
- 设备搜索支持查找正在运行设备中已安装的应用。

### 优化

- 改进环境准备和镜像选择流程，清晰区分运行工具与系统镜像，支持在设置时调整存储目录并查看迁移进度。
- 改进存储迁移对自定义和历史 SDK 目录的处理，保留已有镜像的发现能力，并增加目录重叠检查和迁移错误提示。
- 播放器窗口不可见时跳过画面像素缓冲，减少不必要的处理；改进设备中心的后台请求和应用搜索，减少界面等待。
- 改进日志错误统计和异常画面处理，并在快速启动遇到不支持保存的 Vulkan 应用时给出具体提示。
- 更新应用、命令行和后台组件的 miko 命名，统一设备数据、工具和配置的名称。

### 修复

- 安装 SDK 组件时，取消操作会等待解压结束再清理；覆盖安装失败时尝试恢复原有组件。
- 模拟器已经自行退出时正确完成停止清理；缺少音频输入设备时阻止可能导致模拟器崩溃的音频注入。
- 停止设备或退出应用时，如果快速启动快照保存失败，会提示并保持设备运行；检测到上次异常退出或快照不完整时显示提示。

### 升级与安装

- 适用于 Apple Silicon Mac，要求 macOS 13 或更高版本。打开 DMG，将 miko simulator 拖入“应用程序”。
- 从 0.9.0 升级：数据目录改为 `~/Library/Application Support/miko`，旧数据和配置不会自动迁移；命令行改为 `miko`，环境变量改为 `MIKO_` 前缀。旧版操作录制文件不能直接使用，依赖旧 JSON 字段的脚本需要更新。
- 快速启动快照原地保存失败时，旧快照仍可能受损。

## 0.9.1 — miko 模拟器 (2026-11-20)

### 修复

- 停止设备或退出应用时，如果快速启动快照保存失败，会显示提示并保持设备运行，方便释放空间后重试。
- 检测到上次设备异常退出或快速启动快照不完整时，在设备启动后显示提示。

### 优化

- 将应用、命令行和内部组件统一为 miko 模拟器，包括后台服务、播放器桥接程序、设备数据目录和配置名称。
- 检查快照保存时的磁盘空间和可用内存；资源较少时显示提示。
- 更新命令行 JSON 输出、操作录制格式、设备池及休眠快照名称，并同步更新文档和测试。

### 升级说明

- 数据目录改为 `~/Library/Application Support/miko`。0.9.0 的数据和配置不会自动迁移，需重新配置设备与工具。
- 命令行改为 `miko`，相关环境变量改为 `MIKO_` 前缀；旧版操作录制文件无法直接使用；依赖旧 JSON 字段的脚本需要更新。

### 安装

- 适用于 Apple Silicon Mac，要求 macOS 13 或更高版本。打开 DMG，将 miko simulator 拖入“应用程序”。

## 0.9.0 — miko 模拟器 (2026-11-20)

### 新增

- 重新设计播放器键位编辑：支持在画面中放置、拖动和重绑触控键位，添加 WASD 或方向键虚拟摇杆，并提供 MOBA、射击和街机模板。
- 支持创建、复制、重命名、导入和导出多个键位方案；编辑时可调整标记透明度，并在关闭时提示未保存的修改。
- 播放器增加游戏手柄连接和断开入口，显示连接状态。

### 修复

- 键位方案损坏时保留原文件，提供重新加载、在访达中查看或重置后编辑的选项；保存前检查未绑定的选点和按键冲突。
- 设备数据迁移完成并验证新位置后清理旧目录；清理前检查当前存储位置和目录重叠，失败时提示手动清理。

### 优化

- 调整设备工具箱、Root 模式和系统分区操作的布局与文案，将设备配置入口和相关状态整理得更清晰。
- 改进播放器键位标记、窗口尺寸及全屏编辑交互，补充键位映射测试。
- 将小屏手机配置调整为 5.5 英寸 FHD（1080 × 1920、420 dpi），整理设备规格顺序并保留 6.1 英寸机型作为默认配置。

### 已知问题

- 快速启动快照原地保存失败时，旧快照仍可能受损。

## 0.7.0 — miko 模拟器 (2026-11-18)

### 新增

- **多设备输入同步器（Synchronizer）**：新增 `jidr-sync` 逻辑库、守护进程 `sync.*` 会话控制面（`sync.start` / `stop` / `status` / `add_follower` / `remove_follower` / `set_leader` / `set_options` / `report` 与 `sync.state` / `sync.follower_error` 事件）、CLI `jidr input-sync`、MCP 工具（`sync_start` / `sync_stop` / `sync_status`）、桥接端 `leader`/`follower` 实时转发与坐标适配（`stretch` / `letterbox`），以及液态玻璃风格“同步器”面板（`⇧⌘Y`、命令面板与播放器工具栏入口）、窗口内状态胶囊和锁定跟随端一键接管为主控功能。
- **双向与单向文件夹同步**：新增 `file.sync.start`（支持 `one_way` 与 `bidirectional`）、`file.sync.stop`、`file.sync.list`、`file.sync.resolve` 协议命令与 `file.sync.*` 事件，CLI 同步新增 `jidr sync start [--bidirectional]`、`list`、`stop` 与 `resolve`。双向模式通过 SHA-256 内容基线与设备端扫描同步两端更改与删除，冲突时保留双方内容供手动选择（`keep_local` / `keep_remote` / `keep_both`）。
- **独立文件夹同步面板**：在设备概览页操作栏、菜单栏「设备」及命令面板（`⌘K`）新增“文件夹同步”入口，支持在独立面板中切换双向/单向方向、可选自定义设备目录、查看实时 `↑pushed ↓pulled` 计数、在访达中显示目录以及一键处理冲突。
- **网络丢包与 DNS 模拟增强**：`network.shaping.set` 新增 `dns`（`normal` / `slow` / `fail`）与 `dns_delay_ms` 参数，配置持久化到设备并在重启后恢复；同步更新 CLI（`jidr network dns`）与 SwiftUI 工具页。
- **设备标识、手柄转发与刷机分类**：新增 `identity.get` / `identity.set`（读取 Android ID / IMEI、Root 修改 Android ID）、`input.gamepad.*`（转发 Mac 游戏手柄到设备）及 `file.flash`（安装受支持的 Magisk 模块或复制普通 ZIP，拒绝 recovery 与分区镜像）。
- **录屏音频支持**：`media.record` 新增可选 `audio` 参数（默认开启），并回报是否包含音轨及是否接收到有效音频包。

### 修复

- 修复单向同步删除宿主目录时可能误删设备端自有或忽略文件的问题，改为仅移除本次同步记录过的文件；文件状态读取失败不再直接视为删除；首次镜像与增量传输失败后自动重试；监听器在首次扫描前启动以缩小漏变更窗口。
- 修复文件夹同步面板弹出时与主界面同时闪烁、顶部刷新与添加按钮未对齐及误触发重复选中的问题。

### 优化

- 文件夹同步面板统一采用与“设备设置”一致的原生分组表单、下拉方向选择器与毛玻璃弹窗交互。
- 同步更新协议文档、CLI 文档、MCP 文档、智能体技能文档以及同步、网络、标识、手柄和音视频相关测试。

### 已知问题与验证限制

- 延续 0.6.0 已知问题：快速启动快照原地保存失败时，旧快照仍可能受损；`jidr-daemon --test stop_sync` 中的 `failed_save_preserves_previous_quick_boot` 未通过。
- 本机未安装 `protoc`，`jidr-proto` 重编译与全工作区 `--all-features` 测试未在本地通过；`test-center-ui.sh --interactive` 与真实 Android 模拟器实机端到端验收尚未执行。
- 本地或未配置 Developer ID 凭据构建的产物仅使用 ad-hoc 签名且未公证。

## 0.6.0 — miko 模拟器 (2026-11-16)

### 新增

- 新增系统镜像管理：查看各 SDK 中已安装镜像的来源、占用和关联设备，在界面中安装镜像，或删除未被 Jidr 设备使用的 Jidr SDK 镜像。外部 Android SDK 保持只读；删除前会重新检查目录、设备引用和并发操作。
- 新建设备向导支持直接复用现有 SDK 和已安装镜像，也可按需安装工具或其他镜像；支持自定义分辨率、设备规格、Root 模式、内存、CPU、存储及声音。
- 安装前可查看适用的完整 SDK 许可，包括已同意的许可；只有用户明确同意后才记录许可。安装支持进度、取消、错误提示与重试。
- 新增 `image.inventory`、`image.remove` 和 `sdk.license_details` 控制协议命令，分别用于查询镜像副本、删除可安全移除的 Jidr 镜像和查看许可详情。

### 修复

- 启用快速启动的设备停止或退出时，先显式保存并验证快照；保存失败时保持设备运行，避免继续关闭。重新接管的模拟器不会在退出时覆盖已验证的快照；终止前须确认虚拟机已暂停。
- 修复设置窗口从较高页面切回较低页面时可能出现画面重叠、控件无法点击的问题。
- 修复面板切换、关闭与重复打开之间的竞态，避免丢失未保存的设备设置或让后台快捷键误操作设备；安装向导需要前台时防止窗口关闭或意外退出。
- 修复安装任务取消与任务 ID 尚未返回之间的竞态，并避免由安装页面处理的进度覆盖全局提示。
- 修复清理下载缓存时超大天数参数可能造成整数溢出的问题。
- 修复设备选择、搜索和刷新时旧请求可能留下过期详情；删除设备后重新核对设备列表，避免将未生效的删除显示为成功。

### 优化

- 重新设计空设备库引导、环境准备和创建设备流程，改善加载、无镜像、缺少工具、许可确认、安装进度和失败状态。
- 统一设备创建、设备设置、快照、系统镜像、命令搜索和后台任务面板的原生材质、动画、焦点与交互；兼顾减少动态效果设置及深浅色外观。
- 设置窗口按页面内容调整大小，最多占当前屏幕宽高的 70%；内容过多时可滚动，并适配显示器变化。
- 快照按自动和手动分类，展示不可恢复的原因；刷新时保留已有内容，减少概览按钮跳动。
- 改善设备侧边栏选择状态、播放器启动和连接状态、表单输入、工具栏及中英文文案。
- 增加镜像删除保护、许可读取、快速启动保存、窗口交互、面板协调、创建设备和界面截图回归测试。
- 增加可选的内部开发技能与使用说明，覆盖 macOS 界面、SwiftUI/AppKit 和 Rust 开发。

### 待解决

- 快速启动快照原地保存失败时，旧快照仍可能损坏。完整设备级恢复方案尚未集成；相关回归测试未通过。

## 0.5.1 — miko 模拟器 (2026-11-15)

### 设备和网络

- 支持从 Magisk Root 设备创建基础镜像和设备池克隆，克隆保留独立的 Root 配置。
- 新增出站网络丢包模拟，可查看、设置和清除丢包比例；需要 Root 和系统镜像的 netem 支持。
- 改进 Root 冷启动前的关机流程，先正常关闭 Android，再启动新配置。

### 稳定性和界面

- 持久化设备池租约，修复等待启动时租约提前过期及续期与回收竞态。
- 创建基础镜像时保留源设备动画设置，在克隆交付前关闭动画。
- 改进设备中心、播放器窗口、原生命令搜索与设备工具界面，并更新交互及截图测试。

## 0.5.0 — miko 模拟器 (2026-11-15)

### 界面和交互

- 新增全部设备视图，优化设备概览、侧边栏、搜索框、卡片层次与深浅色外观。
- 调整模拟器标题栏、工具栏及操作录制窗口；长设备名自动截断，标题栏控件对齐。
- 优化键位设置和设备工具的原生材质及布局。

### 稳定性

- 关闭正在启动的模拟器窗口时取消启动，无需等待启动完成。
- 设备停止期间重开应用后可正常退出，停止操作继续在后台执行。
- 为模拟器关闭请求增加超时保护，并改进后台状态持久化和客户端消息处理。
- 更新窗口生命周期测试及界面截图基准。

## 0.4.0 — miko 模拟器 (2026-11-14)

### Features

- Replace the device grid/list and separate inspector with a native device sidebar and responsive overview.
- Consolidate APK installation/drop targets, app management and snapshot actions; keep device settings/tools in the header and capture/send actions in a compact row.
- Share native search controls across device search, the command palette and operation recordings.
- Expose writable `/system` configuration in Root tools with eligibility checks, cold-boot confirmation and read-only restoration.
- Add a Microphone page for WAV injection, looping, progress and cancellation, retaining playback controls across tool-page changes.
- Keep camera editing in Device tools and link to it from device settings without discarding other staged changes.

### Maintenance

- Bundle stopped-device artwork in development and release applications, with a packaging regression test.
- Update scoped UI fixtures, screenshot baselines, localization checks and device-tool operation tests.
- Set workspace and release metadata to 0.4.0. Local build/install outputs stay outside version control.

The [0.4.0 release](https://github.com/hbdzh/JIdr-plus/releases/tag/v0.4.0) was rebuilt later from the merged 0.4.0 source; it is not an upstream Jidr release or the original binary. Developer ID signing, notarization, and real-emulator acceptance of the system-write and microphone UI flows remain unverified.

## [0.3.1](https://github.com/AbdurahmanAlmehdi/jidr/compare/v0.3.0...v0.3.1) (2026-11-03)


### Bug Fixes

* **release:** retry hdiutil create when the image is busy ([f6f0be4](https://github.com/AbdurahmanAlmehdi/jidr/commit/f6f0be4fb82e0f0f969fc320347a843c64bd7b38))
* **release:** retry hdiutil create when the image is busy ([3277063](https://github.com/AbdurahmanAlmehdi/jidr/commit/3277063a8c96b7a4ee392cae5ed8d657c65a086b))

## [0.3.0](https://github.com/AbdurahmanAlmehdi/jidr/compare/v0.2.0...v0.3.0) (2026-11-03)


### Features

* **daemon:** bases, pool leases and a reaper for agents (Phase 13d, D42) ([c780dc5](https://github.com/AbdurahmanAlmehdi/jidr/commit/c780dc5355797bbf8d2ee51913d307ed612a9dbb))
* **daemon:** hibernate, auto-hibernate and a working quick-boot save (Phase 13c, D42) ([a186692](https://github.com/AbdurahmanAlmehdi/jidr/commit/a18669279bf4cc6c3205e2d2eec28a548575cf03))
* **daemon:** host RAM budget, lean profile and host.budget (Phase 13a, D42) ([b06e0a6](https://github.com/AbdurahmanAlmehdi/jidr/commit/b06e0a6123da5317f248db6493f2d382580b45dc))
* many devices on one machine (D42, Phases 13a–13d) ([48a8289](https://github.com/AbdurahmanAlmehdi/jidr/commit/48a8289d64a28674fc1f34d4ae20082f32cba7ab))
* **root:** trust CA certificates as system certs on Full-root devices ([68c79b8](https://github.com/AbdurahmanAlmehdi/jidr/commit/68c79b8a01d1c3ece68e351508c606dd6e023427))
* **root:** trust CA certificates as system certs on Full-root devices ([0bf3c20](https://github.com/AbdurahmanAlmehdi/jidr/commit/0bf3c20b4887c1fe634f8937dbfdbd6e94b19450))
* **sdk:** snapshot-keeping copy-on-write clones (Phase 13b, D42) ([3fe675d](https://github.com/AbdurahmanAlmehdi/jidr/commit/3fe675da6ce7cf71da6fbd78700296f836721899))
* **site:** take the download's version, size and checksum from GitHub ([3f5f8d3](https://github.com/AbdurahmanAlmehdi/jidr/commit/3f5f8d3cb1356ca9455807447fbcc542e93f9862))
* **site:** take the download's version, size and checksum from GitHub ([e287b93](https://github.com/AbdurahmanAlmehdi/jidr/commit/e287b9304b828ec634625d16882b21b4b757ae70))
* **skills:** jidr and jidr-app-testing agent skills ([cec4fcb](https://github.com/AbdurahmanAlmehdi/jidr/commit/cec4fcb6b69f0e05744ff27c759d46552399b351))
* **skills:** jidr and jidr-app-testing agent skills ([db26b30](https://github.com/AbdurahmanAlmehdi/jidr/commit/db26b30a3baaa7fca215bdacac8c5134aa71188f))
* **widgets:** set and clear a device's HTTP proxy ([8b44aab](https://github.com/AbdurahmanAlmehdi/jidr/commit/8b44aab0e957d6629e4c06c7569ea3bbebf41d94))
* **widgets:** set and clear a device's HTTP proxy ([340e11d](https://github.com/AbdurahmanAlmehdi/jidr/commit/340e11d9778c5f572c61cc678201c3010a3d54de))


### Bug Fixes

* CI and test failures after the merge batch ([ebb571d](https://github.com/AbdurahmanAlmehdi/jidr/commit/ebb571df793c07fc2ae972cd3759236ebaa5d734))
* **daemon:** save quick-boot state when stopping a resumed run ([4566dde](https://github.com/AbdurahmanAlmehdi/jidr/commit/4566ddecbd33251a1b80a6cb166720696e3d8bdb))
* Full-root su hang recovery, Magisk version text, new-device auto start ([ba9cb20](https://github.com/AbdurahmanAlmehdi/jidr/commit/ba9cb20dfc764cb20b98ae7d475dac422d993daa))
* **packaging:** keep the template placeholders out of the cask's comment ([0a1a9f6](https://github.com/AbdurahmanAlmehdi/jidr/commit/0a1a9f66758b3f7d3a475369c67b5aa5b8c35330))
* **root:** recover a shell su that hangs right after the Full-root grant ([ff1beeb](https://github.com/AbdurahmanAlmehdi/jidr/commit/ff1beeb9160581424c2f71a58ca6e92bee6bcbf9))
* **root:** report the Magisk version without magisk -v's variant suffix ([a69eb44](https://github.com/AbdurahmanAlmehdi/jidr/commit/a69eb4474d929f30fca9b1a19843f1dd0b8de039))
* **root:** take one Zygote pid when pidof prints several ([a344019](https://github.com/AbdurahmanAlmehdi/jidr/commit/a3440198b1d10e5f8b551168bafc5283ca62b5ea))
* **site:** run the site tests on Node 22 ([e4b0892](https://github.com/AbdurahmanAlmehdi/jidr/commit/e4b08925970dbcfc4e066aa8b53570615e1d6171))
* **ui:** map player touches through the display rotation ([b29b53a](https://github.com/AbdurahmanAlmehdi/jidr/commit/b29b53a3dd69565258e629a4b5d23f178d4bd823))
* **ui:** map player touches through the display rotation ([2b01155](https://github.com/AbdurahmanAlmehdi/jidr/commit/2b01155dbabf3028864852729834b66d93628fc5)), closes [#51](https://github.com/AbdurahmanAlmehdi/jidr/issues/51)
* **ui:** start a new Full-root device's setup when the device list arrives ([edcde01](https://github.com/AbdurahmanAlmehdi/jidr/commit/edcde01fae1fb77dc168b6af8d51b9d1aa107e2a))

## [0.2.0](https://github.com/AbdurahmanAlmehdi/jidr/compare/v0.1.0...v0.2.0) (2026-10-18)


### Features

* **site:** blog with two technical guides ([7264e14](https://github.com/AbdurahmanAlmehdi/jidr/commit/7264e1464fcb3f4b632f0c11bb9f5f8397426600))
* **site:** show the brew install and upgrade lines now the cask is in the tap ([3a30c65](https://github.com/AbdurahmanAlmehdi/jidr/commit/3a30c652052c8a192ba313898d445a87f7a62a14))
* **site:** switch jidr.ly to the launch page for v0.1.0 ([094c642](https://github.com/AbdurahmanAlmehdi/jidr/commit/094c642b5c61b338b2a98d10b1b073b8a42ed6ed))
* **site:** switch jidr.ly to the launch page for v0.1.0 ([3fcc5ad](https://github.com/AbdurahmanAlmehdi/jidr/commit/3fcc5ad4ba4ff58779ebb8adf8e07965cdf57a11))
* **ui:** Check for updates compares with the latest GitHub release ([45ee1d6](https://github.com/AbdurahmanAlmehdi/jidr/commit/45ee1d6d63b20673d1c4e83b0695b536e57c5311))
* **ui:** Check for updates compares with the latest GitHub release ([f907b68](https://github.com/AbdurahmanAlmehdi/jidr/commit/f907b685204da59e3ca8c1ebd13480a32d3aa646)), closes [#44](https://github.com/AbdurahmanAlmehdi/jidr/issues/44)

## 0.1.0 (2026-10-18)


### Features

* **backend:** AOSP emulator launch, boot wait and graceful stop (Phase 3b) ([80e15e7](https://github.com/AbdurahmanAlmehdi/jidr/commit/80e15e722962da2149ae69a676d314af2758a27c))
* **backend:** camera options in LaunchOptions and Device.launch (R7, D36) ([732662f](https://github.com/AbdurahmanAlmehdi/jidr/commit/732662f8c40c8de688f64ce6e54b416ad7c84020))
* **backend:** crash detection, re-adopt, double-launch refusal, console client (Phase 3c) ([9c1dfaa](https://github.com/AbdurahmanAlmehdi/jidr/commit/9c1dfaa3203245edb8cce08f0e2de0996b0dccd3))
* build-prop spoofing for Full-root devices (Phase 9f) ([eef1509](https://github.com/AbdurahmanAlmehdi/jidr/commit/eef1509c28987e954f2b611140efb54f47925db7))
* **cli,sdk:** download cache, uninstall --purge and device log bundles (Phase 5b) ([683c2c5](https://github.com/AbdurahmanAlmehdi/jidr/commit/683c2c5ebd83461a94c150f9d6f9941b64e5d33b))
* **cli:** jidr command line over the daemon protocol (Phase 5a) ([b8abad3](https://github.com/AbdurahmanAlmehdi/jidr/commit/b8abad3a59730a41d953daeb664572471fe5c6b2))
* **cli:** register the root provider ([83e4ea3](https://github.com/AbdurahmanAlmehdi/jidr/commit/83e4ea355d579c13b9743bf2c8b2b24e3c761bda))
* **cpio:** ramdisk cpio reader/writer with gzip, lz4-legacy and xz (Phase 9b) ([452a1fa](https://github.com/AbdurahmanAlmehdi/jidr/commit/452a1faedae63fe88f216ac973019a846eb89561))
* daemon settings, byte progress, pristine ramdisk hash, full command reference (B4, B5, G14, G16) ([ca9df9e](https://github.com/AbdurahmanAlmehdi/jidr/commit/ca9df9e53d48bcc905567a83c74683cc49ca8d26))
* daemon settings, byte progress, pristine ramdisk hash, full command reference (B4, B5, G14, G16) ([9203b99](https://github.com/AbdurahmanAlmehdi/jidr/commit/9203b998997f568652f9eed9e6f4d3da1ed5424d))
* **daemon,client:** jobs, events, idle exit and the jidr-client library (Phase 4b) ([e4b1d08](https://github.com/AbdurahmanAlmehdi/jidr/commit/e4b1d0816d3ffbd2701f7fd7a7227970b3b431ab))
* **daemon:** freeze control protocol v0.1 with generated docs and fuzzing (Phase 4c) ([e7f591d](https://github.com/AbdurahmanAlmehdi/jidr/commit/e7f591de20bd4fcc50874ef22206f56ac7a464f5))
* **daemon:** JSON-RPC daemon core, provider contract and device commands (Phase 4a) ([d56990c](https://github.com/AbdurahmanAlmehdi/jidr/commit/d56990c6cbf345643a1e69917fc243180802f663))
* **jidr:** register the widgets provider in jidr daemon (G6) ([83cb597](https://github.com/AbdurahmanAlmehdi/jidr/commit/83cb597bf993c6be5a5c06b1e27da1f9be6f1c90))
* **jidr:** register the widgets provider in jidr daemon (G6) ([d21876f](https://github.com/AbdurahmanAlmehdi/jidr/commit/d21876f8062e5c9b4e087b73636d2a166aa05a31))
* **mcp:** jidr mcp server with generic and typed device tools (Phase 6) ([1a1e20f](https://github.com/AbdurahmanAlmehdi/jidr/commit/1a1e20f627d72201ac1ce2c6855a2a1dcc51124c))
* **proto,adb:** emulator gRPC bindings with vendored fallback and adb wrapper (Phase 3a) ([485ed78](https://github.com/AbdurahmanAlmehdi/jidr/commit/485ed780c6834184102dcbdcf2774a7c7c58663e))
* **release:** Homebrew cask and MCP bundle with every release ([dc6af6d](https://github.com/AbdurahmanAlmehdi/jidr/commit/dc6af6d4c2e275f050d112899874f54fbb494ecf))
* **release:** publish the Homebrew cask and an MCP bundle with every release ([521e05d](https://github.com/AbdurahmanAlmehdi/jidr/commit/521e05debca849667a77a9dfd3f97737057750f9))
* **release:** unsigned macOS app bundle, DMG, checksum and forbidden-content scan (Phase 12a) ([e13a935](https://github.com/AbdurahmanAlmehdi/jidr/commit/e13a935cde2bd8cef0024173409286abc8556ced))
* **root:** adb root mode with live switching, boot hook and verification (Phase 9c) ([285543f](https://github.com/AbdurahmanAlmehdi/jidr/commit/285543fe7093f39e4b4b481076687afe1ed1b34a))
* **root:** device.props.get/set — build-prop spoofing through the Magisk ramdisk (9f) ([c7d3da6](https://github.com/AbdurahmanAlmehdi/jidr/commit/c7d3da673319977a00cdc660c65dc8327f6ab9a5))
* **root:** Magisk mode (Phase 9d) ([cc9d1e9](https://github.com/AbdurahmanAlmehdi/jidr/commit/cc9d1e904324a1794db039fcaea152e83d665457))
* **root:** root.auto_grant for D22 auto-grant all (G15) ([418b583](https://github.com/AbdurahmanAlmehdi/jidr/commit/418b583a95582268c9a96432d94f9fae11ce6878))
* **root:** root.auto_grant for D22 auto-grant all (G15) ([3834178](https://github.com/AbdurahmanAlmehdi/jidr/commit/3834178c4c93034be9f96f8e9ff2fea7dd0700c6))
* **sdk:** core model, error codes, native SDK installer and AVD lifecycle (Phase 2) ([93d494c](https://github.com/AbdurahmanAlmehdi/jidr/commit/93d494c67f5743ac81f2d6fb885be505712a1af2))
* **site:** coming-soon landing page for jidr.ly (Phase 11a) ([92f1185](https://github.com/AbdurahmanAlmehdi/jidr/commit/92f11850fc6f5dc00e9c883665c0454442c21b94))
* **site:** go live as the coming-soon page with links that work ([991f78e](https://github.com/AbdurahmanAlmehdi/jidr/commit/991f78e41a71f1f140409269ecb2aada03637cf1))
* **site:** launch page with download, agents, CI and docs, plus Arabic RTL (Phase 11b) ([f0858e9](https://github.com/AbdurahmanAlmehdi/jidr/commit/f0858e91c5f9a32c78f826d86c013a9cc6e32324))
* **site:** rebuild the coming-soon page to the Workbench design (Phase 11a, D41) ([8a318ea](https://github.com/AbdurahmanAlmehdi/jidr/commit/8a318ea0c609421dcfe495a94e7f3430b4e73e62))
* **site:** use the brand kit for favicon, header symbol and social card (Phase 11a, D39) ([816e53f](https://github.com/AbdurahmanAlmehdi/jidr/commit/816e53f4b0a2a5b7e1fcf10c77f46116f828a804))
* **spike:** display + input spike passes, ADR-0001 frame transport (Phase 1) ([a09c8a2](https://github.com/AbdurahmanAlmehdi/jidr/commit/a09c8a20dd46482c18dc74dfd1a99fe55e939af9))
* **ui,widgets:** player window with direct gRPC frames, input, drag-drop and logcat (Phase 7c) ([463bda3](https://github.com/AbdurahmanAlmehdi/jidr/commit/463bda3d4577703215666bdb441e7687cef71130))
* **ui:** Build identity form for Full-root devices; docs for 9f ([4ca2a9b](https://github.com/AbdurahmanAlmehdi/jidr/commit/4ca2a9b7c0ea01ec1da5029af314f7fe9752f34a))
* **ui:** first-run setup, create/edit sheet, settings and command palette (Phase 7b) ([c877981](https://github.com/AbdurahmanAlmehdi/jidr/commit/c877981cbaa49dc9eaa7b80892bcaca90ff27e18))
* **ui:** root UX for Phase 9e (src/root) ([e1f8597](https://github.com/AbdurahmanAlmehdi/jidr/commit/e1f8597bd9bbd0e40f3c419a75691c3c69503d00))
* **ui:** root UX slots for Phase 9e (gap B8) ([a70ee36](https://github.com/AbdurahmanAlmehdi/jidr/commit/a70ee36a981e0618db7ee3e21f97b21da264a7db))
* **ui:** Workbench shell, generated theme, daemon bridge and snapshot harness (Phase 7a) ([a9de977](https://github.com/AbdurahmanAlmehdi/jidr/commit/a9de9773104292d4182a419e0ff7c78702c5c78f))
* **widgets,ui:** battery, sensors, network, phone, controls, dev tools, clipboard, fingerprint and camera widgets (Phase 8a) ([d9daeb4](https://github.com/AbdurahmanAlmehdi/jidr/commit/d9daeb47ce1f511f79e422af2e9e78ea83abdf32))
* **widgets,ui:** GPS fix and GPX/KML route replay with an OSM map panel (Phase 8b) ([05fabf5](https://github.com/AbdurahmanAlmehdi/jidr/commit/05fabf5d38e46835b24662a275610d3f37d4d917))
* **widgets,ui:** PNG screenshots and MP4/H.264 recording via VideoToolbox (Phase 10a) ([3fd4314](https://github.com/AbdurahmanAlmehdi/jidr/commit/3fd43140ccb7fcc5ef47e37b50c2a72bc80d37ec))
* **widgets,ui:** snapshot manager with root-mode tags and cross-mode refusal (Phase 10b) ([d702a26](https://github.com/AbdurahmanAlmehdi/jidr/commit/d702a26ca9c835a23ccd10f100e386f159d70812))


### Bug Fixes

* **backend:** launch the emulator with -sysdir for the device's image ([2c5e54b](https://github.com/AbdurahmanAlmehdi/jidr/commit/2c5e54b4ed2522d312699effc3df7793e09a5335))
* **backend:** launch the emulator with -sysdir for the device's image ([f70a090](https://github.com/AbdurahmanAlmehdi/jidr/commit/f70a0905fd6e36580a95e4cc280a0cd1af853cb2))
* **client:** never spawn the GUI as the daemon from Jidr.app ([7c5c8d3](https://github.com/AbdurahmanAlmehdi/jidr/commit/7c5c8d3fffbeacc493a84019c86669751c629cbb))
* **client:** never spawn the GUI as the daemon from Jidr.app ([fb93687](https://github.com/AbdurahmanAlmehdi/jidr/commit/fb93687e5f71a1506b77b70945432e25ee362949))
* **design:** Arabic landing type sizes in tokens.json (G13) ([f6d1462](https://github.com/AbdurahmanAlmehdi/jidr/commit/f6d14622c72c3bb1e04151341cd79086dbc2a2f1))
* **root:** back up the stock init so Magisk devices boot ([820df53](https://github.com/AbdurahmanAlmehdi/jidr/commit/820df53cf25692296665115e88e3d92e26f087fe))
* **root:** never leak the e2e emulator on failure, panic or a killed test run ([757f087](https://github.com/AbdurahmanAlmehdi/jidr/commit/757f087933489c9eae1e201744cd9d627bcb6c73))
* **root:** one guarded adbd switch for adb and Magisk modes; kick hung transports ([e610d7b](https://github.com/AbdurahmanAlmehdi/jidr/commit/e610d7b3b100cea1e10c31f36d1dba62ff25a5f3))
* **root:** wait and retry once when adb root hits a closed transport ([4fdea93](https://github.com/AbdurahmanAlmehdi/jidr/commit/4fdea937a4a130e3127de3aeeb04b5e34ed2067b))
* **root:** wait for adb after a cold boot or an adbd restart ([9a25719](https://github.com/AbdurahmanAlmehdi/jidr/commit/9a2571962231b4dc4043b902d44a6fa0baf2bf88))
* **root:** wait for adb after a cold boot or an adbd restart ([143c510](https://github.com/AbdurahmanAlmehdi/jidr/commit/143c5101284f2c5029d97be1be0923af4a4c4798))
* **scripts:** accept an absolute CARGO_TARGET_DIR in the e2e scripts ([8fc9680](https://github.com/AbdurahmanAlmehdi/jidr/commit/8fc96802c8acf1a16a9b58adcab544e51ce24a79))
* **site:** take the Arabic landing type sizes from jidr.css until tokens.json has them (G13) ([4d284af](https://github.com/AbdurahmanAlmehdi/jidr/commit/4d284af36b224f4254c00180257ca9994003873a))
* **ui:** room for the Play Store card's three-line note ([5395e6a](https://github.com/AbdurahmanAlmehdi/jidr/commit/5395e6a6c5b5686f2b7894ce5ee31820582afca2))
* **ui:** show the auto-grant toggle as disabled without root.auto_grant ([7dff8ae](https://github.com/AbdurahmanAlmehdi/jidr/commit/7dff8aec4fd1885fb1041faa6424139f37dd5b2f))
* **widgets,ui:** snapshot path traversal, AppleScript injection, stuck recordings, per-device record state ([64e78ab](https://github.com/AbdurahmanAlmehdi/jidr/commit/64e78abbb985f2c36a9915dcdf9e62399750b4aa))
* **widgets:** app.launch starts the launcher activity with am start -W and confirms it ([3564e70](https://github.com/AbdurahmanAlmehdi/jidr/commit/3564e70481d55ac9d85512c70fb4ba3a425199c7))
* **widgets:** build on Linux and keep snapshot tags when the emulator renames folders ([c0c1832](https://github.com/AbdurahmanAlmehdi/jidr/commit/c0c1832b38824014700a17001aa7159f4db57e71))
* **widgets:** network.set checks the radio took gsm data; retry mobile waits ([b3d14ab](https://github.com/AbdurahmanAlmehdi/jidr/commit/b3d14abd5ff3cb9b784bbb4436dafc3c1b90a66f))
* **widgets:** network.set checks the radio took gsm data; retry mobile waits ([c993da8](https://github.com/AbdurahmanAlmehdi/jidr/commit/c993da871fb767d3f2eeee2408a998c6c713f43c))
* **widgets:** snapshot.load waits until the device answers adb again ([1074699](https://github.com/AbdurahmanAlmehdi/jidr/commit/10746993cba4235867fedbe8aad8b555ace0b1a3))
* **widgets:** switch mobile data at the emulated radio; wait for voice service before calling ([a5c7dc7](https://github.com/AbdurahmanAlmehdi/jidr/commit/a5c7dc702eecef1276738c363bd79e7bb771213b))
