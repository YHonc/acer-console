# acer-system 接入 API

2026-11-01，系统集成模块。主代理负责将 `crates/acer-system` 加入 workspace 和桌面依赖。本模块没有导入即运行的行为。

## 当前用户开机启动

- `acer_system::get_startup() -> Result<StartupState, SystemError>`：只读 HKCU Run 的唯一名称 AcerConsole，不创建注册表键。enabled 仅在现有 REG_SZ 内容与当前主程序生成的命令一致时为 true；entry_present 表示名称存在，mismatch 表示已有值与本程序不匹配。
- `acer_system::startup_enabled() -> Result<bool, SystemError>`：只需 bool 的接入层可使用此便捷函数。
- `acer_system::set_startup(enabled: bool) -> Result<StartupState, SystemError>`：由显式用户操作调用；true 写入，false 删除该唯一值，最后读回。不得在启动、加载设置或刷新页面时调用。

固定键为 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，值名 AcerConsole。命令只能是 `"<std::env::current_exe()>" --tray`，只接受名为 acer-console.exe 的当前进程，没有 path/arguments/command 输入参数。普通主程序自己调用，不通过提权 helper 写别的账户的 HKCU。UI 可以展示 StartupState，不能把配置 JSON 中的布尔值当作注册表真实状态。

main 必须支持 --tray 启动语义；否则这里只能保证已保存启动项，不能保证后台启动行为。移动 exe 后旧路径会显示 mismatch，用户显式重新开启可更新为当前路径。Windows 系统策略或用户在任务管理器中禁用该启动项的覆盖状态不在 HKCU Run 值查询范围内。

## 设置文件

`read_settings(config_dir: &Path)` 和 `write_settings(config_dir: &Path, settings: &Settings)`，目录由 native 的 app_config_dir 提供，不从 WebView 接受。字段沿用当前桌面：theme（system/light/dark）、poll_interval_ms（1000..60000）、close_to_tray、restore_on_exit。读取缺失文件返回默认值，损坏或非法值返回错误。写入先校验，再写同目录临时文件、sync_all、rename 替换；仅调用写入 API 才落盘。保存设置不会修改开机启动项。

## 官方任务（第二阶段实现）

- `get_official_task() -> Result<OfficialTaskState, SystemError>`：只读固定 `\PredatorSenseLauncher`，返回 enabled 及定义 SHA256。任务不存在、权限拒绝或命令失败均返回错误，不当作已禁用。
- `backup_official_task(config_dir: &Path)`：显式备份动作，在 native config_dir 下创建固定 `official-task-backup/task.xml` 和 `manifest.json`，目录已存在则拒绝覆盖。保存原 Enabled 位和定义摘要，不改变任务状态。
- `disable_official_task(config_dir: &Path)`：显式禁用动作，必须先有有效备份，且当前定义与备份一致，之后固定执行禁用并读回。
- `restore_official_task(config_dir: &Path)`：显式恢复动作，检查保存 XML/manifest/当前定义，仅恢复原 Enabled 位，原来禁用仍恢复禁用。不创建或重注册任务，不覆盖安装器后来更新的定义。

任务名、schtasks 参数均为 native 固定值；命令路径通过 GetSystemDirectoryW 取得，不搜索 PATH，不经过 shell。只执行 `/Query /TN <fixed> /XML` 和 `/Change /TN <fixed> /ENABLE|/DISABLE`。XML 用 roxmltree 解析并处理 UTF-8/UTF-16LE，SHA256 对结构、属性和文本摘要，只忽略任务级 Settings/Enabled（不会忽略触发器自己的 Enabled）。目录由 native app_config_dir 传入，不从 WebView 收文件路径。备份不加密，应作为当前用户本地配置保存；摘要用于发现意外变化，不抵御有权改写全部备份文件的本地攻击者。

这些 API 同步执行固定 schtasks 子进程，应在 spawn_blocking 里调用；当前无子进程墙钟超时。备份失败不改任务；执行 Change 后若读回失败，实际状态未知，应重新查询，不自动重试。没有事务锁能防止其他安装器在校验与 Change 之间修改任务，应用内调用则通过 mutex 串行。

权限方案：查询/备份通常可在普通权限运行，实际权限由本机任务 DACL 决定；Change 被拒绝会保留原错误，不自动 UAC。可由用户在管理员终端显式运行既有固定任务脚本完成切换，或下一阶段实现独立、固定任务动作的受限提权通路；当前不将任意文件路径/命令加入硬件 helper，也不要求整个 WebView 提权。脚本与 crate 的备份清单格式不同，不能交叉恢复。

## 原生电源事件源

新增 `acer_system::power_watch`（当前目标 Windows x64）：

- `get_power_status() -> Result<PowerStatus, SystemError>` 调用 GetSystemPowerStatus，返回 ac_online、battery_percent、charging、battery_present、battery_saver；255/非法字节保留 None，无电池时电量和充电状态为 None。
- `PowerWatcher::start(capacity)` 创建后台线程、一个隐藏的原生顶层窗口及 1..1024 容量的 sync_channel；注册 GUID_ACDC_POWER_SOURCE 的 RegisterPowerSettingNotification。不创建 WebView，不请求权限，不启动 PowerShell。
- `recv_timeout` / `try_recv` 返回 PowerChanged 或 Resumed。原生 WM_POWERBROADCAST 的 PBT_APMPOWERSTATUSCHANGE 和 GUID 通知都只提示重读；恢复仅采用 PBT_APMRESUMEAUTOMATIC，避免 PBT_APMRESUMESUSPEND 追加通知导致同一次恢复被重复处理。事件不附带缓存电源值，core 收到后调用 get_power_status/读取真实设备快照。
- `shutdown(&mut self)` 发 WM_QUIT、等待后台线程注销电源通知、销毁窗口和注销窗口类；Drop 也尝试 shutdown。is_running() 只表示线程尚未结束，不能等同已收到真实电源事件。
- dropped_events() 记录满队列次数；窗口回调只用 try_send，不阻塞。满队列时保留一次待处理恢复标记和电源标记，恢复标记优先；因此通知可合并，不能以事件数计算插拔次数。core 发现 dropped_events 增加时应使规则基线失效并重读，不能重放积压状态。

接入：由 core 持有 watcher 并在自己的规则执行线程消费事件；Resumed 调用 RuleEngine::on_wake() 后重读能力、电源与快照，PowerChanged 触发真实快照读取。首次启动也应先查询当前状态建规则基线，不能把注册后初始通知当成实际转换。应用退出先停消费者，再 shutdown；或者消费者用有界 recv_timeout 检查退出信号。不得在 UI 主线程阻塞等待事件。

隐藏窗口使用真正的顶层窗口而非 HWND_MESSAGE，因为 message-only 窗口不接收广播。原生结构与指针仅在同一线程内保存，窗口销毁前 Sink 始终存活。注册失败会返回具体 Win32 错误，不伪装成正常运行。睡眠恢复事件只建议重新读取，不调用 SetSuspendState、不写任何硬件设置。

新增纯测试检查电源未知/无电池解码、恢复消息分类、队列满时恢复通知保留。主代理可统一运行 `cargo test -p acer-system` 和 `cargo run -p acer-system --example power_watch_smoke`；示例只注册、查询和等待约 3 秒后注销，不发起休眠、不切换电源。该示例成功只能证明注册/查询/注销链路，真实插拔和休眠恢复送达仍须单独验收。本对话暂未编译或执行新增示例。

Win32 常量、RegisterPowerSettingNotification 签名、MSG/WNDCLASSW 布局已对照本机官方 Windows SDK 10.0.26100.0 的 um/WinUser.h；AC/DC GUID 对照 um/WinNT.h 第 18190 行。

## 验证状态

本次没有读取/写入真实自启动项，没有保存真实用户设置，没有执行官方任务变更。主代理正在 Release 构建，当前未并发执行 Cargo；新增 crate 待统一编译。单元测试覆盖命令生成、设置校验、隔离目录文件往返及任务摘要/UTF-16解析，尚未运行新增测试，不以此替代真实登录启动验收。仅新增必要依赖 roxmltree 和 sha2，未更改根 workspace。
