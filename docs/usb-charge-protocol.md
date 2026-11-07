# 本机 USB 关机充电协议证据

2026-11-01，只读研究，PHN16-71 / BIOS V1.18。没有 SET、服务启停、注册表写入或系统设置变更；没有改 crates/UI，也没有派生代理。

## 结论

已定位本机完整静态链：PredatorSense renderer 的 QuickAccess 适配器 → `wss://localhost:5141` → `AcerQAAgent.exe` 的固定 USBCharge/USBChargeSwitch 分派 → 程序内链接的 `i3d::AcerWMI` → COM/WMI `root\WMI:APGeAction.GetFunction/SetFunction`。本机不是靠 `AcerGamingFunction` 实现该功能，也不是通过 PredatorSense 的 ACER/TCP 46933 包直接发送 USB 设置。

底层功能号为 4。阈值与开关是同一个 packed 值的不同字节：GET 输出低字节为固件状态，byte1 为开关枚举，byte2 为阈值百分比。SET 输入为 `4 | (state << 8) | (limit_percent << 16)`；state=0x0F 启用、0x1F 禁用，官方界面阈值为 10/20/30。

这足以准备独立受限 APGeAction 适配器，但**尚未取得当前 GET 原值，也未完成 USB 写回或关机供电验收**。本机普通权限实例读取失败，不能把这一失败写成硬件不支持。主对话应先通过已授权 helper 完成 GET，再安排单变量 SET/恢复实验。

## 来源与二进制身份

- 官方前端：`C:\Program Files\WindowsApps\ULICTekInc.PredatorSenseforNotebook_5.0.1341.0_x64__nt9dgb7efx6bt\app\resources\app.asar`。
- renderer：`dist/widgets/main/main.be349a7a7c9ce11c.js`，下文偏移为解出的 UTF-8 文本字符位置；不是 PE RVA。
- 官方服务：`C:\Windows\System32\AcerQAAgent.exe`，x64 原生 PE，CLR 目录为空，ImageBase=0x140000000，未导出可直接调用的 USB API。
- SHA256：`E7BC1695ABF4700712B40D23F7642FBEF4AC9C8EA3BF13AEF65D9062AC0CAC4B`。
- 本次 Authenticode 状态 Valid，签名者为 Microsoft Windows Hardware Compatibility Publisher。未取到可用 FileVersion/ProductVersion，不编造版本号。
- 下文地址统一为该 EXE 的 RVA；实际 VA 为 ImageBase+RVA。用项目 `tools/reverse/venv` 的 pefile/Capstone 和 `tools/reverse/analyze_pe.py` 读取目标，没有执行官方程序的新实例。
- 已阅读项目 dotnet-decompile/ghidra-headless 技能。目标为 native，不能用 ILSpy 还原 C#；现有项目工具说明 Ghidra 运行时未安装，本次使用已有反汇编工具，不宣称已运行 Ghidra 或完成 C 反编译。

## 前端与 websocket

renderer 证据点：

|位置|事实|
|---|---|
|约 837202|QuickAccess 类 ServiceName=QuickAccess，WEBSOCKET_PORT=5141，batteryvalueArray 为 10%/20%/30%|
|824766|基类 setWebsocket 创建 `wss://localhost:` + WEBSOCKET_PORT|
|823189|PacketType：1 AuthPacket、2 RequestPacket、3 ResponsePacket、9 ErrorPacket|
|827401|sendRequestPacket：无 Param1 时 Action=Get；提供 Param1 时 Action=Set；Version 默认 1，Session 每次新 UUID|
|845970|setUSBChargeEnable → USBChargeSwitch；setUSBChargeValue → USBCharge|
|1218097|setQAValue 对 USBCharge 去掉 `%` 后传字符串；对 USBChargeSwitch 使用当前值的取反|
|839779|USBCharge Get 的 Result.Value 存阈值；USBChargeSwitch Get 的 Result.Value 存开关|
|838437|收到 Action=Set 的 Response 后立即重发同命令 Get|
|830393|ErrorPacketHandler 按 ErrorCode 分流，不能把收到任意 JSON 当作成功|

需要先完成官方 AuthPacket 握手，再发送业务请求。基类用内置认证材料对固定 challenge 做 AES 处理，并校验服务响应；认证材料是访问凭据，本文件不复制其值。定位入口为 onHandshakingMessage（约 823800）、sendAuthPacket（约 827000）及 QuickAccess 初始化（约 837202–838200）。本次没有伪造认证、跳过 TLS 验证或发送 websocket 请求。若主对话选择复用服务，应单独审查本机证书/握手；不要通过关闭证书校验获得“成功”。直接使用已授权 WMI 适配器则无需把这些认证材料引入新应用。

固定业务请求（示例只定义格式，本次未发送）：

```json
{"PacketType":2,"Version":1,"Session":"<new-uuid>","Command":"USBCharge","Action":"Get"}
{"PacketType":2,"Version":1,"Session":"<new-uuid>","Command":"USBChargeSwitch","Action":"Get"}
{"PacketType":2,"Version":1,"Session":"<new-uuid>","Command":"USBCharge","Action":"Set","Param1":"20"}
{"PacketType":2,"Version":1,"Session":"<new-uuid>","Command":"USBChargeSwitch","Action":"Set","Param1":true}
```

官方 UI 实际传阈值字符串 `"10"/"20"/"30"`；EXE 内嵌接口描述列举相同整数值，服务阈值 handler 会转成整数并检查 10/20/30。采用官方 UI 的字符串格式最有直接调用证据；开关必须为 Boolean。GET Response 的 Result.Value 对应阈值数字或开关 Boolean，应同时核对 PacketType、Session、Command、Action；SET 收到 Response 后仍须 GET 读回。

服务内嵌接口描述：USBCharge 从 RVA 0x53B541 开始，USBChargeSwitch 从 0x53BA74 开始；两者 Actions 均列 Get/Set/Notify。该描述不是本机状态读数。

## 服务分派与缓存副作用

|RVA|定位与行为|
|---|---|
|0x39A120–0x39B32A|服务请求分派主函数|
|0x39A4BC–0x39A524|USBCharge 分支：Set 调 0x38C8F0，Get 调 0x38C160|
|0x39A529–0x39A591|USBChargeSwitch 分支：Set 调 0x38BCB0，Get 调 0x38B9E0|
|0x38CC4B–0x38CC58|阈值 handler 仅接受 0x0A/0x14/0x1E，即 10/20/30|
|0x38CD4D–0x38CD6C|设置阈值时明确传 state=0x0F，**会同时开启 USB 充电**|
|0x38BD8B–0x38BD9A|开关 setter 先取得当前完整 USB 状态，错误时不继续写|
|0x38BE44–0x38BE67|Boolean false→0x1F，true→0x0F；保留刚读出的 limit，调用同一 SetUSBChargingStatus|
|0x38BABA–0x38BAC2|开关 getter 判断 state 恰好为 0x0F 才返回 true|
|0x310E00–0x311061|配置写入辅助，引用 USBChargeState（0x310EA6）与 USBChargeLimit（0x310F8E）|

官方设置 handler 在成功路径还调用上述配置辅助；EXE 内存在 “Re-apply USB Charge settings” 和失败日志字符串（0x5034F8、0x503A88、0x503CC8）。因此直接 WMI 写入不会自动同步 QuickAccess 的持久化缓存，后续服务生命周期可能重施自己的值。重施触发条件和实际配置文件路径尚未完整定位，不能承诺停掉前端后绝不覆盖，也不能直接编辑猜测的 settings.json。

## WMI 精确签名

本次通过 Get-CimClass 读取了实际类元数据：

```text
namespace root\WMI
class APGeAction

Boolean GetFunction(
    [in,  ID(0)] UInt32 uiInput,
    [out, ID(1)] UInt64 uiOutput
)
Boolean SetFunction(
    [in,  ID(0)] UInt64 uiInput,
    [out, ID(1)] UInt32 uiOutput
)
```

本机类限定符：dynamic=true，provider=WmiProv，guid=`{61EF69EA-865C-4BC3-A502-A0DEBA0CB531}`。这是实际元数据；本次未继续映射 provider 到具体驱动文件，不把未知驱动依赖写成已验证可移除。

官方服务内部函数为类实现，非第三方 DLL 导出：

- `AcerWMI::GetUSBChargingStatus`：0x323B90–0x323DFE。
- `AcerWMI::SetUSBChargingStatus`：0x323920–0x323B83。
- `CallBIOSGetFunction`：0x3261B0–0x326949。
- `CallBIOSSetFunction`：0x3259D0–0x32616C。

虚表交叉证据：RVA 0x540490 存 GetUSBChargingStatus 的指针（ImageBase+0x323B90），紧邻 0x540498 存 SetUSBChargingStatus 指针（ImageBase+0x323920），与 handler 使用的 +0x40/+0x48 getter/setter 槽位一致。

两个 CallBIOS 包装均显式使用 root\\WMI、APGeAction、uiInput、uiOutput；GET 采用 GetFunction，SET 采用 SetFunction。普通导入表中没有负责 USB 充电的额外 Acer DLL，COM 依赖 ole32/OLEAUT32，WMI provider 则由系统实现。仅凭导入表不能排除其他功能的动态 DLL；本项可定位的执行链已在 EXE 中找到。

### GET

```text
GetFunction(uiInput = UInt32(4))
raw_output = UInt64(uiOutput)
firmware_status = raw_output & 0xFF
state = (raw_output >> 8) & 0xFF
limit_percent = (raw_output >> 16) & 0xFF

state == 0x0F → enabled
state == 0x1F → disabled
其他值 → 未知/保留原始值，适配器不能简单转成 disabled
```

直接证据：0x323D9A 将 edx 置 4；0x323DB3–0x323DD4 分别右移 8/16 并取低字节，写入 C++ tuple。服务 getter 在固件状态非零时返回通用错误。

### SET

```text
SetFunction(uiInput = UInt64(4 | (state << 8) | (limit_percent << 16)))
firmware_status = UInt32(uiOutput) & 0xFF
```

直接证据：0x323B30–0x323B45 为 limit<<8、OR state、再<<8、OR 4，调用 CallBIOSSetFunction；0x323B55–0x323B5E 取返回低字节。

|目标|uiInput 十六进制|十进制|
|---|---:|---:|
|启用，低于 10% 停止|0x000A0F04|659204|
|启用，低于 20% 停止|0x00140F04|1314564|
|启用，低于 30% 停止|0x001E0F04|1969924|
|禁用，保留阈值 10%|0x000A1F04|663300|
|禁用，保留阈值 20%|0x00141F04|1318660|
|禁用，保留阈值 30%|0x001E1F04|1974020|

这里的“低于阈值停止”是官方描述，10/20/30 是 USB 关机供电截止阈值，不是电池充电上限，也不是开启后必然输出电流的实测保证。

## 返回码与成功判据

分开记录三层，不混用：

1. COM/WMI 传输失败：官方 CallBIOS 返回 false 时，USB wrapper 返回 AcerWMIError=0x0100；GET tuple 同时放 state/limit=0xFF。不能把 0x0100 当作固件成功。
2. 固件 uiOutput 的低字节：0 为官方 handler 接受的成功状态；非零返回通用错误。其他低字节码的具体含义尚未定位，不自行枚举“无权限/不支持”等解释。
3. websocket：PacketType=9 的 ErrorCode，1000=General error、1001=Incorrect AuthPacket、1002=Incorrect Parameters；1003/1004 是基类定义的无法停止/其他操作未完成错误，不是已证明 USB 会返回的特有码。USB 参数分支可定位到 1002，WMI 非零/失败分支到 1000。

方法元数据声明 Boolean ReturnValue，但官方 CallBIOS 包装本次可见路径获取的是 uiOutput，未见读取 ReturnValue 属性。新适配器应保留 ReturnValue、HRESULT/权限错误和完整 uiOutput；ReturnValue=false 必须失败，ReturnValue 缺失的处理要由主代理实际 GET/SET 样本确认，不照搬其他 WMI 类的特例。即使低字节为 0，也必须再次 GET 验证 state 和 limit，才报告 Applied。

## 本机只读结果与依赖

- AcerQAAgentSvis 当前 Running，服务路径 `C:\Windows\system32\AcerQAAgent.exe`；SCM RequiredServices/DependentServices 均为空，这不证明可停用其他设备驱动。
- Get-NetTCPConnection 发现本机监听 `[::]:5141`，OwningProcess=5780，与 AcerQAAgent.exe PID 一致。监听地址不是仅 IPv4 loopback，不能在替代实现中照搬为开放外部接口；实际外部可达性未测试。
- 官方功能覆盖审计已从日志确认 USBCharge 和 USBChargeSwitch 在本机支持列表内；本次二进制接口描述与之吻合。
- Get-CimClass 签名读取成功。普通权限 Get-CimInstance APGeAction 报“无法从客户端中访问 CIM 资源”，错误 ID 明确为 HRESULT 0x80041003；分阶段查询确认失败就在实例读取，因此 GetFunction(4) 没有取得实例、没有实际发出方法调用。本次无当前 state/limit 样本。
- 复用 websocket 依赖运行中的 QuickAccess 服务及其 TLS/认证，通常不必保留 PredatorSense 可视窗口；直接 APGeAction 路径依赖 WMI/固件 provider 和对应权限，不要求把 QuickAccess websocket当作 transport。是否可停止 QuickAccess 后继续工作需后续单独验证，本次不做停服务实验。

## 恢复与主代理验收步骤

1. 先经已授权 native 通路只调用 GetFunction(4)，保存完整原始 uiOutput、ReturnValue、state、limit 和解码。固件状态非零、枚举未知或原阈值不在已证明范围时，暂不开始写实验。
2. 保存的是 `(enabled/state, limit_percent)` 一对，不能仅存 Boolean。严禁用参考脚本“禁用”常量一概恢复：663300 硬编码了 10% 阈值，会覆盖原来的 20/30%。
3. 优先选择一个单变量试验，报告参数、响应与独立 GET 读回。通过 websocket 改 threshold 会自动 enable；若原状态禁用，恢复时先恢复原 threshold，再最后将 USBChargeSwitch 恢复 false，并再 GET 两者。使用直接 WMI 则可用一个原 state+limit packed 命令恢复，仍须读回确认。
4. 若借官方服务完成实验，恢复也经该服务走相同路径，以同步持久化状态；若用直接 WMI，须额外比对服务读数及之后重施行为，不能假设缓存同步。不要自动修改 QuickAccess 的持久化文件。
5. SET 超时/失联时先重新 GET，禁止盲目重复写入；恢复失败保留原记录供显式重试，不宣布已恢复。关机后物理端口供电、阈值截止、电源适配器存在与否等行为另立验收，不从 WMI ACK 推导。

## 与现有参考脚本的关系

`references/acer-scripts/.../scripts/setusbcharging.ps1` 使用 APGeAction、GetFunction(4) 和上述四个常量；本次官方 native 反汇编独立支持 packed 布局。脚本以 bit12 判断禁用，并用 `((raw >> 17) & 0x6F)*2` 解阈值；新实现应采用官方完整 byte1/byte2 解码，而不复制这一不直观掩码或脚本的自提权行为。

仅本文件新增研究结论；主代理后续负责选择适配器、native 受限命令、样本采集与硬件实验。没有找到协议不等于不支持，本次已经找到本机协议，剩余为权限下的样本与写回/恢复验证。
