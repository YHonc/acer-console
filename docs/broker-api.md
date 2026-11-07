# 提权 broker 接口（实施中）

接入 crate：`acer-broker`（path 指向 crates/acer-broker；主代理负责加入 workspace）。构建二进制 `acer-helper`，发布时置于桌面主程序同目录。主界面显式“授权访问”操作在 blocking 工作线程调用 `BrokerClient::launch_user_initiated()`；该调用使用 runas，可能显示 UAC。普通 snapshot/apply 调用绝不自动拉起 helper。

客户端实例提供 `snapshot()`、`capabilities()`、`apply(Command)`。都是同步且有时限的调用，Tauri 应放在 spawn_blocking 内，返回 acer-protocol 的类型及 DeviceError。客户端持有启动得到的 helper 进程身份；不要从 WebView 接受 PID、管道名、可执行路径或启动参数。掉线后由 UI 明确反馈并重新提供授权按钮，不自动重试 apply。

helper 当前调用 `Device::connect_with_policy(WritePolicy::Verified)`。硬件写入范围由 acer-device 的当前 policy_allows、逐项前置条件和读回验证决定，具体与后端最新验收记录对照；本次 official_task 扩展不修改硬件策略。通用 `Device::connect()` 仍默认只读。RPC 不包含 AllowUnverified 或策略选择参数；新增授权必须经过 native 代码审查和逐项证据确认，不能靠 JSON 开关启用。

传输使用本机 Windows 命名管道，单个长度前缀 JSON 帧，版本 1、递增请求 ID；请求/响应上限 256 KiB。固定操作为 ping/snapshot/capabilities/apply/official_task，不提供任意 WMI 方法、命令或文件路径。两端核对连接 PID、进程用户 SID 和 Windows 会话；客户端还要求 helper token 已提升。DACL 只授权当前用户、Administrators 和 SYSTEM，拒绝远程客户端，并要求首实例，避免接入被抢占的同名管道。

## 固定官方启动任务 RPC

接入：`use acer_broker::OfficialAction`，通过已有已授权客户端调用 `client.official_task(OfficialAction::Query | Backup | Disable | Restore) -> Result<serde_json::Value, DeviceError>`。这里只表示四个可选枚举值，实际每次传入其中一个。调用同步，放入桌面现有 blocking 工作线程，并复用任务操作锁和 broker 锁。不会自行 runas；没有有效连接时由 UI 返回授权提示。

wire 示例：`{"op":"official_task","command":"query"}`，保持既有 operation 的 op/command 外层格式；动作是严格字符串枚举。未知动作、额外 path/task_name/arguments 字段及对象形式的自定义参数均反序列化失败。硬件消息格式不变。

helper 已通过同用户 SID、同会话及固定客户端 PID 校验后，使用 `SHGetKnownFolderPath(FOLDERID_RoamingAppData, 0, NULL)` 获取提升进程当前账户的真实 RoamingAppData，拼接固定 `local.phn16.acerconsole`；与桌面 app_config_dir 标识一致。GUID 已核对本机 Windows SDK KnownFolders.h。既不从 RPC 收目录，也不从 APPDATA/PATH 环境变量拼命令。任务名和 schtasks 绝对路径继续由 acer-system 固定；RPC 不开放通用文件或进程执行能力。

Query 仅查询任务和备份有效性；Backup 显式创建固定 official-task-backup 目录及文件，不覆盖已有备份；Disable/Restore 必须通过 acer-system 的备份/定义摘要校验后才执行，Restore 只恢复保存的原启用位。此功能没有改变 DACL，仍为当前用户、管理员和 SYSTEM，且双方用户 SID/会话检查保持原样，不支持跨账户管理员凭据。

返回 JSON 包含：

- `state`：实际查询到的 OfficialTaskState，查询失败为 null；成功时 task_name/enabled/definition_sha256 同时放在顶层，兼容当前前端。
- `backup_valid`、`original_enabled`：有效备份及其原启用位，后者没有有效备份时为 null。
- `error`：当前动作或读回失败原因，成功为 null。即使传输 Result 为 Ok，UI 仍必须检查此字段，不能显示虚假成功。
- 兼容字段 `backup_original_enabled` 与 original_enabled 相同，`backup_error` 单独表示备份无效原因。

本地已知文件夹解析失败或管道失败使用外层 DeviceError；动作执行失败尽量仍返回当前状态与 error。沿用服务端 15 秒操作预算，超时会结束 helper，会话返回失败；schtasks 可能已经改变任务，必须重新授权后 Query 确认，不自动重试 Disable/Restore。当前 acer-system 子进程没有独立墙钟超时，helper 退出不保证中止已启动的 schtasks。

主代理已报告 evidence/official-task-roundtrip.json 中管理员原生备份→禁用→恢复实测通过，当前任务 enabled=true。本次 broker 扩展仅做协议/路径测试和构建，不执行任务动作；实际桌面经 helper 的任务读写仍需由主代理统一验收。发布需使用新构建的 acer-helper.exe，旧的已启动 helper 不识别新动作，不能只替换主程序而保留旧会话。

本次验证：`cargo test -p acer-broker` 共 6 项通过（包括官方动作枚举、额外路径/任务名/参数拒绝、只读 KnownFolder 查询），`cargo build -p acer-broker --bin acer-helper --target-dir crates/acer-broker/target-validation` 成功。默认 target/debug/acer-helper.exe 被运行中进程占用，常规构建替换时返回 os error 5；没有结束现有进程，改为独立输出目录。新产物位于 crates/acer-broker/target-validation/debug/acer-helper.exe，需主代理统一切换和重新授权后联调。

超时：连接及请求传输共 10 秒；设备操作 15 秒；响应读取 20 秒；响应交付确认 2 秒；helper 无客户端 60 秒退出。非阻塞管道每 15ms 检查一次截止时间，客户端串行调用。helper 的 Device::connect 另有 15 秒初始化预算，初始化较慢可能使首次连接超时。超时表示结果未知，apply 不可自动重发。UAC 等待时间由系统交互决定，不包含在管道超时内。helper 如设备工作线程超时则结束服务，防止堆积未完成操作。客户端释放后 helper 最迟空闲超时退出。

服务端只接受严格递增的请求 ID；每个连接一问一答，客户端读完后发送确认，服务端才关闭句柄，避免同步 FlushFileBuffers 无限等待或过早关闭丢失回复。当前采用 Win32 的 PIPE_NOWAIT 轮询，后续若换为 overlapped IO 需保留相同身份和超时检查。这里的用户隔离不抵御已能注入同一用户桌面进程的攻击者；发布目录需使用正常安装权限保护 helper 文件。

早期主对话报告两项协议测试通过；本次包含 NOWAIT 连接确认、health 和官方任务扩展的 6 项测试及完整 helper 构建已通过。系统集成对话未自行发起 UAC 或硬件写入；主对话的实际只读提权联调见下节。其余安全和生命周期边界仍须逐项验收。

## 2026-11-01 主代理实机证据

主代理报告：实际桌面按钮触发 `authorize_device` → `runas` helper → 命名管道读取 snapshot/capabilities 成功，UI 显示本机真实温度和风扇转速。这验证了当前普通权限桌面进程经显式授权访问提升权限 helper 的只读端到端路径。没有执行硬件写入，因此不据此宣称模式、风扇或灯光控制已验收。这里引用的是主代理的本机观察；本对话没有独立重跑，也没有收到可附加的截图或原始 JSON 路径。

## 已确认的失效客户端问题与接入建议

更新：主代理报告已将显式 authorize_device 改为先清空旧 slot，再 launch；只有用户点击会请求提权，因此下述 Some(dead) 阻止重建的问题已在接入层修正。broker 新增 `BrokerClient::is_alive() -> Result<bool, DeviceError>`，通过保存的进程句柄调用 GetExitCodeProcess，可供后续避免对仍存活 helper 重复授权。接口尚未纳入本次正在执行的 Release 编译，需下一轮构建确认。它不检查管道/WMI 响应，也不自行清除桌面状态。

修正前的只读检查：`apps/desktop/src-tauri/src/main.rs` 中 BrokerState 持有 `Option<BrokerClient>`，当时 authorize_device 第 14 行仅在 is_none() 时创建客户端；get_snapshot 第 28–29 行和 get_capabilities 第 43–44 行只按 Some/None 分流。管道失败并不会清空 slot。helper 无客户端约 60 秒自行退出后，slot 仍为 Some，再点授权会直接返回 Ok，不能启动新 helper，后续读取继续报旧 helper 已退出。此为修正前的生命周期缺口记录，未实际等待 60 秒复现。

建议由主代理在桌面接入层增加以下逻辑，当前本对话不改 apps：

1. 使用 broker 库的本地健康接口 `BrokerClient::is_alive() -> Result<bool, DeviceError>`，只检查保存的 helper 进程句柄，不请求 UAC、不发硬件 RPC。alive 只代表进程存在，不代表 WMI 或管道可响应。
2. 桌面状态提供 `invalidate_broker` 内部方法，通过 Option::take() 释放失效会话。无需向 WebView 暴露 PID、管道或句柄。authorize_device 在用户显式点击时检查已有客户端；失效则先 take，再调用 launch_user_initiated；只有新建成功才写入 Some。取消 UAC 后保持 None 并返回取消错误。
3. 读取遇到确认的 helper 退出、断管或鉴权失效时清空 slot，向 UI 返回“授权连接已断开，请重新授权”。同一条失败读取不自动拉起 helper，不把一次错误后的授权缺失当作正常读数。设备级读错误应保留原错误，不能仅因某个温度字段未知就使整个会话失效。
4. 需要区分管道传输错误和远程设备错误时，建议库后续提供结构化连接状态/错误来源，避免桌面按错误字符串匹配。当前 DeviceError::Transport 同时用于部分设备与 RPC 错误，不足以无歧义决定全部失效策略。
5. 写入将来启用后，传输超时仍可能已经执行；invalidate 只释放会话，不撤销硬件、不自动补发 apply。重新授权后先读快照再决定下一步。

验证顺序：正常授权读取 → 停止轮询并等待超过 60 秒 → 旧客户端读取明确失败 → 点击授权出现一次新的 UAC 并恢复真实值 → 取消授权时不保存失效 Some。若未来改为并行 RPC，失效回调需携带会话 generation，只清除自身对应的 slot，避免旧请求把新授权会话清掉；当前代码持锁期间调用 RPC，暂不存在该并发替换路径。

## 主对话统一执行的只读联调

1. 等其他 Cargo 构建释放锁后运行 `cargo build -p acer-broker --bin acer-helper --example read_smoke`，再运行 `cargo test -p acer-broker`。不要从安装未完成时的旧产物启动。
2. 将 `target/debug/acer-helper.exe` 复制到 `target/debug/examples/acer-helper.exe`。示例只从自身同目录找固定文件；Tauri 交付同理，将 helper 与实际桌面 exe 放一起。这里由主对话安排复制，broker 不自动安装文件。
3. 在普通权限终端先直接执行 `target/debug/examples/read_smoke.exe`，应只显示用法并以 2 退出，不触发 UAC。
4. 主对话决定开始联调后执行 `target/debug/examples/read_smoke.exe --authorize`。会显式请求同一用户的管理员授权。取消时应返回错误；不会自动再次请求。如果输入另一管理员账号，SID 不一致会被拒绝，当前实现不支持跨账户提权。
5. 授权成功后示例依次读取 capabilities、snapshot、capabilities，共三次不同 ID 的独立连接，输出三行 JSON 并以 0 退出。确认快照中的错误没有被伪装成数值 0；字段解码是否正确仍由后端与官方对照验收。
6. 示例退出后观察 helper 最迟约 60 秒空闲退出。Tauri 普通刷新只调用已授权实例，退出或掉线后不自动重发 apply，也不自动请求 UAC。联调不调用 apply、不修改灯光/风扇/模式/电池设置。

读权限、用户取消、helper 缺失、跨账户 SID 不一致、连接/设备超时都应显示具体错误并保留重新授权入口。管道鉴权失败会中断连接；设备错误在固定响应 result 中返回。初始化 Device::connect 失败时 helper 退出，客户端可报告退出码；此阶段的具体 WMI 原因写入 helper stderr，GUI 发布后需由主代理接入日志收集。

## 静态审查记录

- 通用 Device 默认 ReadOnly；helper 当前固定 Verified。acer-device::policy_allows 限制机型/BIOS及已验收命令，再由逐项前置条件和读回验证控制写入；RPC 不包含策略字段。硬件实验由主代理统一执行，系统集成对话没有自行复跑。
- 管道拒绝远程、要求第一实例；普通用户 ACL 使用显式数据读写权限 `0x12019b`，排除 FILE_CREATE_PIPE_INSTANCE，避免 GENERIC_WRITE 隐含该权限；管理员和 SYSTEM 保留访问。
- 两端从命名管道取得实际 peer PID，再查进程 token 的 SID/session，客户端还核对启动时取得的 helper PID 及 elevation；未把 JSON 自述身份作为凭据。
- ShellExecuteEx 使用固定同目录 helper，runas 在单独 STA 线程执行，句柄 RAII 关闭；不提供任意路径或命令参数。
- 帧长先检查再分配；JSON 限制固定操作与顶层字段，serde_json 自带递归限制；请求 ID 在客户端互斥锁内分配，服务端拒绝倒序或重复 ID。超时不重试操作。
- PIPE_NOWAIT 的 ConnectNamedPipe 非零返回仍额外确认实际客户端 PID，避免把“可连接”误判为“已连接”。响应使用有界确认读保持服务端句柄，不使用可无限阻塞的 FlushFileBuffers。
- 尚需本机动态验证：DACL 实际访问、同会话身份限制、UAC 取消、客户端提前退出、慢设备、连续读取和空闲退出。静态审查与编译不替代这些验收。
