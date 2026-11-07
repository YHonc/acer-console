# DeviceCenter 完整模块接入分析

日期：2026-11-03  
性质：第一阶段只读架构核查；未修改产品代码，未复制 `.tools`、`node_modules`、`.git`、设备凭据或发行物，未运行设备/云账号操作。

## 1. 结论

DeviceCenter 已具备完整的 Tauri 2 + Rust + Svelte 设备后端和界面，但其设备能力目前全部绑定在独立 Tauri 应用内部；Acer Console 现有集成只是“外部 EXE 管理适配”，不能读取鼠标/键盘/挂灯，也不能在 Acer 页面内调用 DeviceCenter 命令。

推荐采用**运行时文件夹模块 + 独立 sidecar 进程**：

1. 模块目录只包含 manifest、已构建的 sidecar 和（可选的）已构建 UI 资源，由 Acer 运行时扫描；删除目录即可不再加载模块。
2. DeviceCenter 的 HID、UDP/miIO、DPAPI、云会话和采样逻辑留在 sidecar；Acer 主进程只经当前用户命名管道/JSON-RPC 调用，不直接链接 GPL Rust crate，也不让主 WebView 直接加载模块脚本。
3. UI 有两种可选实现：
   - Acer 自有 UI：最安全、最易统一视觉，但需要重做大量设备编辑界面。
   - 模块自带 UI：以 Acer 受控的独立 WebView/模块窗口加载，只通过 host bridge 访问 sidecar；能复用现有 DeviceCenter UI，但需要额外做 iframe/窗口隔离和 UI 资源协议。
4. 不建议把 DeviceCenter Rust/Svelte 代码静态并入 Acer 主 crate 后仍宣称“可自由删除模块”。编译期模块只能通过删源码、改 Cargo/前端入口并重新构建来移除，且 GPL 与 Tauri 生命周期耦合更强。

许可证是实施前的前置门槛，不是实现细节：DeviceCenter 为 `GPL-3.0-only`（`D:\ChatGpt\DeviceCenter\src-tauri\Cargo.toml:5`、`D:\ChatGpt\DeviceCenter\src-tauri\tauri.conf.json:20`）。本轮未在 Acer 仓库根目录发现 `LICENSE`/`COPYING`；在确认 Acer 代码的许可策略前，不应直接复制或链接 DeviceCenter 代码。下述 sidecar、模块目录、独立 UI 方案仍需许可证审查。

## 2. 已核对的入口与边界

### 2.1 DeviceCenter 入口

- 应用入口：`D:\ChatGpt\DeviceCenter\src-tauri\src\main.rs:2` 调用 `device_center::run()`。
- Tauri `run()`：`D:\ChatGpt\DeviceCenter\src-tauri\src\lib.rs:1003`。
- 状态与命令注册：`lib.rs:1068` 初始化 `Backend`、`CloudState`、`SnapshotStore`，启动 `monitor::start` 和 tray；命令注册在 `lib.rs:1011`。
- 前端入口：`src/main.ts` 根据查询参数挂载 `App`、`Compact`、`MappingPreview` 或 `LampPresetPreview`。
- UI 主结构：`src/App.svelte`、`src/DeviceSettings.svelte`、`src/Compact.svelte`、`src/TrayPreferences.svelte`。

### 2.2 设备与协议模块

| 领域 | 主要文件 | 关键符号/边界 |
|---|---|---|
| HID 枚举、电量、设备队列 | `src-tauri/src/lib.rs` | `list_hid_devices`、`read_battery`、`battery_match`、`device_queue`、`query_battery_sync` |
| ATK/KX 基础设置 | `src-tauri/src/settings.rs` | `SettingsSnapshot`、`SettingsChange`、`read_sync`、`apply_sync` |
| ATK 性能/DPI/宏 | `src-tauri/src/settings/atk_extensions.rs` | `read_performance_sync`、`apply_performance_sync`、`read_dpi_sync`、`read_macro_sync`、`bind_macro_sync` |
| ATK 按键映射 | `src-tauri/src/settings/mappings.rs`、`settings/mappings/shortcuts.rs` | `MappingSnapshot`、`MappingChange`、24/144 字节块与快捷键区 |
| KX 按键映射 | `src-tauri/src/settings/kx_mapping.rs` | 三层 96 槽、512 字节块、37 包 ACK/读回 |
| KX 灯效 | `src-tauri/src/settings/kx_extensions.rs` | `Snapshot`、`Change`、128 字节性能块与 490 字节颜色块 |
| 挂灯 UDP/miIO | `src-tauri/src/protocol.rs`、`src-tauri/src/crypto.rs` | `HELLO`、`probe`、`Session`、AES/MD5、UDP 54321 |
| 挂灯凭据 | `src-tauri/src/config.rs`、`src-tauri/src/crypto.rs` | `Device`、`normalize_ip`、`token_bytes`、DPAPI |
| 米家扫码/云列表 | `src-tauri/src/cloud.rs` | `begin_mi_login`、`poll_mi_login`、`list_mi_devices`、`import_mi_lamp`、`resume/forget/cancel` |
| 后台监控/通知 | `src-tauri/src/monitor.rs` | HID 电量轮询、挂灯轮询、低电量通知、退避 |
| 托盘显示/偏好 | `src-tauri/src/tray_display.rs` | `get_tray_preferences`、`set_tray_preferences`、托盘电池图标 |

## 3. 现有功能与命令依赖

### 3.1 鼠标（ATK F1）

| 功能 | 前端入口 | 命令 | 主要依赖/语义 |
|---|---|---|---|
| 设备枚举与电量 | `App.svelte` 的设备卡、`read_battery` | `list_hid_devices`、`read_battery` | `hidapi`；ATK 373B:101A、FF02、usage2、interface1；电量事件 `battery-update` |
| 基础性能 | `DeviceSettings.svelte` 性能区块 | `get_device_settings`、`apply_device_settings` | 10 字节块；`reportRateHz`、`dpiLevel`；预览值/`expectedState`/完整读回 |
| 扩展参数 | `AtkParameters.svelte` | `read_atk_performance`、`apply_atk_performance` | 去抖、Motion Sync、直线/波纹修正、休眠、抬起高度；完整 12 字节 CAS |
| DPI 档位 | `AtkDpi.svelte` | `read_atk_dpi`、`apply_atk_dpi` | 已有档位 X/Y 值、范围/步长、完整原块 CAS |
| 按键功能 | `MappingPanel.svelte` | `read_button_mappings`、`apply_button_mapping` | 6 个物理键、19 项已核实动作、未知映射只读、24/144 字节保留 |
| 组合快捷键 | `MappingPanel.svelte` | 同上，`change.action='shortcut'` | 最多两个不同左侧修饰键、有限键码；未知原序列保留 |
| 宏 | `AtkMacro.svelte` | `read_atk_macro`、`apply_atk_macro`、`bind_atk_macro` | 12 槽、最多 70 动作、384 字节/槽、切片写后读回、绑定 4 字节映射 |
| 官方入口 | `DeviceSettings.svelte` | `open_official({brand:'atk'})` | 打开 ATK 官网/官方驱动页；不执行固件升级 |

缺口：配置管理、恢复默认、任意组合键/其他传感器参数、固件升级仍没有完整契约；当前 UI 对未实现项显示“待补/只读”，不能伪装为已支持。

### 3.2 键盘（KX75）

| 功能 | 前端入口 | 命令 | 主要依赖/语义 |
|---|---|---|---|
| 电量 | `App.svelte` 设备卡 | `read_battery` | KX 3554:FA09、FF02、usage2、interface1；每次先 `verify_kx75` 校验型号签名；充电语义未知 |
| 基础性能 | `DeviceSettings.svelte` | `get_device_settings`、`apply_device_settings` | 128 字节性能块；250/500/1000Hz、TopSpeed、Mac 模式、Win 锁、休眠；完整块 CAS |
| 分层映射 | `MappingPanel.svelte` | `read_keyboard_mappings`、`apply_keyboard_mapping` | Default/Fn/Fn2、96 槽、512 字节、37 包 ACK；基础/鼠标/媒体/扩展动作 |
| 内置灯效与单色 RGB | `KeyboardLighting.svelte`、`KeyboardLayout.svelte` | `read_keyboard_lighting`、`apply_keyboard_lighting` | 15 个已核实模式、亮度/速度/多色、490 字节颜色区；逐键 DIY、音乐/GIF 未开放 |
| 官方入口 | `DeviceSettings.svelte` | `open_official({brand:'mchose'})` | 打开迈从官方入口 |

缺口：KX 任意组合键/设备宏编码、逐键 DIY、音乐/GIF、固件升级仍未闭环。

### 3.3 挂灯（小米 1S）

| 功能 | 前端入口 | 命令/存储 | 主要依赖/语义 |
|---|---|---|---|
| 本地连接 | `App.svelte` 手动连接、`MiCloudConnect.svelte` | `save_lamp_config({ip,token})` | 私有 IPv4、32 位十六进制 Token；Token 仅在后端接收并写入 DPAPI |
| 读取/开关/亮度/色温 | `LampPalette.svelte`、`Compact.svelte` | `read_lamp`、`set_lamp({change})` | UDP 54321；亮度 1–100、色温 2700–6500K；写后读回；事件 `lamp-update` |
| 本地预设 | `LampPresets.svelte` | 前端 localStorage 的 `lamp-preset-v1:` 前缀 | 只保存名称、亮度、色温；内置阅读/专注/柔和；跨窗通过事件刷新 |
| 米家扫码/导入 | `MiCloudConnect.svelte` | `begin_mi_login`、`poll_mi_login`、`resume_mi_login`、`list_mi_devices`、`import_mi_lamp`、`forget_mi_login`、`cancel_mi_login` | 小米 HTTPS 云、二维码、Cookie/RC4/签名；设备摘要不含 Token；导入内部直传后端凭据 |
| 连接状态 | `App.svelte` | `get_lamp_config`、`read_lamp` | `get_lamp_config` 只返回 `configured/ip`，不返回 Token |

缺口：当前只支持准确型号 `yeelink.light.lamp22`；云端风控、共享家庭分页和实际写入仍按 DeviceCenter 的验收矩阵保留待测。

### 3.4 桌面、托盘和生命周期

| 功能 | 命令/文件 | 边界 |
|---|---|---|
| 快照 | `get_device_snapshots` | `SnapshotStore` 缓存；超过 5 分钟标记 stale；事件不替代当前读取 |
| 主窗/快捷窗 | `show_main_window`、`close_device_window`、`exit_device_center` | DeviceCenter 自己的窗口生命周期；关闭主窗取消米家活动会话 |
| 登录自启 | `get_autostart`、`set_autostart` | Tauri autostart 插件；模块嵌入式集成时应由 Acer host 统一拥有，避免双重自启 |
| 托盘 | `tray_display.rs`、`get_tray_preferences`、`set_tray_preferences` | DeviceCenter 自己的托盘电池图标、快捷窗入口和退出 |
| 后台电量/挂灯 | `monitor.rs` | 30 秒前台、120 秒后台、指数退避至 900 秒；低电量 20% 通知；关闭主窗不停止 |

当前注册的 37 个 Tauri 命令集中在 `D:\ChatGpt\DeviceCenter\src-tauri\src\lib.rs:1011`（含 `get_autostart`、`get_device_settings` 等条件 attribute 命令）；sidecar 不应原样暴露所有命令，尤其不应把窗口/托盘/自启命令暴露给 Acer 页面。

### 3.5 生产命令总表（37）

| 组 | 命令 | 主要依赖 |
|---|---|---|
| 挂灯配置/控制 | `get_lamp_config`、`save_lamp_config`、`read_lamp`、`set_lamp` | `config.rs`、`protocol.rs`、`crypto.rs`、`lamp_queue` |
| HID/快照 | `list_hid_devices`、`read_battery`、`get_device_snapshots` | `hidapi`、`battery_match`、`device_queue`、`SnapshotStore` |
| 基础设置 | `get_device_settings`、`apply_device_settings` | `settings.rs` |
| ATK 扩展 | `read_atk_performance`、`apply_atk_performance`、`read_atk_dpi`、`apply_atk_dpi`、`read_atk_macro`、`apply_atk_macro`、`bind_atk_macro` | `settings/atk_extensions.rs` |
| ATK 映射 | `read_button_mappings`、`apply_button_mapping` | `settings/mappings.rs`、`mappings/shortcuts.rs` |
| KX 映射/灯效 | `read_keyboard_mappings`、`apply_keyboard_mapping`、`read_keyboard_lighting`、`apply_keyboard_lighting` | `settings/kx_mapping.rs`、`settings/kx_extensions.rs` |
| 官方/系统 | `open_official`、`get_autostart`、`set_autostart` | 外部 URL、Tauri autostart 插件 |
| 窗口/托盘 | `show_main_window`、`close_device_window`、`exit_device_center`、`get_tray_preferences`、`set_tray_preferences` | Tauri 窗口、`tray_display.rs` |
| 米家云 | `begin_mi_login`、`poll_mi_login`、`list_mi_devices`、`import_mi_lamp`、`resume_mi_login`、`forget_mi_login`、`cancel_mi_login` | `cloud.rs`、`reqwest`、DPAPI、米家 HTTPS |

## 4. 数据存储与凭据边界

### 4.1 DeviceCenter 现有数据

| 数据 | 位置/格式 | 是否含秘密 | 说明 |
|---|---|---|---|
| 挂灯配置 | `app_config_dir()/lamp.dpapi`（`lib.rs:1077`） | 是 | JSON `ip/token/did`，Windows DPAPI 加密；当前用户范围 |
| 米家登录态 | `app_config_dir()/mi-cloud-session.dat`（`cloud.rs:137`） | 是 | 地区、用户ID、security、serviceToken；DPAPI 加密；不返回 WebView |
| 托盘偏好 | `app_config_dir()/tray-preferences.json`（`tray_display.rs:269`） | 否 | 托盘显示模式等 |
| 挂灯预设 | WebView `localStorage`，键前缀 `lamp-preset-v1:`（`lampPresets.js:2`） | 否 | 名称、亮度、色温；不含 Token |
| 设备设置/宏/DPI/灯效 | 设备本身；后端只保留读回快照/状态 token | 否 | 不写隐藏的应用数据库；未知字节保留 |

### 4.2 必须保持的安全边界

1. `get_lamp_config` 只返回 `configured/ip`；`list_mi_devices` 只返回设备摘要；`import_mi_lamp` 的 Token 仅在 Rust 内部转交 `save_lamp_config`。模块 RPC 不得新增返回 Token 的字段。
2. 手动 Token 输入是用户显式输入，经过 Tauri/IPC 到后端后立即加密；不得进入 localStorage、日志、模块 manifest、命令行或异常文本。
3. DPAPI 绑定当前 Windows 用户。sidecar 若改由提权服务或其他账户运行，将无法解密现有数据；建议 sidecar 与 Acer UI 同用户运行，硬件权限问题不要通过更换服务账户解决。
4. HID `deviceId` 是本次枚举路径的十六进制编码；不能跨重启持久化或接受调用方任意路径，必须由 sidecar 重新枚举匹配。
5. 云模块的 `begin/poll/list/import/cancel/resume/forget` 保留当前会话、超时和取消语义；不得把 Cookie、serviceToken、用户ID 或原始响应暴露给 Acer 主 WebView。
6. 模块删除时不应自动删除 DPAPI/云凭据；应由用户明确选择“移除模块并保留数据/清除数据”。删除模块前必须停止 sidecar、托盘/自启/通知/窗口等残留。

## 5. 许可证边界

- DeviceCenter：`GPL-3.0-only`（`src-tauri/Cargo.toml:5`、`src-tauri/tauri.conf.json:20`），根目录 `LICENSE` 和 `src-tauri/LICENSE` 为 GPLv3；`THIRD_PARTY_NOTICES.md` 还记录 QiLight GPL 与 ATK Tool Plus MIT。
- Acer Console：本轮在 `E:\逆向宏碁控制台` 根目录未发现 `LICENSE`/`COPYING`，README 也未给出项目自身许可证。不能假定 Acer 代码可吸收 GPL 代码。
- 直接复制 DeviceCenter 的 Rust/Svelte 代码、把 GPL crate 静态链接进 Acer、或把两者作为同一衍生作品分发，许可证风险最高；独立进程/独立模块目录仍需保留 GPL 源码、许可证和第三方通知，是否构成组合作品应由许可证审查确认。
- 推荐把 DeviceCenter sidecar 作为单独的 GPL 组件分发，模块目录内带 `LICENSE`、`THIRD_PARTY_NOTICES.md`、对应源码/源码获取方式；Acer 本体是否可以在非 GPL 条款下分发需先做法律/许可决策。本文件不是法律意见。

## 6. Acer 当前集成与完整接入差距

### 6.1 当前实现

`E:\逆向宏碁控制台\crates\acer-system\src\device_center_plugin.rs`：

- `Binding { exe, sha256 }`，保存到 `app_config_dir()/device-center-plugin.json`（`:14`）。
- `query` 只查询外部 exe/进程/启动项（`:50`）。
- `bind` 保存路径和 SHA256（`:66`）；`action` 仅支持 `Launch/EnableStartup/DisableStartup`（`:13`、`:78`）。
- `log` 读取本控制台管理事件 JSONL（`:92`）。

`E:\逆向宏碁控制台\apps\desktop\src-tauri\src\plugin_commands.rs:7` 只暴露管理命令；`E:\逆向宏碁控制台\apps\desktop\src\lib\DeviceCenterPlugin.svelte` 只显示绑定、启动、自启动和进程统计。文档 `docs/DeviceCenter插件集成.md:3` 明确写明设备设置仍在原应用。

### 6.2 缺口清单

| 能力 | 当前 Acer | 完整模块需要 |
|---|---|---|
| 设备枚举/电量 | 无 | sidecar RPC + snapshot/event 合同 |
| ATK 鼠标设置、DPI、宏、映射 | 无 | 复用 `settings/*` 或 sidecar 命令映射 |
| KX75 性能、分层映射、灯效 | 无 | 同上；大块 CAS/读回语义不得改 |
| 挂灯读取/控制/预设 | 无 | UDP/miIO sidecar + 独立 UI/事件 |
| 米家扫码/导入 | 无 | 云会话 RPC；凭据留在 sidecar |
| 设备 UI | 仅外部 exe 管理页 | 模块自有 UI 或 Acer 自有 UI |
| 采样/托盘/通知 | 仅管理事件日志 | 明确 owner；避免 DeviceCenter 与 Acer 双重轮询/托盘/自启 |
| 模块发现/删除 | 硬编码页面 | 运行时 manifest 扫描、缺失即隐藏、停止 sidecar、保留/清理数据策略 |
| 许可证 | 未接 GPL 代码 | sidecar 分发、源码提供、通知与许可审查 |

## 7. 可选模块方案

### 7.1 方案 A：运行时文件夹 + sidecar + 模块自有 UI（推荐用于完整功能复用）

模块目录：

```text
modules\device-center\
  module.json
  bin\device-center-service.exe
  ui\index.html              # 可选：复用已构建的 DeviceCenter 前端
  LICENSE
  THIRD_PARTY_NOTICES.md
  source\                    # 或源码获取说明
```

运行方式：

- Acer 启动时扫描 `%APPDATA%\AcerConsole\modules\*\module.json` 和/或安装包内置 `resources/modules`；只接受已登记 API 版本的 manifest。
- 打开模块时 Acer 创建当前用户命名管道，启动 sidecar，传入管道名/一次性 nonce；sidecar 只提供 manifest 允许的 RPC。
- UI 由 Acer 在受控 `WebviewWindow`/隔离 iframe 中加载 `ui/index.html`，只允许通过 host bridge 调用 sidecar；不把模块目录当作可访问 Tauri IPC 的主页面。
- 删除模块目录后 Acer 停止 sidecar、隐藏入口；DPAPI/预设数据按 manifest 的保留策略处理。

优点：可复用现有 DeviceCenter 完整 UI 和设备协议；删除模块不要求重编译 Acer；GPL 代码不进入 Acer crate。  
缺点：需要新增模块 host、RPC、WebView 隔离和 UI 资源协议；UI 与 Acer 主页面不是同一 DOM，视觉整合需额外工作。

### 7.2 方案 B：运行时文件夹 + sidecar + Acer 自有 UI

sidecar 相同；Acer 自己用 Svelte 重建设备页，调用同一模块 RPC。

优点：单窗口、统一视觉、权限最小；模块删除只影响入口；最容易控制错误/未知/禁用状态。  
缺点：鼠标 DPI/宏/映射、KX 分层灯效、挂灯二维面板/预设、米家扫码等 UI 需要重做；工作量大，且不能用“复用 UI”作为快速交付。

这是长期最安全的宿主边界，但不是最小实现。

### 7.3 方案 C：编译期模块

把 DeviceCenter 的 Rust 后端作为 workspace crate、前端组件作为 Acer `src/modules/device-center/` 引入，通过 feature flag 构建。

优点：类型共享、单进程、最容易做到同一窗口；设备队列和快照可直接复用。  
缺点：不是运行时可删除模块；与 Tauri 窗口/托盘/自启生命周期冲突；依赖集合明显扩大；GPL 与 Acer 代码组合/分发风险最高；升级必须重编译整个 Acer。  
结论：仅当用户接受“源码模块、重新构建才能删除”并完成许可证决策时选择，不建议作为本轮目标。

### 7.4 方案 D：保持独立 EXE 管理（现状/回退）

保留 `device-center-plugin.json`、外部 exe 路径/SHA256、启动和管理日志，不接入设备。

优点：现有已交付、无 GPL 代码进入、可以作为过渡和回退。  
缺点：不满足完整接入；设备操作仍要打开原应用；两套托盘/自启/进程并存。

## 8. Manifest 模板

以下为建议的 v1 模板；`ui.kind` 可为 `module-window`、`sandboxed-webview` 或 `host-rendered`。字段名和值应集中定义，不要让模块自行解释任意脚本。

```json
{
  "schemaVersion": 1,
  "id": "device-center",
  "displayName": "Device Center 外设中心",
  "version": "0.1.1",
  "apiVersion": 1,
  "entry": {
    "kind": "sidecar",
    "executable": "bin/device-center-service.exe",
    "sha256": "SHA256_HEX",
    "args": ["--module-mode", "--pipe", "{pipeName}"],
    "singleton": true,
    "healthTimeoutMs": 3000
  },
  "ui": {
    "kind": "module-window",
    "entry": "ui/index.html",
    "origin": "module://device-center",
    "bridge": "postmessage",
    "csp": "default-src 'self'; connect-src 'none'; img-src 'self' data:; style-src 'self' 'unsafe-inline'"
  },
  "permissions": [
    "hid.read",
    "hid.write",
    "udp.54321",
    "cloud.xiaomi",
    "dpapi.current_user",
    "open.external_url"
  ],
  "capabilities": ["mouse", "keyboard", "lamp", "cloud"],
  "lifecycle": {
    "start": "on-demand",
    "stopOnHostExit": true,
    "ownsTray": false,
    "ownsAutostart": false,
    "ownsNotifications": false
  },
  "data": {
    "namespace": "device-center",
    "legacyProfile": "local.device-center.app",
    "deletePolicy": "retain-until-user-confirms"
  },
  "license": {
    "spdx": "GPL-3.0-only",
    "file": "LICENSE",
    "source": "source/"
  }
}
```

若采用 Acer 自有 UI，`ui.kind` 改为 `host-rendered`，`ui.entry` 可省略，`capabilities` 驱动页面显隐；若采用 `sandboxed-webview`，UI 不得直接获得 Tauri invoke 权限，只能使用受限 bridge。

### 8.1 必须校验的 manifest 规则

- `id` 只允许 `[a-z0-9-]`；`entry.executable` 必须是模块根目录内相对路径，规范化后不得逃逸。
- `apiVersion` 不匹配、缺少许可证、哈希不符或 UI 入口不存在时不加载；未知字段拒绝而不是忽略。
- manifest 不存放 Token、Cookie、绝对路径、用户名或数据文件内容。
- `permissions` 只允许 Acer 已实现的白名单；`ownsTray/ownsAutostart/ownsNotifications` 默认 false，避免与 Acer 生命周期重复。
- 删除/缺失模块时不得阻塞 Acer 启动；停止 sidecar，隐藏入口，保留数据并提示用户是否清理。

## 9. sidecar 与 host bridge 建议

### 9.1 进程和传输

- 每个模块一个 sidecar 实例；Acer 创建当前用户命名管道，sidecar 只接受该管道名/一次性 nonce，不监听公网端口。
- JSON-RPC/行分隔请求；请求包含 `id`、`method`、`params`，响应包含 `result` 或 `error`。事件用独立 `event` 帧。
- host 只允许 manifest 声明的方法；方法名命名空间为 `deviceCenter.*`，拒绝任意 Tauri command 名称。
- 不把原始 Token、Cookie、设备原始响应、绝对文件路径写入日志；错误固定分类，不拼接敏感响应。

### 9.2 建议方法面

| 命名空间 | 方法 | 等价现有命令 |
|---|---|---|
| deviceCenter.device | `list`、`battery`、`snapshots` | `list_hid_devices`、`read_battery`、`get_device_snapshots` |
| deviceCenter.settings | `get`、`apply` | `get_device_settings`、`apply_device_settings` |
| deviceCenter.atk | `performance.get/apply`、`dpi.get/apply`、`mapping.get/apply`、`macro.get/apply/bind` | 同名 ATK 命令 |
| deviceCenter.kx | `mapping.get/apply`、`lighting.get/apply` | 同名 KX 命令 |
| deviceCenter.lamp | `config.get/save`、`read`、`set` | 挂灯命令 |
| deviceCenter.cloud | `begin/poll/resume/list/import/forget/cancel` | 米家命令 |
| deviceCenter.host | `health`、`foreground`、`shutdown` | 新增模块生命周期，不暴露窗口/托盘命令 |

窗口、托盘、系统自启和通知由 Acer host 拥有；DeviceCenter 的 `show_main_window`、`close_device_window`、`exit_device_center`、`get_tray_preferences`、`set_tray_preferences`、`get_autostart`、`set_autostart` 不应作为模块 RPC。

### 9.3 生命周期与采样

- sidecar 复用 `monitor.rs` 的 HID/挂灯轮询和退避，但把 Tauri `AppHandle` 可见性改为 host 通过 `foreground` 事件提供的状态；不得让 sidecar 在没有窗口时依赖 `get_webview_window('main')`。
- host 负责系统托盘、自启、通知；sidecar 通过 `battery.updated`/`lamp.updated`/`battery.low` 事件发送摘要，host 决定是否通知。
- host 关闭/删除模块时先 `shutdown`，等待有界时间；强制结束只针对已确认的 sidecar PID，并清理管道；不把关闭 Acer 窗口误当模块删除。
- 首次打开模块才启动 sidecar；模块缺失、manifest 失效或权限不足时显示可删除/重装状态，不进行设备 I/O。

## 10. 数据迁移与删除策略

1. **挂灯/云凭据**：优先让 sidecar 继续使用 DeviceCenter 现有 app config 目录（当前 identifier `local.device-center.app`），避免重新解密/迁移 DPAPI；Acer 不读取这些文件。若必须改名，使用同用户 DPAPI 重新加密并保留回滚备份。
2. **挂灯预设**：`lamp-preset-v1:` 位于 DeviceCenter 原 WebView origin 的 localStorage。如果模块 UI 使用不同 origin，预设不会自动出现；第一阶段应导入/导出或提供一次性迁移，不能宣称自动兼容。
3. **现有外部 EXE 绑定**：保留 `device-center-plugin.json` 作为回退；完整模块验收通过前不覆盖、不删除。迁移时先停外部应用/自启，再由用户确认移除旧绑定。
4. **删除模块**：停止 sidecar、隐藏入口，不删除 DPAPI/云缓存/预设；提供“仅移除模块”和“移除并清除模块数据”两个明确选择。Acer 自身数据库不应有无法清理的模块外键。

## 11. 实施阶段与文件落点（建议，不在本阶段修改）

### 阶段 0：许可证和隔离决策

- 完成 Acer 仓库许可/分发方式决策；保留 DeviceCenter GPL、QiLight、ATK 通知和源码提供方案。
- 明确 UI 方案：模块窗口/隔离 WebView 复用现有 UI，还是 Acer 自有 UI。

### 阶段 1：从 DeviceCenter 提取 headless sidecar

- 在 DeviceCenter 内新增 `src-tauri/src/bin/device-center-service.rs`（或同 crate 的 sidecar module），复用 `Backend`、`SnapshotStore`、`device_queue`、`protocol.rs`、`settings/*`、`cloud.rs`。
- 将 `lib.rs:1003 run()` 拆成：
  - `run_desktop()`：保留现有窗口、托盘、autostart；
  - `run_module_service()`：无窗口、无 tray/autostart，只管理 RPC 和采样。
- `monitor.rs` 的 `get_webview_window` 可见性改为 host foreground 注入；`tray_display.rs` 不启动。
- 保留 `open_official` 和现有 37 个命令的 DTO 语义；不要改协议 CAS、读回和错误分类。

### 阶段 2：Acer module host

- 新增 Acer 运行时模块注册表（建议 `apps/desktop/src-tauri/src/module_host.rs`）：
  - 扫描用户模块目录；
  - 校验 manifest、路径、哈希、API 版本；
  - 管理命名管道、sidecar 生命周期和事件。
- 新增前端模块入口（建议 `apps/desktop/src/lib/modules/DeviceCenterModule.svelte` 或 `modules/DeviceCenterPage.svelte`）；删除目录时入口消失，不影响 Acer 其它页面。
- 保留现有 `DeviceCenterPlugin.svelte` 作为外部 EXE 回退，直到模块验收完成。

### 阶段 3：UI 与功能验收

- 按设备分组接入鼠标、键盘、挂灯页面；未知/过期/断线/禁用状态与当前 DeviceCenter 语义一致。
- 米家扫码 UI 必须保证 Token 不进入 Acer 主 WebView、localStorage 或日志。
- 设置关闭、窗口销毁、托盘/自启/通知 owner 只保留一个。

## 12. 验收清单（供主代理决策后实施）

- [ ] 删除模块文件夹后 Acer 可启动，模块入口消失，无残留 sidecar/管道/自启/托盘。
- [ ] manifest 校验拒绝路径逃逸、哈希不符、API 版本不符、缺失许可证和未知权限。
- [ ] 鼠标电量、ATK 性能/DPI/映射/宏，KX 性能/映射/灯效，挂灯读取/控制/预设/米家导入均有独立成功、失败、断线和读回状态。
- [ ] 37 个现有设备命令的 DTO/错误/读回语义没有改变；sidecar 不暴露窗口/托盘/自启命令。
- [ ] HID `deviceId` 每次重新枚举；未知设备/错误型号不发送写入。
- [ ] Token、Cookie、用户ID 不进入 manifest、Acer 日志、localStorage 或模块 UI 持久化；DPAPI 与同用户 sidecar 兼容。
- [ ] DeviceCenter 自己的托盘/自启/通知在模块模式下关闭，Acer host 只保留一套生命周期。
- [ ] 模块删除/停用时不删除凭据，除非用户明确选择清除；删除前可停止正在运行的设备操作。
- [ ] 许可证、完整源码/源码获取方式、GPL/QiLight/ATK 通知随模块分发。

## 13. 待主代理决定

1. 选择方案 A（sidecar + 模块 UI 窗口）、方案 B（sidecar + Acer 自有 UI）还是先保持方案 D（外部 EXE）。
2. Acer 仓库的许可证/分发方式；是否接受 GPL sidecar 与源码提供义务。
3. 单窗口嵌入 UI 是否必需；若必需，是否接受新增受控 WebView/iframe bridge。
4. 现有 DeviceCenter 数据是否原地保留，还是做一次性 DPAPI/预设迁移。
5. 托盘、通知、自启最终 owner；本分析建议全部归 Acer host，模块只负责设备 I/O。
