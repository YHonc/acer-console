---
name: test-package
description: Build the user test package (console + Device Center module, zip, manifest) with one command, then record it. Use when asked to 打包, make a 测试包 or release build for the user to test.
---

# 测试包

产物：`artifacts/AcerConsole-<名称>/` 与同名 `.zip`。只构建、打包，不启动程序、不碰硬件。

## 步骤

1. **定名称**：`<主题>-<YYYYMMDD>`，如 `DCIPC-20261108`；`artifacts/` 下不能已存在。
2. **确认输入已提交**：控制台工作区建议干净（脏工作区会在清单中标 `commit_dirty`）；Device Center 源码必须干净，脚本会拒绝未提交的 DC 源码。默认 DC 源码与工具链见脚本参数（`-DeviceCenterSource`、`-DeviceCenterTools`），分支选择以 HANDOFF 为准。
3. **运行**（后台，约 10–20 分钟；DC 提交未变时复用缓存，快很多）：
   ```
   bash scripts/ps.sh scripts/Build-TestPackage.ps1 -Name <名称>
   ```
   完成标准：输出末尾的 `== done` 段，含 zip 路径、字节数、SHA256、条目数和两个提交号。任何 `throw` 都是失败，按信息修复后换新名称重跑。
4. **记录**：把 zip 的 SHA256、大小、控制台与 DC 提交写进 `HANDOFF.md`「当前目标」与 `docs/当前进度.md` 顶部；替代旧包时，把旧包移到 `待删除-<日期>/` 并在说明里登记；提交。
5. **告诉用户**：包的位置、哈希、相对上一包的变化、需要实测的项目（从 HANDOFF「下一步」取）。

失败排查（工具链、WebView2Loader、Node 22 测试写法、BOM）见 `docs/DeviceCenter模块打包说明.md` 末尾的实际打包记录。
