# 后端 API 契约

## 后续接口索引（2026-11-03 整理）

原有设备接口说明保留。桌面命令注册已增加 `get_thermal_config`、`save_thermal_config`、`get_thermal_status`、`start_thermal`、`stop_thermal`，实现位于 apps/desktop/src-tauri/src/thermal_commands.rs；保存不自动运行，详见[温控使用](温控候选版使用与测试.md)。文件夹模块增加 `bind_device_center_module`，由原生选择器选择目录，详见[模块接口](DeviceCenter模块接口约定-2026-11-03.md)。完整注册以 main.rs 为准；这些是已存在的接口，本轮未修改代码。

## 原生设备接口

`acer_device::Device::connect() -> Result<Device, DeviceError>` 创建固定工作线程；COM连接仅在该线程使用。Device可clone、可跨线程，所有请求串行。

原生策略：ReadOnly（默认）；Verified（精确PHN16-71/V1.18的已核验白名单）；AllowUnverified（监督实验）。策略不支持serde，不能由WebView JSON覆盖。Verified允许Auto/Max、官方0..100自定义风扇目标、AC下Quiet/Balanced/Performance/Turbo、四区静态颜色/亮度、动态灯效1..7、会话内上一灯光配置恢复及health-mode保护开关。Eco仍拒绝。动态普通接口已经实现；正常路径实机验收与下述受控实验分别记录。

- `device.snapshot() -> Result<acer_protocol::Snapshot, DeviceError>`：只读，字段含原始结果/错误。值未知为None。
- `device.capabilities() -> Result<acer_protocol::Capabilities, DeviceError>`：机型/BIOS、方法与实际读取能力。
- `device.apply(command: acer_protocol::Command) -> Result<acer_protocol::ApplyReport, DeviceError>`：固定枚举写接口，记录before/after、原始返回；未通过本机协议验收的功能返回拒绝，禁止前端任意方法调用。
- `device.restore_previous_lighting() -> Result<ApplyReport, DeviceError>`：等价于`device.apply(Command::RestoreLighting{})`，只使用当前worker持有的上一灯光配置，不接受调用方帧或颜色。
- `device.restore_validation_fan() -> Result<ApplyReport, DeviceError>`：仅native受控验收。后端在AllowUnverified、精确机型/BIOS、成功前读下的Max调用前保存原Auto/Max；此方法只恢复该已保存值，不接收用户提供的模式。读取失败也不阻止恢复尝试。ReadOnly策略或没有保存值时拒绝，不提供UI命令。
- `restore_validation_profile()`恢复worker保存的非Eco原模式，独立重验AC/电量>=40%和支持掩码；当前profile不可读不会单独阻断，但供电条件未知/不满足时报告待恢复。
- `begin_fan_custom_70_validation()`仅AllowUnverified固定70/70试验，原Auto/Max前读通过后保存恢复值；`restore_validation_fan()`不因当前Custom阻断恢复。
- `begin_lighting_brightness_validation()`与`restore_validation_lighting()`仅AllowUnverified、native受控试验；UI无对应命令。原getter15字节+gmReturn0+static0+四区可读才保存state并重构setter16，只改brightness20后恢复。原状态和重构帧由BacklightFrame保留；不将gmReturn拼入载荷。
- `save_validation_lighting_checkpoint()`仅AllowUnverified、精确目标机、原static0帧/四区可恢复且无待恢复记录时使用。只读保存独立原始配置，返回空writes；通过`restore_validation_dynamic_lighting()`恢复。它不对应WebView Command、不接收调用方帧，不会被另一个Verified Device的“上一灯光配置”记录覆盖。

所有公开数据derive serde Serialize/Deserialize。Command：SetProfile { profile: Profile }；SetFanMode { mode: FanMode }；SetFanSpeeds { cpu_percent:u8, gpu_percent:u8 }；SetLighting { lighting: Lighting }；SetLightingAnimation { lighting: LightingAnimation }；RestoreLighting {}；SetBatteryCare { enabled: bool }。Profile=Quiet/Balanced/Performance/Turbo/Eco；FanMode=Auto/Max/Custom。Custom为当前状态，设置时必须使用SetFanSpeeds（SetFanMode Custom拒绝）。Lighting={brightness:u8, zones:[[u8;3];4]}。Command以`type`内部标签、snake_case序列化，并拒绝未知字段；写入默认禁用。

SetFanSpeeds { cpu_percent:u8, gpu_percent:u8 }范围0..=100，来源为本机官方SUPPORT_FAN_CONTROL IPC CPU/GPU min0/max100，不是临时70下限。仅70点物理实测，0和100等其他点不宣称物理验收；每次请求都核对行为和两个目标百分比。编码始终三步Custom行为+CPU百分比+GPU百分比，0/100不偷换成Auto/Max。CPU bits8..9/GPU bits14..15及组合mask9一致时识别Auto/Max/Custom，混合/未知保持None。普通Auto/Max可从Custom切回；受控实验仍拒绝原Custom以保护原设置。

温度选择器和能力掩码已实现；灯光配置可在header、字段范围和gmReturn有效时解析。本机DLL布局与brightness20/四色实测已交叉确认。主代理随后完成动态1..7逐项受控写入和恢复，真实fixture回归与模拟切换测试分别保留。

Snapshot新增`cpu_fan_percent:Option<u8>, gpu_fan_percent:Option<u8>, lighting_effect:Option<LightingEffect>, lighting_config:Option<LightingConfig>`。Auto/Max中的百分比是缓存Custom目标，并非占空比/RPM百分比。LightingEffect按0..7映射为static/breathing/neon/wave/shifting/zoom/meteor/twinkling；未知或读失败为None。`lighting`仅表示完整静态配置。`LightingConfig={effect,speed:u8,brightness:u8,direction:u8,color:[u8;3],zones:Option<[[u8;3];4]>}`；静态缺失任一区时zones=None，动态时zones始终None，不把动态区读取当作历史静态预设，也不补黑色默认值。Option在JSON中为null。

Capabilities字段：`model:String, bios:String, matched_model:bool, items:Vec<Capability>`。Capability字段：`id:String, readable:bool, writable:bool, status:CapabilityStatus, reason:String`。status为snake_case：verified / implemented_unverified / protocol_pending / permission_blocked / hardware_unsupported。当前没有项目被判定为hardware_unsupported。

ApplyReport字段：`status:ApplyStatus(Applied/Failed/Unconfirmed/Rejected), message:String, before:Option<Snapshot>, after:Option<Snapshot>, writes:Vec<RawRead>`。Verified Auto/Max在精确setter参数、gmOutput=0（可省ReturnValue，但显式false/非Boolean拒绝）、after中CPU/GPU/组合三份模式读回与目标一致时才Applied；明确错误Failed，ACK缺失或读回不全/不符Unconfirmed，策略/前读不满足Rejected。RPM可能延迟变化，记录但不作为即时模式确认阈值。其他写入保持原有门槛。

Verified profile独立检查：AC在线、电量>=40%、原已知非Eco、原/目标都在支持掩码，目标仅Quiet/Balanced/Performance/Turbo；不复用fan_ready。固定SetGamingMiscSetting(0x0B|code<<8) ACK精确0与原始GetGamingMiscSetting(0x0B)目标读回同时满足才Applied。

Verified Custom要求0..100目标、当前三模式读回一致、双RPM与缓存百分比可读；逐步ACK未知/失败即停止。三个写ACK精确0后，after必须Custom且三份行为=3、CPU/GPU目标百分比与本次请求逐项相同，才Applied。id为fan_custom，能力范围与guard均0..100；不把70点实测推广为全范围物理验收。

capabilities中的fan_mode在目标机有读值时标记Verified；writable仅在当前连接策略为Verified且CPU/GPU/组合模式读回一致、双RPM可读时为true。ReadOnly连接即使能读也不会宣称可写，helper需要通过自己的Verified连接返回相应能力。

正常Auto/Max控制的已知前读也可为Custom；“原Auto/Max”限制只用于受控风扇实验。fan_custom、profile、lighting、battery_care分别使用自己的前置条件，绝不共用fan_ready阻断其他独立设置。

Verified静态SetLighting要求当前已知0..7配置可保存且可恢复；原静态必须四区可靠，原动态可仅保存完整动态帧。先发static0目标亮度帧，再依次写四区RGB，逐步ACK精确0，未知/失败即停止本次设置。after静态亮度、四区及非颜色状态与目标一致才Applied。仅颜色缓存字节按协议归一化，不忽略四区配置读回。支持静态→动态、动态→动态、动态→静态。

Verified SetBatteryCare要求health function bit1支持、原bool可靠、校准/其他函数状态idle。SetBatteryHealthControl仅mask1、保留数组5个0，ACK是UInt16 uReturn精确0；after目标bool、支持列表及其他函数状态不变才Applied。该开关已实测反转和恢复，不宣称物理80%充电截止曲线已测。

动态普通命令`SetLightingAnimation`接收`LightingAnimation={effect,speed:u8,brightness:u8,direction:u8,color:[u8;3]}`。effect只接受breathing/neon/wave/shifting/zoom/meteor/twinkling，静态使用SetLighting；speed为1..9，官方IPC提供该范围。应用提供0..100亮度输入，官方UI的1..5档到协议值映射尚未确认，不称为官方数值范围。Wave/Shifting使用direction=1右/2左；其他效果接受0..2但方向无语义，0发出时归一为1。Neon/Wave忽略颜色，其他动态效果使用单色。Applied要求固定帧ACK精确0及effect/speed/brightness读回一致，并核对该效果适用的颜色/方向。

```json
{"type":"set_lighting_animation","lighting":{"effect":"breathing","speed":3,"brightness":20,"direction":1,"color":[255,128,0]}}
```

`lighting`和`lighting_dynamic`能力分别对应静态和动态；目标机、Verified策略、可保存且可恢复的当前配置同时满足才writable。`lighting_restore`仅当前worker存在恢复记录且为Verified时writable。每次通过前置检查的普通灯光请求在首个写入前保存实际原配置；后续有效灯光请求覆盖这份记录，前置拒绝不会覆盖。记录只在当前helper会话中存在，进程退出后不保留。

```json
{"type":"restore_lighting"}
```

恢复命令不接受raw_frame或其他未知字段；Rust构造必须为`Command::RestoreLighting{}`。恢复不依赖当前灯光前读成功；先static0亮度0停止动画，原四区已知时全部恢复四区再发原帧，原动态且四区未知时仅恢复保存的原动态帧，不伪造静态预设。任一步失败也继续剩余恢复；全部ACK及原配置有意义字段读回一致才Applied并消耗记录，否则保留供重试。恢复动态配置不承诺动画相位一致。

动态受控入口仍为`acer-probe --validate-lighting-effect 1 --restore`，仅1..7，一次一项；0不重复静态验证。参数固定speed3/brightness20/right1、需颜色时橙色255/128/0。原static0完整帧/四区保存后才试，恢复先static0亮度0停止动画，再原四区和原帧；原getter15、重构setter16、映射、全部请求/返回/读回均在报告中。七个效果各自的实机记录仅覆盖这些参数点；不等同于全部速度、方向、亮度或普通UI路径验收。

正式路径验收入口`acer-probe --validate-lighting-verified-restore`先只读保存独立static0完整checkpoint；再用同一个Verified Device串行执行breathing亮度0、breathing亮度100、wave亮度20、RestoreLighting恢复上一breathing100、SetLighting恢复原静态目标。每步保留Command、即时ApplyReport及等待2秒后的原始快照，要求正式Applied、全部ACK和目标读回同时成立；失败停止后续试验。最后总用独立checkpoint完整尝试static-stop、原四区和原帧六步恢复，并比对全部原始配置。退出0仅代表该次五步正式接口与最终恢复均确认；没有覆盖UI交互或全部参数组合。命令不会自动执行。

2026-11-01主代理已执行该正式路径；evidence/lighting-verified-path-restore.json保存outcome=readbacks_confirmed、五步readback_confirmed均true、最终restore_readback_confirmed=true及errors=[]。主代理同时报告进程退出0。此记录补足正式接口路径证据，UI集成验收单独处理。
