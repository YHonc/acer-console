# Device Center 文件夹模块打包

脚本：`scripts/Package-DeviceCenterModule.ps1`。接口遵循 `DeviceCenter模块接口约定-2026-11-03.md`。仅打包已完成构建，不构建、不运行应用、不安装、不读账号配置、不修改 D 盘源项目。原项目保留。

## 当前调用方式

等待主代理确认原项目 release 构建完成后，再执行：

```powershell
./scripts/Package-DeviceCenterModule.ps1 `
  -SourceRoot 'D:\ChatGpt\DeviceCenter' `
  -BuildDirectory '%USERPROFILE%\AppData\Local\Temp\device-center-target-C9E33D96061F\release' `
  -OutputDirectory 'E:\逆向宏碁控制台\modules\device-center'
```

`OutputDirectory` 是模块根目录，不是其父目录；必须不存在，且不能与 SourceRoot/BuildDirectory 互相包含。输出先在同父级临时目录完成核验，再移动为正式模块。失败保留临时目录供检查，不自动递归删除输入或旧产物。再次打包请选新目录。

## 模块内容

- `DeviceCenter.exe`：从构建目录中的 DeviceCenter.exe 或 device-center.exe 复制并统一入口名；若两个同时存在则拒绝猜测。
- `WebView2Loader.dll` 与 PE 导入表所需的构建目录本地 DLL：只做文件解析，不加载 DLL。Windows 系统/API-set DLL 不复制；未解析的非系统依赖会使打包失败，应先将正确架构 DLL 提供到构建目录，不从旧发布包或任意 PATH 猜选。加载器之外的动态加载插件和完整设备行为仍需运行验收。系统 WebView2 Runtime 需预先安装，文件夹模块不含安装器或固定版浏览器运行时。
- `LICENSE`、`LICENSE-ATK`、`UPSTREAM.md`、`THIRD_PARTY_NOTICES.md`：保留项目声明的 GPL/MIT 及上游来源材料。保留现有材料不等于完成全部传递依赖许可证审计。
- `module.json`：只有约定的 schema_version/id/name/version/entry/files；files 列出入口与实际复制的运行 DLL 的真实 SHA256，包含入口且不重复。版本读取当前 package.json 与 tauri.conf.json，两者必须一致。
- `DeviceCenter-source-<version>.zip`、`source-manifest.json`：现场采集的对应工作区源码白名单，不复制历史 source ZIP。manifest 同时记录原文件与 ZIP 文件内容哈希，Markdown 的本地路径/设备地址等脱敏；产品源码字节不改。当前源码快照与输入 EXE 用 SHA256 建立关联，不声称具有 Git 提交构建证明或逐字节可复现性。
- `packaging-report.json`：完整输出文件清单与哈希（不递归记录自身）、系统导入依赖及离线核验范围。源码 ZIP/许可证哈希由此记录，宿主 runtime files 无需包含它们。

选择该文件夹只绑定与校验；真正启动仍由宿主既有显式确认流程完成，普通用户权限，不经过 Acer helper。打包不移动或改写原账号/设备配置；程序自己的 Windows 配置目录与此模块目录是不同位置。

## 源码白名单与隐私

沿用原项目 archive-source.ps1 的构建输入思路：根目录源码/锁文件/许可证，src-tauri 的 Cargo/构建配置/脚本/许可证，src、src-tauri/src、capabilities、icons、tests、scripts、可选 public、assets/icons-v2，以及 docs 顶层 Markdown。没有复制 docs/history、docs/evidence、研究网站快照与历史发行物。

排除并拒绝白名单中出现 .tools、node_modules、.git、target、references、releases、artifacts、archive、环境文件、账号会话/凭据、DPAPI/dat/密钥/证书、日志/CSV/JSONL、EXE/DLL/ZIP 等非源码输入；EXE/DLL 只通过单独的运行文件管线进入模块。选择文件时拒绝重解析点。不会读取 AppData、凭据库或源码目录外的用户账号数据。

打包会校验 ZIP 条目与内容哈希、源码在采集期间是否变化、运行文件复制与输入是否稳定。请在构建成功和源代码冻结后执行；不是从任意变化中的工作区推断“必然精确对应已构建 EXE”。

## 模板与验证边界

`templates/device-center/module.example.json` 是格式示例，零哈希为占位值，不能用于实际绑定；正式清单必须由脚本生成。脚本与模板可先做 PowerShell AST/JSON 离线检查。实际模块打包等待构建完成通知后运行。应用启动、账号复用、设备读写与独立删除模块后的宿主状态由主代理/用户另行验收；打包成功不等于这些功能已验收。

## 2026-11-03 实际打包记录

主代理确认 Device Center 原 release 构建 session90087 退出码 0（约 4m20s），前端 0 错误/0 警告；Rust 有未使用 apply 警告，GNU linker 有 .rsrc multiple non-default manifests 与重复 ICON 资源警告。该构建不是无警告构建。本打包流程未修复原项目资源代码，也未启动应用。

模块已输出至 `E:\逆向宏碁控制台\modules\device-center`，原路径此前不存在，没有覆盖未知文件。PowerShell AST、模板 JSON、实际 PE 普通/延迟导入、复制哈希、141 个白名单源码文件、ZIP 条目/内容及打包期间输入稳定性核验通过。

使用原项目已有 GNU objdump 交叉检查：DeviceCenter 是 pei-x86-64、Windows GUI 子系统、入口 RVA 0x1030。直接导入未发现 libgcc/libstdc++/libwinpthread；实际本地运行依赖只有 WebView2Loader.dll。loader 本身为 x64，普通依赖 KERNEL32，延迟导入 ADVAPI32/ole32，均为系统依赖。因此本次无需复制 GNU 运行库，不是只凭文件名猜测。尚不能排除未在 PE 导入表描述的动态加载及目标机器 WebView2 环境问题。

资源目录有 3 个同 ID 的 RT_MANIFEST 条目，两份 Common-Controls assembly 及一份声明 asInvoker/uiAccess=false 的 assembly，并有重复 ICON/版本语言条目。这与链接器警告相符；找到 asInvoker 文本不等于证明 Windows 运行时会选择哪份重复资源。原生启动、DPI/图标和 manifest 生效仍待验收，不能用离线结构检查替代。

| 文件 | SHA256 |
| --- | --- |
| DeviceCenter.exe（29,086,581 字节） | `4449898A6A17B6E49D2194194981E0A2C12F5EDC05B0692A7A27960CCE99926F` |
| WebView2Loader.dll（160,880 字节） | `86545B66CDB0603BC26B626FB9AD610CB6E71F28D468F5EA66DF23B03DDA96D5` |
| module.json | `CCBE2A0290A16F0A195A9B00E6202443AFC58F8E4706705D75BBDB598D709FE2` |
| DeviceCenter-source-0.1.1.zip | `8348AECC00ECD6B7C741023B5286CFC9B312C1B1E9034AF95E1DB8B5D50D3142` |

模块内 source-manifest.json 逐项记录源码哈希；packaging-report.json 记录全部输出文件（自身除外）的哈希。最终 runtime files 与实际文件再次核对一致。未读取或迁移账号数据；D 盘原目录及原构建输出均保留。

## 2026-11-07 实际打包记录（DCIPC 测试包）

- **源码**：`D:\ChatGpt\DeviceCenter-dc-ipc-ui-integ`，分支 `cli/dc-ipc-ui-integ`，提交 `8ca1faf`。该分支以 B1 headless IPC 为基础，叠加了主仓库中未提交的 0.1.1 改动。
- **构建**：`src-tauri/build-windows.ps1` 要求使用 worktree 自带的 `.tools`，但 worktree 里的 `.tools` 为空。因此本次沿用该脚本的 `release` 环境变量和参数，改为使用 `D:\ChatGpt\DeviceCenter\.tools` 中的 GNU 工具链（cargo、rustup、WinLibs），并把 `CARGO_TARGET_DIR` 设为 `%TEMP%\dc-integ-target-20261107`。构建后，从 `webview2-com-sys-0.39.1/x64` 复制 `WebView2Loader.dll`。
- **注意**：本机默认的 MSVC 工具链会静态链接 WebView2 loader，构建目录中不会有 `WebView2Loader.dll`，本脚本因此会拒绝打包。打包时请使用 GNU 构建。
- **测试**：`package.json` 中的 `node --test tests` 在 Node 22 下会把目录当作模块，报 1 项失败。改用 `node --test "tests/*.mjs"` 后，56 项全部通过。`cargo test` 109 项通过，4 项被忽略。
- **打包**：修复了脚本在 Windows PowerShell 5.1 下缺少 `System.IO.Compression` 程序集的问题，然后打包到 `artifacts/AcerConsole-DCIPC-20261107/modules/device-center`。

| 文件 | SHA256 |
| --- | --- |
| DeviceCenter.exe | `76C7C437A2C2EFA1D192083599C412AE26D644D5A73DB53B2582DD1B63F2FF15` |
| WebView2Loader.dll | `86545B66CDB0603BC26B626FB9AD610CB6E71F28D468F5EA66DF23B03DDA96D5` |
| module.json | `EAE4DB7C06A45DD5EF438E74537055B4C98601188E85E0EE0E86FFA2EA3D1DD2` |
| DeviceCenter-source-0.1.1.zip | `D598E4E22EFE229C6AA0024E05D6327A38F3C861AB853A2DEEB99A526FCA9664` |

本次未启动应用，也未进行任何设备读写。

## 2026-11-08 一条命令打包

`scripts/Build-TestPackage.ps1 -Name <名称>`（从 bash 运行：`bash scripts/ps.sh scripts/Build-TestPackage.ps1 -Name <名称>`）按顺序完成以下工作：控制台构建与全部测试、Device Center 的 GNU 测试与 release 构建、模块打包、复制用户文档、写清单、打 ZIP，并做条目自检。

- Device Center 源码有未提交改动时，脚本拒绝打包，保证模块能对应到具体提交。
- 构建缓存在 `%TEMP%\device-center-gnu-target`，用标记文件记录源码提交。提交不变时跳过 DC 重编，加 `-RebuildDeviceCenter` 可以强制重编。
- ZIP 不用 `Compress-Archive` 生成。PowerShell 5.1 的 `Compress-Archive` 会让子目录条目使用 `\`；Windows 上的 python `zipfile` 读取时会自动把它换成 `/`，所以看起来正常，必须检查 `orig_filename` 才能发现。脚本改用 .NET `ZipArchive` 逐个写入条目，统一使用 `/`，并和包目录逐项对比。
