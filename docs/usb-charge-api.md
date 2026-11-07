# acer-controls USB 关机充电 API

新 workspace 包名 **acer-controls**，位置 crates/acer-controls，主代理负责注册根 workspace。依赖 acer-protocol、serde、serde_json；Windows WMI 固定使用与现有设备层一致的 wmi=0.18.4。实现原理见 usb-charge-protocol.md。

## native 接入

- `UsbDevice::connect()`：创建固定 COM 工作线程，默认 ReadOnly。先读取精确机型/BIOS，只有 PHN16-71（含官方 Predator 前缀）/V1.18 才连接 USB 控制路径；不提权、不改设置。
- `UsbDevice::connect_with_policy(WritePolicy::Verified)`：生产 native 受限入口；只允许本机目标、合法开关/阈值、真实初读确认及 ACK+独立完整 pair 读回确认。默认 connect 仍为 ReadOnly。`SupervisedValidation` 保留供显式实验，WritePolicy 不可由 JSON 反序列化。
- `get() -> Result<UsbRead, DeviceError>`：固定 APGeAction.GetFunction，uiInput=UInt32(4)。保留完整 RawRead，包括 ReturnValue、uiOutput、原始错误。传输/协议错误在 raw.error，外层 Result 处理 worker 启动/停止错误。
- `set(UsbState) -> Result<UsbApplyReport, DeviceError>`：必须是非 ReadOnly native 策略，重新 GET 原 pair 且确认成功后才发 SET；uiInput=UInt64(packed)。没有任意 class/method/input 入口。
- `UsbState { enabled: bool, threshold: UsbThreshold }`，阈值通过 serde 以数字 10/20/30 表达；其他数字拒绝。state 字节只认 0x0F/0x1F；固件状态低字节非零、未知状态/阈值及未解释高位均不作为可写原值。

UsbRead.ack 为 confirmed/rejected/unknown。2026-11-01 主代理提升权限样本 `evidence/usb-charge-read.json` 显示本机 GET 仅返回 uiOutput=1969920（0x1E0F00），没有 ReturnValue，与官方 CallBIOSGetFunction 只读取 uiOutput 的代码吻合。仅对此 reviewed schema 兼容缺失返回值：class=APGeAction、method=GetFunction、唯一输入 uiInput=Unsigned(4)、唯一输出 uiOutput=Unsigned、低字节为 0、完整 state/threshold/高位检查通过且无 raw.error，才确认 GET 状态。其他 class/method/selector、额外输入输出、未知枚举或字段类型不满足时不授权写入；ReturnValue 显式 false、错误类型、固件状态非零或 raw.error 都拒绝。decoded_state 在 ack 未确认时仍只是候选值。没有套用 AcerGamingFunction 的通用例外。

SET 的缺 ReturnValue 兼容已根据主代理真实往返样本收敛：仅 class=APGeAction、method=SetFunction、唯一输入 uiInput 为合法 packed USB 命令（功能号4、state0F/1F、阈值10/20/30、无多余高位）、无 raw.error、唯一输出 uiOutput=Unsigned(0)，才把缺返回值视为 ACK 确认。非零 uiOutput（包括 0x100）、ReturnValue 显式 false 或错误类型仍拒绝；其他未审查 schema 仍为 unknown。此例外不推广到其他类/方法，Applied 仍额外要求独立 GET 的真实 pair 等于目标。

SET 后同时要求 setter ACK 确认和独立 GET 确认开关、阈值均等于目标，才能返回 Applied；其他情况 Failed/Unconfirmed。writes[].attempted 只在进入 exec_method 之前置 true，实例/参数准备失败不会冒称已派发。调用可能在返回错误前已经改变硬件，attempted=true 时不能据 Failed 推断未改变。读取输出途中失败时仍保留此前已取得的字段与错误。

接口自身串行执行并在工作线程拥有 COM。接入 broker 时应复用现有的有界等待/超时处理，不在 WebView 主线程同步等待；当前 UsbDevice mpsc 等待没有独立截止时间。超时、用户强杀或进程崩溃不能保证清理完成，禁止宣称异常退出恢复保障。

## 只读与显式往返探针

```text
cargo run -p acer-controls --example usb_probe
cargo run -p acer-controls --example usb_probe -- --validate-switch-restore
cargo run -p acer-controls --example usb_probe -- --validate-threshold-restore 10
cargo run -p acer-controls --example usb_probe -- --validate-threshold-restore 20
```

默认只 GET，输出完整 JSON；写入必须显式指定 `--validate-switch-restore` 或 `--validate-threshold-restore 10|20|30`，不能同时指定。其他参数拒绝。无自动 UAC。原 pair/ACK 未确认时不写，完整记录在 JSON 中并返回非零退出码。已保存的本机 GET 样本通过精确兼容确认 enabled=true/threshold=30；该原值只用于 fixture，不会硬编码成运行时原值，监督探针仍须先真实读取。

`validate_switch_restore()`：只捕获一次真实原 state+threshold，反转 enabled 而保留阈值。试验 SET 一旦已派发，无论 ACK 或后续 GET 成功与否，都会尝试恢复捕获的原 pair。恢复 SET **不要求新的前置 GET 成功**，仅使用已保存原值，再独立 GET 确认恢复。即使试验 Applied 也执行恢复。还原 ACK 缺失、还原失败或读回不匹配时 restored=false、restore_required=true，保留原报告供主代理显式恢复，绝不自动重复写。

validation 的 restored 仅表示原 pair 恢复确认；试验是否成功应另查 trial.status。探针只有试验 Applied 且 restored=true 才以 0 退出。默认读取仅 state 确认才以 0 退出，未知状态仍输出 raw JSON 但返回 1。

`validate_threshold_restore(UsbThreshold)`：仅临时改变 threshold，保持原 enabled，之后无条件走同一原 pair 恢复路径（只要已派发试验写入）。支持 10/20/30；若目标已等于原阈值，输出“未派发验证”且不写，探针返回 1，不能据此声称完成一次往返。原先关闭的 USB 不会为阈值实验自动开启，这与官方 websocket 改阈值时强制开启的行为不同。本接口通过固定 WMI pair 显式保持原开关。

本模块不修改 QuickAccess 缓存，不调用 websocket，不启停厂商服务。直接 WMI 与 QuickAccess 后续重施设置的关系仍待主代理单独验证；本机后续关机物理供电与阈值截止也未验收。

对符合已审查 schema 的缺 ReturnValue SET，ACK 与独立读回均通过时报告 Applied。未审查或失败 ACK 仍为 Failed/Unconfirmed；只要试验已派发，恢复绝不跳过。原值/目标、两次 raw SET 和两个独立 GET 均完整输出。任何失败或退出码 1 均不能自动触发重复写入，由主代理审查报告。

## 验证记录

纯测试覆盖机型/BIOS、六个开关阈值组合、错误枚举、缺失/错误类型 ReturnValue、未知 GET 不写、只读拒绝、ACK 与完整 pair 读回、试验读回失败仍恢复、exec 错误仍恢复、恢复未确认保留状态，以及准备失败不误记派发。没有在测试里创建 WMI 连接。

系统集成对话不运行 usb_probe，不执行硬件读取或写入；主代理负责真实样本、接口收敛和正式 broker/UI 接入。

2026-11-01 编译验证：根 workspace 尚未注册时，在本 crate 的忽略目录 `.validation` 放置独立 Cargo harness，仅以 path dependency 引用真实源码（未复制实现），离线执行 `cargo test --manifest-path crates/acer-controls/.validation/Cargo.toml -p acer-controls --offline --target-dir target`，7 项单元测试通过；使用现有依赖缓存，未修改根 Cargo.toml。主代理注册后直接使用标准 `cargo test -p acer-controls` 即可，不需要保留 harness 作为正式项目结构。

随后 `cargo build --manifest-path crates/acer-controls/.validation/Cargo.toml -p acer-controls --example usb_probe --offline --target-dir target` 成功。此为首次严格版本构建的历史记录；之后主代理已注册 workspace 并取得上述真实只读样本。

历史阶段：引入 reviewed GET 兼容后，标准 `cargo test -p acer-controls` 10 项通过，probe 构建成功。fixture `tests/fixtures/phn16-71-usb-get.json` 保留原采集时的旧 ack/state 元数据，测试只读取其 raw 并断言新版解码，未篡改 evidence 原件。

主代理随后运行 SHA256=67E39A439A35DD8A63BCE35513B728AF852CB5F714302C03519FCB0339A314DC 的历史版本，保存 evidence/usb-charge-switch-restore.json：原GET1969920；试验SET1974020，uiOutput0，GET1974016；恢复SET1969924，uiOutput0，GET1969920。SET均缺 ReturnValue，原版本因过严判据报 Unconfirmed，但独立 GET 证明原 enabled=true/threshold30 pair 已恢复。原始 evidence 保留不改；完整副本 tests/fixtures/phn16-71-usb-switch-restore.json（保留 UTF-8 BOM）用于新版 ACK/往返测试，不篡改历史 status 字段。

最新 `cargo test -p acer-controls` **14 项通过**，`cargo build -p acer-controls --example usb_probe` 成功，覆盖真实 SET fixture、错误 ACK/schema 拒绝、生产 Verified 仍需初读、阈值实验保留原 enabled，以及试读失败仍恢复。本次没有运行探针；10/20 阈值的真实往返及关机物理供电仍待主代理验收。

当前稳定产物：`E:\逆向宏碁控制台\target\debug\examples\usb_probe.exe`，1,349,632 bytes，SHA256=`84AE73846CCA3E61DE392E40179791271F2C8D37036EDF6C674A747A07627FDF`，构建时间 2026-11-01 19:14:59（香港时间）。记录哈希后没有修改源码或重建，后续运行由主代理统一执行。
