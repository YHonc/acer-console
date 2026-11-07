# 本机 BatteryCalibration 协议与生命周期

日期：2026-11-01。目标：Predator PHN16-71 / BIOS V1.18；本机 AcerCCAgent 1.5.21、PredatorSense 5.0.1341.0。范围为只读静态研究，未开始、停止或中止校准，未修改充电模式、电源计划、服务或 Rust/UI，也未要求用户拔插电源。

## 结论

校准不是普通布尔开关。官方链路为：真实 UI → CareCenter WebSocket → `ACCAgentSvis::OnSetBatteryCalibrationPacket` → `StartBatteryCalibration(bool)` → `ROOT\WMI:BatteryControl.SetBatteryHealthControl` 的 Calibration 掩码 `0x02`；校准结果经 `APGeEvent` 返回，Agent 还负责 AC/DC 电源设置的保存、调整和恢复。

`BatteryCalibration.Get.Result.Value` 和现有 `uFunctionStatus[1]` 只是模式标志。成功需要独立终止通知；官方 UI 用 `Data.BatteryStatus=1 && Data.Progress=100` 判成功，`BatteryStatus=0 && Progress=100` 判失败/中断。原生通知构造器把 `Progress` 固定写为 100，不能据此生成连续进度条。电量百分比、阶段、剩余秒数是另一路 `BatteryStatus` 数据。

开始、Stop、Abort 的命令和清理分支已定位，可据此设计实现；本机真实启动、长时间阶段推进、终止事件、取消和电源设置恢复均未执行验收。不能以 mask2 的布尔写入/读回替代完整任务成功或取消确认。

## 证据基线与定位方式

| 来源 | 本次核对的位置 |
|---|---|
| 官方原生 Agent | `C:\Windows\System32\AcerCCAgent.exe`，FileVersion `1.5.21`，SHA256 `33dc36902e885d249f6bf69fc25bb98ade3678e701d1af46990c1c6b723ecbed` |
| 官方 ASAR | `C:\Program Files\WindowsApps\ULICTekInc.PredatorSenseforNotebook_5.0.1341.0_x64__nt9dgb7efx6bt\app\resources\app.asar`，SHA256 `4955f25d50094d177b3bcf485e9214177a99e7abe2142fc79caee7a1b5f53f59` |
| 真实 renderer | ASAR 内 `dist/widgets/main/main.be349a7a7c9ce11c.js`，下文 offset 为解码后 JS 字符偏移 |
| 官方说明文字 | ASAR 内 `dist/widgets/main/shared_assets/i18n/en.json` 的 `MUI_Battery_Calibration_DESC1..8`；外部 `resources/src/assets/i18n/en.json` 是空占位文件，不是该组说明 |
| WMI 方法元数据 | `tools/probe/signatures.local.json:1028` 起的 BatteryControl 方法；本次另只读查询 `Get-CimClass ROOT/WMI APGeEvent` 的属性元数据 |
| 本机已存在的支持证据 | `docs/official-feature-coverage.md:128` 的 CareCenter 功能列表，以及 `evidence/rust-probe-elevated.json` 的 BatteryControl 原始读数：function list=3、status 全0、return=[0,0]；这是历史空闲样本，不是校准运行样本 |

Agent 为原生 x64 PE，CLR directory 为空；未使用 ILSpy 反编译。当前没有可调用的 Ghidra 命令，本次用现有只读 `tools/reverse/analyze_pe.py` 的 PE/Capstone 解析核对 `.pdata` 函数边界、指令、字符串和跳转表，未加载执行厂商 EXE/DLL。以下原生地址均为 RVA，不依赖 ASLR 后的运行地址；该 EXE 的首选 ImageBase 为 `0x140000000`。

## 官方 UI → CareCenter 协议

真实 CareCenter 类 `mr` 位于 renderer 约 offset `834400`，配置 WebSocket 端口 `4343`；这不是 PredatorSense Agent 的 `46933` TCP 协议。连接和认证由基类 `zS` 完成，必须遵循官方认证/会话流程。本研究未连接该 socket，也未发送任何 Get/Set/Abort 消息；认证材料不复制进项目文档。

renderer 证据：

- offset `823221`：包类型 Auth=1、Request=2、Response=3、Error=9。
- offset `827401`：`sendRequestPacket(command,value,version)`；value 为空时 Action=Get，否则 Action=Set、Param1=value，Version 默认1，Session 为新 UUID。
- offset `827659`：`sendActionRequestPacket(command,action,version)` 发送显式 Action，无 Param1。
- offset `848828` 附近：真实 adapter 的 get 返回已缓存的 `mr.instance.BatteryCalibration`；set 调用 `mr.instance.sendRequestPacket(BatteryCalibration,value)`。
- offset `1223178` / `1223243`：`startBatteryCailbration()` 传 true；`stopBatteryCailbration()` 传 false。函数名拼写是厂商原样。
- offset `1394344` 附近：运行中 Cancel 按钮调用 stop(false)，随后 UI 立即显示失败页；这个即时显示不是固件已停止的证据。
- offset `835242` 附近：收到 Set 响应后另发 Get；Get 的 `Result.Value` 更新缓存。Notify 通过 `CCSendNotify` 分发。

Agent 内置的功能定义位于 EXE RVA `0x4F3CD2` 起，列出的动作是 Get、Set、Notify、Abort；Set 参数只有 Param1（true/false），Abort 无参数。请求形状如下，仅为静态协议示例：

```json
{"PacketType":2,"Version":1,"Session":"<UUID>","Command":"BatteryCalibration","Action":"Get"}
{"PacketType":2,"Version":1,"Session":"<UUID>","Command":"BatteryCalibration","Action":"Set","Param1":true}
{"PacketType":2,"Version":1,"Session":"<UUID>","Command":"BatteryCalibration","Action":"Set","Param1":false}
{"PacketType":2,"Version":1,"Session":"<UUID>","Command":"BatteryCalibration","Action":"Abort"}
```

Get 的业务结果为 `Result:{Value:bool,Description:string}`；原生实现使用 Enabled/Disabled 描述。它不返回任务 ID、完成时间、连续校准百分比或历史成功记录。真实通知的业务片段为 `Command:"BatteryCalibration", Action:"Notify", Data:{BatteryStatus:<event byte>,Progress:100}`。不要把 `CheckBattery` 的健康检查进度当作校准进度。

同一bundle在约offset803000..822600另有 `Up` 模拟adapter及 `fakenotify`：它每秒递增模拟值，最后拼出BatteryStatus=1/Progress=100。该段不是本机服务运行证据；上文通知字段另由真实Agent构造器和真实 `mr` 处理链确认，未拿模拟样例推定硬件成功或耗时。

## 原生调用链与开始/停止差异

| 原生函数 / RVA | 已核对行为 |
|---|---|
| `OnGetBatteryCalibrationPacket` `0x3B22A0..0x3B274C` | 调 IAcerWMI 的 GetBatteryHealthControlStatus(BatteryIndex=1)，检查返回错误和 Calibration 支持，再返回校准 enabled flag；与服务本地 running flag 不等价 |
| `OnSetBatteryCalibrationPacket` `0x3B2760..0x3B3975` | 解析 Param1；检查服务本地 `battery_calibrating_`（对象+0xF10），再调用 StartBatteryCalibration(目标bool)；成功后保存该 flag、持久化模式状态并设置开始时间/容量追踪 |
| `StartBatteryCalibration(bool)` `0x3AC890..0x3ACB09` | `0x3AC8B1..0x3AC8BA` 将 bool 放 r9b、function mask=2 放 r8b、BatteryIndex=1 放 dl，调用 IAcerWMI vtable+0x58；UInt16 错误码0返回 true，非0记录错误并返回 false |
| `AcerWMI::SetBatteryHealthControl` `0x360740..0x36109A` | WMI wrapper 接收 BatteryIndex、BatteryFunctionMask、bool，固定 BatteryControl/SetBatteryHealthControl 方法链 |
| `AcerWMI::GetBatteryHealthControlStatus` `0x3610D0..0x361E9D` | 读取支持/各功能状态，供 Get、启动恢复检查使用 |
| `OnAbortBatteryCalibrationPacket` `0x3B1800..0x3B2299` | 独立 Abort 入口；本地未运行时返回错误。运行时调用 StartBatteryCalibration(false)，即使该调用失败也继续恢复已保存 AC/DC 电源设置，清除本地追踪和持久化模式标志 |

Set 的状态分支不是幂等赋值：

- 空闲 + true：尝试开始。
- 运行 + false：尝试正常 Stop；只有硬件调用返回成功才进入正常停止清理。
- 空闲 + false：记录“Stop action but calibration is not running”，返回错误1002，见 `0x3B2A19..0x3B2C75`。
- 运行 + true：记录“Start action but calibration already running”，返回错误1004，见 `0x3B2C7A..0x3B2EFD`。
- 缺失/不接受的参数：错误1002；底层调用失败：错误1000。错误响应由 `0x357070` 构造，须保留原始 ErrorCode，不当作普通成功响应。

Abort 是恢复系统状态的另一条分支，不等价于已经确认 EC 停止。其“failed to stop ... but still restore power settings”字符串位于 RVA `0x4EFD40`；即使响应/本地标志已清除，也必须另外保留底层停止失败和硬件状态未知。当前真实 UI Cancel 走 Set(false)，没有在该按钮路径中找到改用 Abort 的调用。

## WMI 方法签名和原始参数

namespace：`ROOT\WMI`；class：`BatteryControl`。元数据 ReturnType 都是 Boolean，但既有本机 WMI Getter 实际输出曾省略 ReturnValue；不能把元数据 Boolean 与固件返回码合并处理。以下 mask=2 指 `0x02`，即 bit index 1，勿误写成 `1<<2`。

| 方法 | 输入（CIM 类型） | 输出（CIM 类型） |
|---|---|---|
| `GetBatteryHealthControlStatus` | uBatteryNo UInt8=1；uFunctionQuery UInt8=1；uReserved UInt8[2]=[0,0] | uFunctionList UInt8；uReturn UInt8[2]；uFunctionStatus UInt8[5]；方法级 ReturnValue 如 provider 实际提供则保留 |
| `SetBatteryHealthControl` | uBatteryNo UInt8=1；uFunctionMask UInt8=2；uFunctionStatus UInt8=1开始/0停止；uReservedIn UInt8[5]=[0,0,0,0,0] | uReturn UInt16；uReservedOut UInt16；方法级 ReturnValue 如存在则保留 |

输入字段/长度由 `signatures.local.json:1028..1224` 的本机元数据约束；BatteryIndex=1、mask=2、目标bool由官方 Start wrapper 的指令确认。保留数组全0与现有 BatteryControl 编码一致，但本次没有发出校准 setter，也没有校准 ACK 样本。已验收的 health mask1 UInt16 uReturn=0 不自动构成校准写入验收。

Get 的 `uFunctionList & 0x02` 是校准支持；`uFunctionStatus[1]` 是校准模式状态，`uFunctionStatus[0]` 是健康充电模式。已知空闲样本为支持列表3、5字节状态全0和uReturn=[0,0]。Get 未提供百分比、终止原因或校准任务句柄；未知返回、缺失字段和非预期状态应保留原值和未知状态，不能填 false。

`GetBatteryFunctionData` / `SetBatteryFunctionData` 另含 `uBACStartTime`、`uBACStopTime`、`uBACStatus`、`uBACSwitch`，见元数据1226..1434；官方校准 Start wrapper 不调用它们。不能因名称 BatteryFunctionData 或 BAC 就拿这些字段当校准进度/开始停止接口；其与 Adaptive Charging 的关系需单独研究。

## 硬件事件 → 结果通知

官方 `AcerWMIEvent::Start` 的 `0x368553` 使用 WQL `SELECT EventDetail FROM APGeEvent`，`0x36857F` 对应 `root\WMI`。本次只查询类元数据，未建立事件订阅：`APGeEvent : WMIEvent`，`EventDetail` 类型 UInt8Array、MAX=8，说明为“APGe WMI Event Data, Version 2.88”。

调用链：

```text
ROOT\WMI APGeEvent.EventDetail
  → AcerWMIEvent::OnEvent (RVA 0x367A60..0x3681FC)
  → EventDetail[0] == 11 / 0x0B
  → ProcessBatteryCalibrationEvent(EventDetail[1]) (0x366720..0x366A16)
  → IAcerWMIEventListener vtable+0x30
  → ACCAgentSvis::OnBatteryCalibrationWMIEvent(byte) (0x3AD0A0..0x3AD735)
  → Notify 构造器 (0x3ACB10..0x3AD06A)
  → CareCenter websocket Notify
  → renderer 校准对话框 CCNotify
```

字节映射由 `OnEvent` 的 `0x367EDA..0x367F77` 及 RVA `0x3681BC` 的16项跳转表核对：event kind 11跳转到0x367F6B，第二字节传入校准处理器。不要误用第三/第四字节拼出的其他热键参数。事件处理器把原始 byte 转发；不能给未见的数值自行命名阶段。

`OnBatteryCalibrationWMIEvent` 在 `0x3AD4A0` 判断 event byte<2；当服务本地仍运行时，0/1走终止清理并恢复原 AC/DC 电源设置、清除持久化 flag。通知构造器在 `0x3ACBD6` 把事件字节放入 `BatteryStatus`，在 `0x3ACCCE` 固定写入整数100作为 `Progress`。这不是硬件完成百分比采样。

renderer 约 offset `1396300..1396900` 的明确语义：

| 通知字段 | 官方 UI 解释 | 实现时保留的边界 |
|---|---|---|
| BatteryStatus=1 且 Progress=100 | 完成，显示成功并记录本地 last check time | 本次只有静态调用链证据；未接收真实完成事件 |
| BatteryStatus=0 且 Progress=100 | 失败/中断页 | 不区分用户取消、AC丢失、异常中断等详细原因；另保留先前动作/错误/供电证据 |
| 其他字节 | 当前成功/失败条件不匹配 | 原始状态未知，不猜测 charging/discharging 或成功 |
| Get.Value=false | 模式关闭 | 单独不能证明成功或用户取消完成 |

另有非固件终止来源：`OnPowerSettingNoitify` 在 `0x3C12EF..0x3C17E3` 发现正在校准但 `SYSTEM_BATTERY_STATE.AcOnLine` 为0时，调用 `OnBatteryCalibrationWMIEvent(0)`。因此 BatteryStatus=0 并非只能解释成特定 EC 故障码。

## 电量阶段、时间与前置条件

真实 UI 校准对话框约 offset `1391700..1397800`：

1. 开始按钮绑定 `disabled=!wsinfo.BatteryACMode`；起始电量<100从 Charging to 100% 开始，电量=100直接进入 Discharging to 3%。未找到该真实按钮额外要求“至少40%”的条件；40%是性能模式切换规则，不应套入校准。
2. UI 根据 `BatteryStatus.BatteryPercent` 依次在100、3、100时推进阶段：先充满→放至3%→再充满→Ready to finished。这些是 UI 基于电量推导的阶段，不是 WMI getter 的进度枚举，也不是固定线性百分比。
3. `BatteryFlag>=8` 或末阶段的特例判断为 charging，显示 `FullLifeTimeSeconds`；否则显示 `RemainingSeconds`。未知/负值不显示假倒计时。两者是当前阶段估计，不是总校准剩余时间。
4. 原生 `OnPowerSettingNoitify` 通过 `GetSystemPowerStatus` 和 `CallNtPowerInformation(SystemBatteryState=5)` 取得电源状态；无HID分支的 `CreateBatteryStatusJSON` 为 `0x3DD030..0x3DDC11`，字段 RemainingSeconds / FullLifeTimeSeconds 分别在0x3DD946 / 0x3DDA4E附近构造。
5. 校准运行期间，Agent 还有软件估时：`0x3C1526..0x3C17D0` 初次容量未知时使用43200秒，按经过时间及1.1系数调整，过大时回退34560秒，并写回输出时间字段。它是估计逻辑，不能宣传为“校准固定12小时”“12小时自动超时”或完成判据。本次未确认独立的总任务超时限制。

官方说明文字（en.json 的 DESC2、DESC4、DESC5、DESC6）要求：校准需数小时，全程连接适配器，不拔掉适配器，并关闭其他窗口、避免运行其他应用。DESC1建议每3个月一次；这不是本项目自动调度要求。DESC8文本提到 Optimized/Adaptive Battery Charging 会临时关闭，但当前校准对话框模板只渲染DESC1..7；本次没有证明这些模式在本机何时关闭/怎样恢复，不能据此新增健康充电或BAC写入。

原生 Set handler 的已读路径没有独立的 AC/最低电量预检查，UI AC gating与运行中供电中断处理分别存在；未来原生实现不能假定服务已提供所有开始条件检查。全程适配器保持连接，放电由校准模式控制，不需要用“用户拔电源”驱动阶段。本次未做任何物理配合或耗时实验。

## 电源设置副作用与恢复

开始成功后 `OnSetBatteryCalibrationPacket` 的 `0x3B346A..0x3B36DF`：

- `PowerGetActiveScheme` 读取当前计划；用 `PowerReadACValueIndex` 与 `PowerReadDCValueIndex` 保存AC/DC各6项。对象持久化键包含 `ACPowerSettings` / `DCPowerSettings`，字段为 CriticalAction、CriticalLevel、ReserveLevel、HibernateAfter、SleepAfter、TurnOfDisplayAfter。
- 临时数组按该顺序为 `[0,1,1,0,0,0]`；AC写函数 `0x33CA90..0x33CB8D` 与DC写函数 `0x33C980..0x33CA7D` 分别循环 `PowerWriteAC/DCValueIndex`，再 `PowerSetActiveScheme` 应用。
- `SetThreadExecutionState(0x80000003)` 保持系统/显示活动；正常Stop在 `0x3B37CB..0x3B37FC` 恢复保存的AC/DC值，再调用 `SetThreadExecutionState(0x80000000)` 清除该持续请求。
- Abort 同样在 `0x3B1D10..0x3B1D47` 恢复两组值并清除execution request，但它在底层stop失败时仍继续系统状态恢复；不能吞掉该失败。
- 0/1终止事件在 `0x3AD4C9..0x3AD510` 恢复保存的AC/DC值、清除本地与持久化校准标志。此事件分支未直接看到同一个execution-state API调用，不声称每条清理路径完全相同。

恢复/生命周期证据：`SetSettings4BatteryCalibration` (`0x3BC0F0..0x3BC587`) 会读取当前固件标志，必要时恢复保存电源设置，若持久化记录仍标为校准但实际已关闭则发失败通知。`0x3C21A0..0x3C23CE` 的恢复处理会重核并清除本地running标志，相关字符串包含 resumed from S3 or S4；renderer恢复处理约offset1211047发送 `BatteryCaliBration_Close_Dialog` 并走失败页面。不能把睡眠/进程重启后的bool状态当成成功历史。

## 足以指导实现的契约与剩余缺口

实现应分开保留：支持能力、硬件模式flag、服务/本项目任务状态、推导阶段、电池百分比、阶段ETA、终止事件原值、命令/固件错误，以及电源设置恢复结果。建议显式 Start / Stop / Abort 与只读状态接口；不要复用健康充电开关、接受前端任意 WMI 方法，或以“写回原bool”作为校准回滚。

若走官方 CareCenter IPC，应由官方 Agent 统一拥有该生命周期，并处理其认证、Session、ErrorPacket、Notify和Get的差别；若未来独立实现 WMI 路径，则必须自行拥有事件订阅、开始前电源设置checkpoint及退出/取消恢复。仅复制 SetBatteryHealthControl(mask2,bool) 会漏掉官方的电源计划管理和终止确认。不同控制器不能同时写入同一校准流程。

当前缺口明确为：本机校准setter ACK与原始事件样本；真实阶段耗时/ETA可信度；正常Stop与Abort的固件停止确认；休眠/服务退出恢复的真实结果；其他充电模式是否变化；未知事件字节和固件错误码的完整含义。研究已定位实现所需方法、输入输出和生命周期，不以临时启动校准来补这些证据。本次交付只增加本文件，未修改 crates/UI 或启动任何硬件试验。
