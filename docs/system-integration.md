# 系统集成辅助脚本

2026-11-01，本机只读核验。本文配合 `Rust控制台路线.md` 的 P0/P4，脚本不管理服务启停。

## 资源采样

在项目根目录运行：

```powershell
# 默认输出 JSON 到 stdout，600 秒；OutputPath 才会写报告文件，父目录需已存在。
.\scripts\Measure-Resources.ps1 -Scenario official-visible -OutputPath .\evidence\official-visible.json
# 新应用用自己的名字或已运行主进程 PID 指定；仍单独标记保留服务。
.\scripts\Measure-Resources.ps1 -RootProcessId 1234 -ProcessNamePattern '^acer-console\.exe$' -Scenario replacement-tray
```

默认匹配 PredatorSense*.exe、AcerCentralService.exe、AcerLightingService.exe，另外按 AASSvc/AcerLightingService 当前服务 PID 关联，递归纳入可见后代，包括 WebView。输出每项 PID、父 PID、创建时间身份、可读取路径、累计 CPU 秒、内存及服务状态。同一进程只计一次；创建时间防止把 PID 重用的计数相减。显式 RootProcessId 在首帧锁定身份，已识别后代在父进程退出后仍继续跟踪。

CPU 为两帧累计用户态加内核态秒数之差，除实际观测间隔和逻辑 CPU 数，再乘 100，采用全机百分比。首帧/新进程无差值时为 null，报告匹配数量。总 CPU 是当帧可匹配身份之和；最终另给按实际间隔加权的平均值。P95 用最近秩法，内存均值是样本平均。查询本身的耗时包含在实际间隔内，采集总时长可能超过请求时长。

WorkingSetBytes 是驻留工作集，包含共享页，多个进程求和会重复计算共享页；PrivateCommitBytes 是 Win32_Process.PrivatePageCount（私有提交虚拟内存），**不是私有工作集**。PrivateWorkingSetBytes 来自 Win32_PerfRawData_PerfProc_Process.WorkingSetPrivate，可用于路线中的私有驻留工作集预算。性能计数器另次查询按 PID 关联，进程恰在两次查询间退出或 PID 重用会影响对应样本，须结合身份与匹配计数检查；任一已选进程缺少计数器时，总私有工作集为 null，错误写入 CollectionErrors。按进程 Group 可拆分应用及保留服务成本；其他必须保留服务应显式扩充 ServiceNamePattern。

采样无法捕获两帧之间退出的进程，无法可靠归属开始采样前已经成为孤儿的共享 WebView 进程，也不通过命令行猜测其他应用的 WebView。应在目标进程树仍完整时开始，并检查 Processes 中路径/父子关系；需要共享运行时全量分析时另做 ETW/性能计数器采集。没有读取温度或执行硬件写入。

正式比较应保持同机、同电源、同负载，Release 包预热后各采官方前台、官方后台、新应用前台、新应用托盘各 10 分钟、至少 3 轮。这次短时运行只验证脚本采样与汇总，不构成性能基线。

## 启动任务备份、禁用与恢复

```powershell
.\scripts\Manage-Startup.ps1
# 下面默认仅预览，不创建备份目录或改变任务。
.\scripts\Manage-Startup.ps1 -Action Disable -BackupDirectory .\evidence\startup-20261101
# 用户决定实际变更时显式加 -Apply，仍会由 ShouldProcess 确认；支持 -WhatIf。
.\scripts\Manage-Startup.ps1 -Action Disable -BackupDirectory .\evidence\startup-20261101 -Apply
.\scripts\Manage-Startup.ps1 -Action Restore -BackupDirectory .\evidence\startup-20261101 -Apply
```

Backup/Disable 必须指定一个全新备份目录；实际禁用前先写 task.xml 和 manifest.json，保留原启用位和定义摘要，不覆盖已有备份。单独备份使用 Action Backup 加 Apply。Restore 仅恢复原 Enabled 状态，原来禁用则仍恢复禁用；需要原任务仍存在，且定义（忽略任务级 Enabled）与备份一致，防止覆盖安装器或用户之后修改的任务。XML 保留完整定义供人工审查；删除任务、重新注册定义、口令和账户迁移不在自动恢复范围。修改任务可能需要管理员权限。禁用登录触发不终止已经运行的官方进程。

## 本机事实

只读查询时间：2026-11-01（香港时间）。

- `\PredatorSenseLauncher`：Ready，Enabled=true；MSFT_TaskLogonTrigger，触发器 Enabled=true、Delay=PT20S、UserId 为空。
- 执行文件：`C:\Program Files\PredatorSense\Prerequisites\PredatorSenseLauncher.exe`，参数和工作目录为空。
- Principal：RunLevel=Limited、LogonType=Group、GroupId=Authenticated Users。
- 按任务名/路径 Acer 或 Predator 查询仅发现上述任务；不代表系统不存在其他名称的关联启动项。

|服务|当时状态|启动模式|
|---|---|---|
|AASSvc（Acer Agent Service）|Stopped|Auto|
|AcerLightingService|Running|Auto|
|AcerARTAIMMXDriverService|Running|Auto|
|AcerARTAIMMXService|Running|Auto|
|AcerCCAgentSvis（Care Center）|Running|Auto|
|AcerDeviceEnablingServiceV2|Running|Auto|
|AcerDIAgentSvis（Device Info）|Running|Auto|
|AcerPixyService|Running|Auto|
|AcerQAAgentSvis（Quick Access）|Running|Auto|
|AcerServiceSvc|Running|Auto|
|ADESvc|Running|Auto|

以上 11 项 Get-Service 的 RequiredServices/DependentServices 均为空，即 SCM 未声明依赖；应用/驱动的运行时依赖没有因此被排除。AASSvc 和 AcerLightingService 均来自 DriverStore 下 predatorservice.inf 对应目录，文件分别为 AcerCentralService.exe 和 AcerLightingService.exe。没有做停服务实验，不能推断任意服务可移除或禁用。

## 验证记录

补充：已在 Windows PowerShell 5.1 实跑新增私有工作集采样，6 秒请求实际约 6.28 秒、3 帧、2 个有效 CPU 差值，PrivateWorkingSetBytes 三帧均有效，CollectionErrors 为空。该短时默认进程选择样本只用于验证采集链路，不能当作官方前台或后台的正式性能结论。

已实际运行 Measure-Resources 的短时只读路径，选取执行 PowerShell PID，识别本机 20 逻辑 CPU，输出 2 帧，CPU 首帧空、次帧有效，工作集和私有提交汇总均为正数。已运行默认 Inspect、Disable 无 Apply 预览，确认 Applied=false、任务 Enabled=true、预览备份目录未创建。另用 Apply + WhatIf 检查写入门控。未实际执行备份、禁用、恢复、服务启停或重启验收；恢复路径需后续在测试任务上验证。
