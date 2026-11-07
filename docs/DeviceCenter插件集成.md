# Device Center 外设插件集成

来源：用户指定对话“整理项目文件并编写 AGENTS.md”，实际项目已迁移至 `D:/ChatGpt/DeviceCenter`，负责ATK F1鼠标、KX75键盘和小米挂灯1S。当前集成的是独立应用管理，不复制设备协议、米家账号或GPL应用代码。

## 使用

打开“服务插件”→“Device Center 外设中心”。首次选择已安装的 `DeviceCenter.exe`、`device-center.exe` 或 `device_center.exe`。不能选择setup安装器；绑定只保存主程序路径和SHA256，不运行程序。原项目安装包在 `D:/ChatGpt/DeviceCenter/releases/DeviceCenter_0.1.1_x64-setup.exe`，本次未执行安装。

- 显示未绑定、未运行、正在运行和文件版本变化；启动按钮仅在绑定文件仍匹配时可用。
- 启动前页面确认；进程已运行时不再重复启动。启动请求提交不等于窗口或设备连接已成功。
- 原应用无跨进程打开/退出接口。已运行时从原应用托盘打开或退出；本控制台不强杀进程，不把关闭窗口误当退出。
- 当前用户自启动使用单独的 `AcerConsole.DeviceCenter` 注册表项；读回确认，只改本项。发现原应用已有对应路径启动项则提示在原应用管理，避免重复注册。换绑路径前需先关闭本控制台的旧启动项。
- 文件更新后哈希变化需重新绑定；这只是文件一致性检查，不是发行者签名认证。

## 用量与日志

按绑定可执行文件路径匹配进程，收集当次观察到的子进程，包含WebView。工作集、私有提交、句柄、线程按树求和；CPU按墙钟间隔和逻辑核数归一。最多120个样本；进程集合、创建时间或时钟发生变化时重建基线。第一条或无有效样本时CPU未知，不填0。进程树中新生/退出、独立外部进程仍可能造成采样盲区，不作为完整性能验收。

默认进入页查询一次；可手动刷新或选择每15秒刷新。离开页面停止定时器，隐藏窗口不采样。查询使用短命PowerShell/CIM，无额外常驻管理服务。

“查看日志”默认关闭，只读取本控制台的管理事件：绑定、启动、自启动变更和失败。**不是原应用的设备/云账号运行日志**，因为原应用当前没有这样的文件接口。不读取账户Token、加密缓存或设备配置。日志在控制台配置目录，64KiB轮转，最多保留当前与上一份；显示末尾32KiB，收起清空页面缓存。

## 接口及验证

- Rust：`crates/acer-system/src/device_center_plugin.rs` 与嵌入的同名PowerShell脚本。
- Tauri：`get_device_center_plugin`、`bind_device_center_plugin`、`device_center_action`、`get_device_center_management_log`，普通用户权限，不走设备提权helper。
- 前端：2026-11-07 起为 `DeviceCenterSettings.svelte`（模块与启动管理）和 `DeviceCenterPeripherals.svelte`（外设页，经 DC IPC v1.0.5），状态在 `dcState.svelte.ts`。旧的 `DeviceCenterPlugin.svelte` 已移到 `待删除-20261107/`。
- 已验证：前端构建、Rust测试、隔离未绑定状态的本机只读查询。新增测试覆盖操作枚举限制、安装器/相对路径拒绝和路径引号处理。
- 未执行真实安装、启动、绑定用户文件、自启动写入或设备操作；由用户功能实测。没有把只读空状态或编译成功称为真实插件功能验收。

2026-11-02交付：`artifacts/AcerConsole-Plugins-20261102`及同名ZIP。前端构建0错误0警告，工作区测试和Release构建通过。浏览器预览验证插件切换、日志展开；1280像素视口无横向溢出，截图见`evidence/device-center-plugin-preview.png`。浏览器无法调用Tauri，状态显示未知，不作为已绑定或运行证明。
