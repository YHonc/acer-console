# 项目约定

- 技术栈：Rust workspace + Tauri 2 + Svelte 5（`apps/desktop`）。daisyUI 依赖保留，但不加载其 components 层，界面样式以 `apps/desktop/src/tokens.css` 为准。
- UI 基准是 `design/apple-redesign-20261105/`（预览与动效令牌）。新界面使用 `tokens.css` 的令牌和全局类（`.group .row .sh .tag .btn` 等）与 `lib/ui/` 组件；动画只由交互或真实值变化触发，必须支持 `prefers-reduced-motion`。
- 硬件状态语义不可破坏：当前值、草稿、读回、过期、未知分开显示；未知显示“—”，不伪造读数；温控曲线不预填点。
- 硬件写入、启动项、电源、磁盘、引导和安全设置的改动，以及任何功耗写入，都需要用户逐项明确批准。实机功能测试由用户执行，代理只做构建、离线测试和浏览器检查。
- 清理文件时先移到 `待删除-<日期>/` 并写说明，由用户删除；用户证据（`evidence/`、`research/`）原样保留。
- 第三方安装包、发行二进制和嵌套仓库不入库，在 `docs/第三方二进制本地位置.md` 记录来源与 SHA-256。
- 发布包输出到 `artifacts/<名称>`（已忽略），用 `scripts/Build-Release.ps1`，Device Center 模块用 `scripts/Package-DeviceCenterModule.ps1`（流程见 `docs/DeviceCenter模块打包说明.md` 末尾）。

## 验证

- 合并前的完成标准：`cargo test --workspace --locked` 全过；`apps/desktop` 中 `npm run check` 0 错误 0 警告、`npm run build` 通过；改了 UI 时，`vite preview` 跑 `design/apple-redesign-20261105/check_app_*.py` 通过（`check_app.py` 性能页的 `°C` FAIL 是已知误报）。
- 报告测试结果时给出命令和通过数；代理自报的结果，合并前由主代理在本地复跑。

## 环境坑

- 只有 Windows PowerShell 5.1：含中文的 `.ps1` 存为 UTF-8 **带 BOM**；从 bash 调用用 `scripts/ps.sh`，它把输出切到 UTF-8 并保留退出码。
- Device Center 模块必须用 GNU 工具链构建（MSVC 静态链接 WebView2 loader，打包脚本会因缺 `WebView2Loader.dll` 拒绝）。
- 浏览器检查脚本用 Playwright 的 `channel="msedge"`。

## 协作

- 模型分工、Codex 委派与审查流程见 `docs/代理协作.md`；委派或审查前先读它。
- 每个里程碑（合并、打包、阶段完成）结束时，更新 `docs/当前进度.md` 与 `HANDOFF.md` 的“当前目标/下一步”，让下一个会话能直接接手。
