---
name: ui-verify
description: Verify desktop UI changes without Tauri or hardware - build, headless-Edge regression checks, and live review in codeg's built-in browser. Use after editing apps/desktop UI, before merging UI work, or when asked to 检查/验收界面.
---

# 界面验证

两种方式，按目的选：

- **回归**（合并、打包前必跑）：脚本一次跑完多页面、深浅主题、两种宽度、减少动效和同步注入数据。
- **现场查看**（改 UI 时与用户一起看）：codeg 内置浏览器，所见即用户所见。

## 回归

1. 运行：`bash design/apple-redesign-20261105/run_checks.sh`（已构建过可加 `--no-build`；只跑部分：`run_checks.sh dc sync`）。脚本自己启动并关闭 `vite preview`，端口被占用时报错退出。
2. 完成标准：`check_app_dc`、`check_app_sync`、`check_app_shell` 全部 PASS；`check_app.py` 只允许性能页因 `°C` 坐标刻度被判 “fake” 的已知误报，其他 FAIL 都要修。
3. 抽看 `shots-app/` 中与改动相关的截图，对照 `preview.html` 的同一状态。

## 现场查看

1. `cd apps/desktop && npm run build`，后台启动 `npx vite preview --host 127.0.0.1 --port 4173 --strictPort`。
2. `browser_open_tab http://127.0.0.1:4173/`，用 `browser_screenshot` 看画面、`browser_console_messages` 看报错；只在其他工具答不了时用 `browser_eval`（每次可能需要用户批准，代码 ≤4000 字符）。
3. 内置浏览器宽度跟随面板、不能模拟减少动效和系统深色，这些结论只能来自回归脚本。
4. 结束时停掉 4173 端口上的 vite 进程。

## 每次都要核对的状态语义

当前值、草稿、读回、过期、未知分开显示；未知显示 “—”，没有伪造读数；温控曲线没有预填点；动画只由交互或真实值变化触发，减少动效下关闭。
