# 当前用户的粘滞键直接控制

2026-11-01，替代程序把粘滞键从官方 Agent 转至桌面进程的 Win32 `SystemParametersInfoW`。后台服务可能运行在不同会话；此实现明确操作运行界面的当前用户。开启同时置 `SKF_STICKYKEYSON` 和 `SKF_HOTKEYACTIVE`（掩码 0x5），关闭同时清除，保留其余选项。界面明确说明会同步切换连按 Shift 快捷键。

`crates/acer-system/src/sticky_keys.rs` 实现读取、写入和独立读回。`fWinIni=0` 与本机官方调用一致，不额外写入用户配置注册表。写入接口只接受布尔值，未向 WebView 开放任意 flags。

## 实机证据

- `evidence/sticky-keys-direct-restore.json`：普通权限原 flags=506，开启读回 flags=511，恢复读回 flags=506，restored=true，探针退出 0。
- 随后内置前端的 Debug 桌面完成无 helper、无设备授权的开启→关闭，界面两次均显示读回一致。独立重新运行只读探针确认 flags=506、enabled=false、hotkey_active=false。
- 系统模块 10 项测试通过；桌面 cargo check 通过；前端构建 0 errors / 0 warnings。测试包含不改变其余 flags，以及仅启用快捷键时不误报粘滞功能已开启。

三项硬件设置现已改为受限 helper 的直接 WMI 路径，正式包 `artifacts/AcerConsole` 已包含本模块。本项完成不代表已可停掉全部官方服务，音频等仍在补齐。
