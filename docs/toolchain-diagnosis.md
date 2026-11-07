> **历史阶段文档（2026-11-02整理标记）**：下文保留当时的计划和证据，不代表当前范围或最新包状态。UI现已选定并集成Gemini/daisyUI，电池校准取消，服务插件已接入，功能实测由用户负责。当前结论以[当前进度](当前进度.md)及[目标与验收](目标与验收.md)为准。旧产物路径见迁移记录。

# VS Build Tools 安装只读诊断

2026-11-01 16:04–16:05，Asia/Hong_Kong。结论：安装会话正在继续下载 Windows 11 SDK 的 CAB 包；已有多个包执行成功，实例尚未完成。当前没有等待 UAC 或安装已失败的证据。保留现有安装进程，等待同一会话完成。

## 进程证据

查询 Win32_Process：winget PID 54724（15:49:40）→ vs_BuildTools.exe PID 49188（15:49:47）→ vs_setup_bootstrapper.exe PID 38288（15:49:49）→ setup.exe PID 28356（15:52:04）→ setup.exe PID 47980（15:53:28）。winget 命令请求 Microsoft.VisualStudio.2022.BuildTools，安装参数为 `--wait --quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended`。未取消或重新启动进程。

普通权限下部分 setup 进程路径/命令行不可读取；这不等于进程卡死。查询时没有发现 consent.exe；日志已经进入 elevate 后的安装执行与下载，故不能把当前阶段归为等候 UAC。

## 日志证据

主日志：`%USERPROFILE%\AppData\Local\Temp\dd_setup_20261101155329.log`，文件在读取期间持续增长（首读 23386 行，随后超过 23406 行）。仅提取安装状态，不复制其中遥测标识或键值。

- 第 1 行：15:53:29 Setup Engine v4.11.24。
- 第 23 行：15:53:31 开始 Install operation。
- 第 18940、18949、19219、19381 行：15:53:33 至 15:54:04 均为 `Package executed successfully. Return code: 0`，后面还有多项同类成功记录。
- 第 23338 行：16:04:27 完成下载 `b82881a61b7477bd4eb5de2cd5037fe2.cab`。
- 第 23360 行：16:04:51 完成下载 `e87de81a902e0c8cde9e1355c3ff5f3f.cab`。
- 第 23366–23371 行：16:04:56 完成多个 CAB，继续开始 `eca0aa33de85194cd50ed6e0aae0156f.cab`。
- 目标路径位于 TEMP 下 `gocjyw23\Win11SDK_10.0.26100.F872955055CDB6106FAD\Installers`，下载来源为 Microsoft Visual Studio 下载站。
- 同会话 `dd_setup_20261101155329_errors.log` 为空。组件子日志最新写入约 15:57:16，而主日志仍继续下载；不能只按子日志更新时间判断安装停滞。

开头的旧 Windows 版本包 `not applicable` 记录表示条件筛选，不作为失败证据。空错误日志和部分包返回 0 也不代表整次安装成功；尚无最终成功退出码。

## 实例状态

使用本机 `C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe -all -products '*' -format json`，已能看到实例 `012541af`：BuildTools 17.14.37827.9（产品显示版本 17.14.44），路径 `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools`，state=13、isComplete=false、isLaunchable=false、isRebootRequired=false。

因此先前普通 vswhere 返回 `[]` 不能证明未开始安装；必须包含 `-all` 查看未完成实例。目前只确认实例已注册且尚未就绪，不把编译器文件出现视为工具链验收。

## 后续判断点

由主对话继续观察现有 winget 会话 67212 的最终退出码。完成后再通过 vswhere 的完整实例与 VC Tools 组件查询确认路径，并实际执行 Rust MSVC 编译/链接。若日志停止增长且会话不退出，再根据当时尾部日志判断具体包或网络错误。本次仅查询进程、日志和实例，并写本文档，没有取消、重启、提权或修改系统。
