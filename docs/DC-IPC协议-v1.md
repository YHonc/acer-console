# Device Center IPC 协议 v1（契约）

日期：2026-11-07。状态：**冻结**，B1（DC 服务端）、B2（宿主客户端）、U3（前端）都以本文为准。改动须升 minor（向后兼容）或 major（不兼容），并同步本文。
来源：`work/dc-inventory.md` 第 3–5 节；用户确认的方案见 `work/plan-20261106.md`（独立进程 + 命名管道，无窗口无托盘，默认随控制台退出）。

## 1. 进程与启动

- 宿主（Acer Console）发现 `<控制台 exe 目录>/modules/device-center/` 并通过 manifest 与哈希校验后，以子进程启动：
  `DeviceCenter.exe --headless --pipe <管道名> --parent-pid <宿主 PID> --session <nonce>`
- 四个参数缺一不可；出现未知参数时退出码 2。`--headless` 模式下：
  - 不创建任何窗口或 WebView（不能用 `visible:false` 代替），不建托盘，不注册菜单，不改自启动。
  - 只保留 Backend、CloudState、SnapshotStore、monitor（HID、电量、挂灯轮询）。
  - 轮询频率：有 IPC 客户端连接时按前台频率 30s，没有连接时按后台 120s，退避上限 900s。
- 单实例：按当前用户 SID 建命名互斥体 `Local\AcerConsole.DeviceCenter.<sidhash>`。互斥体已被占用时（独立 GUI 或另一个 headless 正在运行），退出码 3，宿主提示“Device Center 已在其他地方运行”。
- 父进程监视：`--parent-pid` 退出时，服务端自行 shutdown。“控制台退出时保持后台运行”（`keep`）为真时，宿主发 `detach`，服务端停止监视父进程，并在没有连接 30 分钟后自行退出。
- 退出码：0 正常，2 参数错误，3 已有实例，4 管道创建失败，10 内部错误。

## 2. 管道

- 名称：`\\.\pipe\AcerConsole.DeviceCenter.<sidhash>.<nonce>`。`sidhash` 是当前用户 SID 的 SHA-256 前 16 个十六进制字符；`nonce` 由宿主生成，32 个十六进制字符。
- 服务端创建：`PIPE_REJECT_REMOTE_CLIENTS`，DACL 只允许当前用户 SID，最多 1 个实例，第一个客户端连接后不再接受新客户端。
- 帧：4 字节小端长度，后跟 UTF-8 JSON，单帧不超过 1 MiB，超长即断开。字段一律 camelCase。
- 连接后客户端必须在 5 s 内发 `hello`，否则断开。服务端校验客户端 PID（`GetNamedPipeClientProcessId`）等于 `--parent-pid`、`nonce` 等于 `--session`，不符返回 `UNAUTHORIZED` 并断开。

### 2.1 v1.0.1 补充（2026-11-07，按 B1/B2 实现反馈）

- hello 被拒（`PROTOCOL_MISMATCH` / `UNAUTHORIZED`）：服务端发 `{"type":"response","id":0,"ok":false,"error":{…}}` 后断开。`id=0` 保留给握手。
- 重连：前一个客户端断开后，服务端可再接受一个新连接，仍须通过 PID 与 nonce 校验。所以同一宿主进程可以重连；新的宿主进程不能接管（一期限制）。
- `keep=true` 的一期行为：新启动的控制台遇到退出码 3 时，提示“Device Center 后台仍在运行，请先停止”，不尝试接管。接管协议留到 v1.1。
- 握手完成前，宿主取消启动时不强杀子进程，依赖服务端的父进程监视退出。

### 2.2 v1.0.2 修订（2026-11-07，按 A2 审查）

- **同一设备的 HID 事务一律串行**，读取和写入都排进同一个设备队列，取代第 4 节原来的“读取不排队，也不被写入阻塞”。原因：底层 HID 按“发请求→顺序读应答”工作，并发读写会让应答错配（A2 阻断 1）。不同设备之间仍可并行。UI 的“读取不锁操作”由宿主和前端实现，与设备侧排队无关。
- 写入完成前，排队中的读取等写入结束后再执行，各自的 `deadlineMs` 照常计时；超时返回 `TIMEOUT`。
- 超时或部分写入后的补读，每台设备合并为一次：只在该设备所有在途操作结束后执行一次，服务端和宿主都不重复触发。
- 重连（2.1）为必选实现：服务端在客户端断开后重置认证状态、请求 ID 记录、缓冲区和事件订阅，再重新监听；宿主保留子进程、管道名和 nonce，断线后在 5 s 内退避重连（最多 3 次），仍失败或子进程已退出才转为 `error`。
- 宿主退出预算：从调用开始共 5 s。`detach` 帧必须确认写出，否则返回错误，让 ExitCoordinator 保留控制台并提示用户。`shutdown` 超时后，只在 PID 与 helloAck 一致时终止子进程；终止失败也返回错误。
- **v1.0.3 补充（A3 复审）**：
  - **恢复读取只由宿主执行**。服务端不主动补读，只报告 `TIMEOUT` / `PARTIAL_WRITE`。宿主按设备合并，只执行一次读取；服务端的设备队列保证这次读取排在所有已接收的写入之后。
  - **断线期间在途的写请求，结果视为未知**。宿主不能把它报成普通 `INTERNAL`，要报 `TIMEOUT`（`details.reason = "disconnected"`），并在重连成功后加入该设备的合并补读。重连失败时，模块转为 `error`，该设备的读数清空，显示“—”。
  - **退出预算覆盖全路径**。5 s 从 ExitCoordinator 调用开始计，获取 Host 锁也算在内，应使用带超时的 `try_lock`。watcher 的重连不能一直持有 Host 锁，等待和退避期间要释放锁。超时后的取消要确保 detach 帧不会再迟到写出：取消时从发送队列中撤回，或者在写线程上检查截止时间。
- **v1.0.5 更正（A4 复审）**：
  - **退出预算的范围**：5 s 只约束 Device Center 这一步，从 ExitCoordinator 开始执行 DC 退出时计时，获取 Host 锁也算在内。风扇和温控的退出恢复（`stop_thermal`、退出恢复 `apply`）保持 B2 之前的原有行为和超时，不受这 5 s 截断。v1.0.3 的“从 ExitCoordinator 调用开始”说得过宽，以本条为准。之所以这样规定，是为了避免界面已经报超时，后台却仍在写风扇。
  - **补读必须闭环**：
    - 补读结果由宿主通过 Tauri 事件 `dc://readback` 推给前端，内容为 `{deviceId, method, ok, result | error}`，前端据此更新读回。
    - 补读失败时，该设备对应项变为未知。
    - 补读过程中再次断线时，这项补读要重新排队，重连后再执行。
  - **终态清理**：进入 `error` 或 `elsewhere` 后，宿主要关闭旧连接、回收子进程句柄并清空会话。重新检测或重新启动时再建立新会话。
- 宿主把错误码反序列化为枚举，未知错误码按 `INTERNAL` 处理；`retryable` 由宿主按第 6 节规则重新计算，不信任服务端给的值。
- token：`lamp.config.save` 的请求可以带明文 token，因为管道是本机、仅限当前用户 ACL 且已认证的连接。token 不得出现在响应、事件、日志和错误详情中；内存中的副本用 `Zeroizing` 包装。

## 3. 消息

```jsonc
// 客户端 → 服务端
{"type":"hello","protocol":{"major":1,"minor":0},"nonce":"…","clientPid":1234}
{"type":"request","id":7,"method":"settings.read","params":{"deviceId":"…"},"deadlineMs":5000}
{"type":"ping","id":8}
{"type":"detach"}                         // 宿主退出且 keep=true
{"type":"shutdown","reason":"hostExit"}   // 宿主退出且 keep=false，或用户停止

// 服务端 → 客户端
{"type":"helloAck","protocol":{"major":1,"minor":0},"moduleId":"device-center","moduleVersion":"0.1.1","pid":8840,"capabilities":["device.list", "…"]}
{"type":"response","id":7,"ok":true,"result":{…}}
{"type":"response","id":7,"ok":false,"error":{"code":"DEVICE_DISCONNECTED","message":"…","retryable":true,"details":null}}
{"type":"pong","id":8}
{"type":"event","seq":42,"name":"battery.updated","deviceId":"…","data":{…}}
```

- `major` 不同：服务端回 `PROTOCOL_MISMATCH` 并断开；宿主显示“协议版本不兼容”。`minor` 不同时双方按能力交集工作。
- `id` 由客户端单调递增；响应可以乱序。`deadlineMs` 默认 5000、最大 30000，超时返回 `TIMEOUT`（写入超时另见第 6 节）。
- `capabilities` 只列本次实际可用的方法名。客户端调用不在列表里的方法，服务端返回 `UNKNOWN_METHOD`。UI 对不可用的能力不显示控件。
- `shutdown`：服务端停止接受新请求，最多等在途请求 3 s，取消云会话、停止线程后退出（退出码 0）。宿主发出后最多等 5 s，超时只终止**自己启动且 PID 与 helloAck 一致**的进程。

## 4. 方法（一期，白名单）

参数和结果**直接复用 DC 现有的 serde 类型**（`src-tauri/src` 中的类型），不另行定义。

|方法|对应 DC 命令|params|result|
|---|---|---|---|
|`device.list`|`list_hid_devices` + 已知设备过滤|`{}`|`HidDevice[]`|
|`device.snapshots`|`get_device_snapshots`|`{}`|`Snapshots`|
|`battery.read`|`read_battery`|`{deviceId}`|现有 Value|
|`settings.read`|`get_device_settings`|`{deviceId}`|`SettingsSnapshot`|
|`settings.apply`|`apply_device_settings`|`{deviceId, change: SettingsChange}`|`SettingsSnapshot`（写后读回）|
|`mouse.dpi.read`|`read_atk_dpi`|`{deviceId}`|`DpiSnapshot`|
|`mouse.dpi.apply`|`apply_atk_dpi`|`{deviceId, change}`|`DpiSnapshot`|
|`mouse.mapping.read`|`read_button_mappings`|`{deviceId}`|现有类型|
|`mouse.mapping.apply`|`apply_button_mapping`|`{deviceId, change}`|现有类型|
|`keyboard.lighting.read`|`read_keyboard_lighting`|`{deviceId}`|现有类型|
|`keyboard.lighting.apply`|`apply_keyboard_lighting`|`{deviceId, change}`|现有类型|
|`lamp.config.read`|`get_lamp_config`|`{}`|`LampConfig`，token 只给末 4 位（`tokenTail`），不返回明文|
|`lamp.config.save`|`save_lamp_config`|`{ip, token?}`，token 省略表示保留原值|`LampConfig`（同上，打码）|
|`lamp.read`|`read_lamp`|`{}`|`LampState`|
|`lamp.apply`|`set_lamp`|`{change: LampChange}`|`LampState`|

- 禁止：原始 HID 读写、任意文件路径、宏、键盘映射、ATK 性能、小米云登录（后几项是二期，届时升 minor 加入）。鼠标灯效没有可用命令，不进入 capabilities。
- 所有 `*.apply` 都是“写入后读回”：result 是设备读回的值，不是请求值。读回与请求不一致时返回 `READBACK_MISMATCH`，`details` 带读回值。
- 同一设备的写入串行执行（挂灯沿用 `lamp_queue`）；同一设备已有写入在进行时，新的写入返回 `BUSY`。读取排在设备队列中，见 2.2 节。

## 5. 事件

|name|deviceId|data|
|---|---|---|
|`battery.updated`|有|与 `battery.read` 的结果相同|
|`lamp.updated`|无|`LampState`|
|`device.changed`|有|`{connected: bool}`；断开的设备在 UI 中保留原位置，显示“已断开”|
|`module.status`|无|`{state: "ready"\|"degraded"\|"stopping", message?}`|

- `seq` 从 1 开始，每个连接内单调递增。客户端发现 seq 有缺口时重新调用 `device.snapshots`。
- 事件中的秘密字段（token、云会话）一律打码或省略。旧 UI 的 `request-window-close`、`request-compact-exit` 不转发。

## 6. 错误码

`PROTOCOL_MISMATCH`、`UNAUTHORIZED`、`UNKNOWN_METHOD`、`INVALID_PARAMS`、`MODULE_UNTRUSTED`、`DEVICE_DISCONNECTED`、`BUSY`、`TIMEOUT`、`STALE_STATE`、`READBACK_MISMATCH`、`PARTIAL_WRITE`、`INTERNAL`。

- `retryable`：只有 `BUSY`、`TIMEOUT`、`DEVICE_DISCONNECTED` 为 true。
- 写入超时或 `PARTIAL_WRITE`：宿主不能宣称写入成功，也不能宣称已回滚，应提示“结果未知，请刷新读回”，并立即触发一次读取。

## 7. 宿主侧发现与信任（B2）

- 模块目录：`<exe 目录>/modules/device-center/`，manifest 校验复用 `crates/acer-system/src/device_center_module.rs`（`parse_manifest`、`validate_files`）。
- 信任快照：首次通过校验时，记录每个文件的 SHA-256。之后哈希与快照不一致时，状态为 `untrusted`，不启动；前端显示“模块文件已变更，确认后启动”，用户确认后才更新快照并启动。快照与 `keep` 存放在现有的 `device-center-plugin.json`，旧字段保持兼容。
- 目录监视：用 debounce（500 ms）后完整重新校验。目录消失时，入口隐藏、拒绝新请求、发送 `shutdown`；在途的写入不伪称回滚。
- 前端可见的模块状态：`absent | untrusted | stopped | starting | running | reconnecting | elsewhere | incompatible | error`，并附带 `pid`、`protocol`、`moduleVersion`、`capabilities`、`keep`、`message`，以及 `reconnectAttempt`（1–3，仅 `reconnecting` 时有值）和 `exitCode`（进程已退出时有值）。
  - v1.0.4（2026-11-07，对齐用户确认的预览）新增两个状态：`reconnecting` 表示断线重连中；`elsewhere` 表示子进程以退出码 3 结束，即已有其他实例在运行。界面表现以 `design/apple-redesign-20261105/README.md` 的“模块状态”一节为准。

## 8. Tauri 命令（宿主 → 前端，B2 实现，U3 使用）

|命令|参数|返回|
|---|---|---|
|`dc_module_status`|—|模块状态（第 7 节）|
|`dc_module_trust_and_start`|—|模块状态|
|`dc_module_set_keep`|`{keep}`|模块状态|
|`dc_call`|`{method, params}`|`{ok, result\|error}`，原样透传第 4 节|

- 事件转发：Tauri 事件 `dc://event`（payload 即第 5 节的 event）、`dc://module`（模块状态变化）和 `dc://readback`（宿主补读结果，见 2.2 节 v1.0.5）。
- `dc_call` 只放行第 4 节白名单内且在 capabilities 中的方法。
