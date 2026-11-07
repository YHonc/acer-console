> **历史阶段文档（2026-11-02整理标记）**：下文保留当时的计划和证据，不代表当前范围或最新包状态。UI现已选定并集成Gemini/daisyUI，电池校准取消，服务插件已接入，功能实测由用户负责。当前结论以[当前进度](当前进度.md)及[目标与验收](目标与验收.md)为准。旧产物路径见迁移记录。

# Windows 后端交付记录

日期：2026-11-01；目标PHN16-71 / BIOS V1.18。

当前接口以backend-api.md为准；下文保留分阶段证据，早期“未开放”描述以最新阶段为准。最新稳定版Verified已支持Auto/Max、官方0..100自定义风扇目标、AC条件下四种非Eco模式、四区静态颜色/亮度、动态1..7及会话内上一灯光配置恢复、health-mode开关。主代理已完成七个动态效果的逐项受控实机读回/恢复，以及正式接口亮度0/100、静态与动态切换、上一动态恢复和最终原始完整配置恢复；UI集成验收由主代理继续。本后端对话未执行硬件写入。当前93项测试通过，日志和二进制记录为tools/probe/logs/verified-lighting-path-*。

## 已实现

- acer-protocol：serde数据契约、原始有符号/无符号值及字节数组、Boolean和固件低8位状态双校验、模式与Auto/Max及独立风扇百分比编码、BatteryControl读写参数编码、温度/能力掩码与静态四区RGB候选编解码。
- acer-device：原生wmi 0.18.4 COM连接、固定拥有线程、串行请求、可clone Device、原始返回和结构化PermissionDenied错误。没有采样PowerShell子进程。
- acer-probe：默认及--capabilities只读JSON；另有明确隔离的--validate-fan-max-restore受控风扇验收命令，不自动执行。
- tools/probe/Inspect-Signatures.ps1：只读类/方法/参数签名导出。结果为signatures.local.json，不含序列号。
- 原始UInt64传到JavaScript时以字符串保存，避免大于2^53的数据损失。

## 本机证据与限制

Get-CimClass成功读取AcerGamingFunction和BatteryControl的完整签名。普通权限Get-CimInstance失败（无法从客户端访问CIM资源）；主对话另核对Get-WmiObject拒绝访问。随后主对话成功执行管理员只读Rust探针，原始样本见evidence/rust-probe-elevated.json。后续主对话统一执行一次Auto→Max→Auto受控实验，见文末增量记录；后端开发对话未自行执行硬件写入、停止服务、修改启动项或BIOS/EC操作。

元数据声明Boolean返回，但本机实际Getter输出省略ReturnValue。最初强制Bool(true)检查错误地将有效读数屏蔽为null，已修正并加入真实样本回归：无调用异常、无显式失败ReturnValue，且固定Getter返回结构完整时允许读取；数值gmOutput仍验证低8位status=0。显式false/非Boolean值仍拒绝，Set方法缺失ReturnValue不按Getter规则放行。Battery要求uFunctionList、5字节uFunctionStatus和2字节uReturn结构齐全，且uReturn=[0,0]、status[0]=0/1才解码。该修正源自本机证据，而不是普遍放宽返回值处理。

管理员首次样本原始值：CPU风扇980224→3829RPM，GPU风扇960000→3750RPM；模式1024→Performance；CPU行为256/GPU行为16384→Auto；四区颜色均3350069248→RGB(0,174,199)，静态亮度0；Battery uFunctionList=3、uReturn=[0,0]、status全0→充电保护关闭。传感器掩码9244246016声明CPU/GPU温度，修正后待再次读取具体温度。这些是实际接口读值，仍需与官方页面对照，并非写入验收。

经独立下载核对OpenSense固定提交后，采用更强的交叉证据取代早期setfans.ps1推导：GetGamingSysInfo低8位状态须0、数据取bits8..23；风扇CPU行为取bits8..9、GPU行为取bits14..15，1/2/3为Auto/Max/Custom；GPU查询mask是8，组合查询9，旧脚本的3不作为正式GPU查询。原始GetGamingFanSpeed(1/4)保存的是目标百分比而非实际RPM。尚未与本机官方界面对照。

温度实现先读GetGamingSysInfo(0)能力掩码，bit24对应CPU、bit33对应GPU1；仅声明的传感器才发0101/0A01。状态不为0、0°C、超出1..125°C范围均保留未知。GPU休眠可能导致0返回，不能显示假0°C。风扇速度同样只接受状态0和合理范围。

## 待验证与待逆向（不是本机明确不支持）

- 温度、传感器掩码和风扇行为布局已实现候选映射，仍待本机读回验收；独立Custom状态/原始百分比恢复需继续验证。
- 四区RGB字节序已由交叉源码确认R/G/B在bits8/16/24。已读取DeepSeek更新的本机AcerECKeyboardController.dll 1.0.3.10布局证据，定版为[mode,speed,brightness,0,direction,R,G,B,3,1,0...]；现有backlight_payload与之相符，本轮未开启UI写入。只有当前静态、gmReturn=0、header形状符合时才解码结构化Lighting；动态模式保留原始数据。SetLighting执行层仍拒绝，待主代理单独安排写回验收。
- BatteryControl uFunctionList能力掩码与uReturn含义；充电80%阈值不能仅凭README称为本机验证。

当前没有证据证明上述功能在本机明确不支持。缺失协议继续由源码对照和官方组件逆向解决。

## 写入接口

Device::connect默认ReadOnly。前端JSON不能改变策略。Device::connect_with_policy(WritePolicy::AllowUnverified)仅供原生监督验证会话，要求精确机型和V1.18、能读取当前值、Performance/Turbo确认AC供电；写入后保存before、writes和after，返回Failed或Unconfirmed，不声称完整验收。灯光即使设置此策略仍拒绝，直到主代理安排专门写回验收。

AllowUnverified已由主对话用于下述一次受控风扇实验；后端对话未运行实验命令。实际用户界面由主代理集中处理权限和策略，不自动UAC提权。灯光、睡眠恢复、AC/DC限制和长期稳定性等尚未完整验收。

## 验证

协议单元测试12项，覆盖模式往返/未知模式、两个风扇编码字、独立百分比范围和顺序、组合风扇混合状态、UInt64 JSON精度、显式返回码、充电双状态、RPM哨兵、温度/掩码、RGB载荷、固件写状态、真实管理员输出回归。适配层2项测试权限错误分类和原始数组/UInt64保真。2026-11-01自行运行cargo test -p acer-protocol -p acer-device，14项全部通过；cargo build -p acer-probe成功。fixture副本已规范为无BOM UTF8，原始evidence未改。尚未自行运行硬件写入，探针重采由主代理统一执行。

## 受控风扇验收CLI（2026-11-01增量）

--validate-fan-max-restore单独出现时才选择AllowUnverified。流程要求PHN16-71/V1.18、原Auto/Max、CPU/GPU转速前读均成功；Custom/混合/未知拒绝。写Max后保存即时读回，等待3秒再采RPM和模式，随后恢复原模式并再次读取。JSON包含时间、全部原值、两次ApplyReport及原始写返回、稳定后和恢复后的快照、分阶段错误及恢复确认状态。

为避免Max之后读失败阻断恢复，acer-device在Max请求发送前自行保存已验证的原Auto/Max；native restore_validation_fan无参数接口可恢复该值，不重新依赖当前读成功。没有已保存值或ReadOnly策略均拒绝；恢复明确失败/读回不符时保留保存值供后续恢复处理。此接口未添加到Tauri/UI命令。

新增11项probe测试，覆盖精确CLI参数、成功顺序、非目标机、Custom/未知/初读失败、缺失RPM、Max错误后的恢复、稳定读取失败后的恢复、原Max恢复Max、恢复API错误/读回失败/读回不匹配。连同既有12协议+2适配测试，cargo test -p acer-protocol -p acer-device -p acer-probe共25项全部通过；cargo build -p acer-probe成功。所有失败测试使用fake设备，不触碰WMI；本轮没有运行显式硬件验收命令。

## Verified Auto/Max白名单（受控实机证据之后）

主代理已执行一次受控命令，evidence/fan-max-restore.json记录outcome=readbacks_confirmed、errors=[]。原Auto，Max稳定后CPU/GPU为7435/6534RPM，随后Auto读回确认。两次SetGamingFanBehavior写回gmOutput都为0且均没有ReturnValue。这证明该机型/BIOS的一次AC场景切换与恢复，不延伸为DC、睡眠恢复或长期验收。

新增native WritePolicy::Verified，仅精确PHN16-71/V1.18可调用SetFanMode Auto/Max；性能、充电、自定义百分比、灯光仍拒绝。默认connect仍ReadOnly，策略不经serde暴露给UI。前读须原Auto/Max、CPU mask1/GPU mask8/组合mask9模式一致并且双RPM可读；因此未知或Custom不被覆盖。

Applied仅在这一固定setter和精确参数匹配时允许省略ReturnValue，gmOutput须精确0，任何显式false/非Boolean或非0固件返回拒绝；缺失ACK保持Unconfirmed。after同样要求CPU/GPU/组合三份状态0并与目标一致才Applied；明确错误Failed，读回失败/不全/不符Unconfirmed。模式确认不要求RPM立即下降，原始快照保留其异步收敛过程。恢复接口在Verified策略下使用相同确认准则。

capabilities仅该目标机fan_mode可标Verified；writable同时要求当前连接策略Verified和上述前读条件。ReadOnly连接不会宣称可写。新增策略白名单测试及两项真实实验fixture回归；当前14协议+3适配/策略+11probe共28项测试全部通过，acer-probe构建成功。后端未额外执行硬件写入。

## Profile与Custom增量

独立读取官方PHN16-71手册第53/54页，AC连接且电量至少40%是模式切换条件；第54页确认CPU/GPU独立Custom滑杆但没有给出数值范围。主代理分别执行Balanced、Quiet、Turbo后恢复Performance，evidence/profile-{balanced,quiet,turbo}-restore.json均保存成功读回和恢复。Verified由此允许Quiet/Balanced/Performance/Turbo，仍严格检查支持掩码和AC>=40%，独立profile guard不依赖任何风扇读数。Eco未做电池场景验收，保持拒绝。

主代理Custom70实测evidence/fan-custom-restore.json中三setterACK均0且无ReturnValue，行为3与两个目标70均读回，随后Auto恢复。另独立核对references/research/official-ipc/SUPPORT_FAN_CONTROL.json，官方CPU/GPU各为min0/max100且result0。后端range、guard、能力reason均0..100，不保留临时70下限；只把70标为物理实测点，其余请求逐次验证配置读回。零目标不当成物理停转承诺。

Snapshot加入可选cpu_fan_percent/gpu_fan_percent，Auto/Max时是缓存目标。FanMode加入Custom，CPU/GPU/组合三份行为一致才识别；正常Auto/Max可从Custom切回，受控实验仍要求原Auto/Max。id=fan_custom。Custom每次三步ACK及两路请求百分比逐项确认后才Applied，0/100仍发Custom三步，避免与Auto/Max命令混淆。

## 灯光brightness20受控验证阶段

新增--validate-lighting-brightness-restore，仅原static0、15字节getter完整、gmReturn0、四区均成功时执行。根据本机DLL将getter15重构为setter16：byte9固定设备号1、byte15为0 padding，状态码gmReturn独立保留；报告保存observed_state、setter_frame、mapping和zone_words。只改字段2为20，保持其他输入状态，等2秒读取，再直接恢复保存的原frame。恢复不依赖当前效果或当前读取成功。比较时归一化设备路由字段，其他观察字段与原四区必须一致；不宣称实读了原始16字节。

当前57项测试全部通过（22协议、4设备、31探针），覆盖真实fixture、映射正反例、未知/失败ACK、非static/缺区拒绝、写失败/读失败后的恢复、恢复失败显式反馈。cargo build -p acer-probe与exe --help都退出0；实际日志tools/probe/logs/lighting-validation-{tests.log,build.log,help.txt,binary.json}。EXE修改时间2026-11-01T09:34:32.6031142Z，SHA256 E7D3BD2EFCF6D21FEED291AE8378A4EDF6A32DE4C6616EDBBE31FE8FC0AA0214。没有运行灯光硬件命令，已交回主代理审查后统一执行；没有顺带开启UI灯光或充电写入。

## 已验收静态灯光与电池开关

主代理随后完成brightness20→原值、静态红/绿/蓝/白四区→原四区/帧，以及电池保护false→true→false，均readbacks_confirmed、errors=[]，数据分别为evidence/lighting-brightness-restore.json、lighting-static-colors-restore.json、battery-care-restore.json。静态方法ACK为gmOutput0且缺ReturnValue，电池setter ACK为UInt16 uReturn0。健康充电function mask1，校准function mask2从不写入。功能支持列表/其他状态保持不变；物理充电截止阈值没有测量。

据此已开放Verified静态SetLighting与SetBatteryCare；两者前置检查互相独立，也不依赖风扇状态。静态每次目标亮度帧+四区逐步ACK和目标读回，电池每次检查可靠bool/其他功能idle、精确uReturn及其他状态未变。75项稳定版测试已通过，日志verified-static-battery-*。此后新增的Snapshot lighting_effect按0..7报告已知灯效，未开放动态普通写入。

## 动态单项受控CLI

`acer-probe --validate-lighting-effect 1 --restore`允许数值1..7；0、8、缺恢复参数均拒绝。一次仅一项，固定speed3、brightness20、direction1，需颜色的效果用橙色255/128/0；Neon/Wave忽略颜色字段，方向只对Wave/Shifting做有意义的确认。所有项检查effect/speed/brightness/状态。原static0帧与四区齐全、可恢复才保存receipt并写入；写或读失败也尝试恢复，先静态亮度0停止动画，再原四区与原完整帧共6步，任一恢复失败不跳过其余步骤。

报告包含target_effect/code/参数、原BacklightFrame、trial_apply/target_snapshot、restore_apply/restored_snapshot、两个readback bool和errors。当前82项测试实际通过（28协议、5设备、49探针），其中七效果循环是明确标记的模拟配置测试；cargo build -p acer-probe与exe --help退出0。日志dynamic-effects-{tests.log,build.log,help.txt,binary.json}；EXE UTC2026-11-01T10:23:26.6366871Z，SHA256 C69DA69BFF4E5ACBEE1D13432FF3BC16A2D19C38C120B6E7E719B26E8863289C。没有自行执行任一动态硬件命令，已交主代理统一安排。

## 正式动态灯光与会话恢复

主代理逐项执行evidence/lighting-effect-{1..7}-restore.json，七份均为outcome=readbacks_confirmed、target_readback_confirmed=true、restore_readback_confirmed=true及errors=[]。参数均为speed3/brightness20/direction1，需颜色时为橙色255/128/0；证据不扩展为全范围、动画相位或普通UI路径验收。真实WMI输入/ACK/读回的紧凑fixture为crates/acer-protocol/tests/fixtures/phn16-71-seven-dynamic-configs.json。

正式Command新增SetLightingAnimation与RestoreLighting{}；Snapshot新增lighting_config，可选zones在动态或静态缺区时为None/null。当前配置解析扩展到0..7，支持静态→动态、动态→动态、动态→静态；正常灯光写入在首步前保存worker内上一实际配置。恢复只使用后端记录，不接收前端原始帧，不因当前前读失败放弃尝试；全部恢复步骤继续执行，成功确认后才清除记录。正常切换和动态恢复的测试明确为模拟配置测试。

应用提供brightness0..100输入，官方UI1..5档到协议值的映射尚未确认。已核对本机DLL的SetKeyboardBrightness和SetMode都写入同一byte2，没有据此声称官方数值上限；正式接口breathing亮度0/100及恢复的实机证据见下文。

86项测试实际通过（32协议、5设备、49探针），cargo build -p acer-probe与exe --help实际退出0；后者只打印帮助且不连接WMI。日志formal-lighting-{tests.log,build.log,help.txt,binary.json}。探针EXE UTC2026-11-01T11:18:30.1950709Z，SHA256 0BD357D5E63C1583B5E909AC7E4AF3E688ADF5B6D17E98D2CFF7BCF9B2179468。探针未提供普通apply CLI；正式路径应通过持有同一Verified Device会话的broker/helper验收。

## 正式路径验收CLI

新增唯一显式flag `--validate-lighting-verified-restore`。AllowUnverified worker首先仅读保存static0原帧及四区独立checkpoint；另一Verified Device串行走正式Command：breathing亮度0→breathing100→wave20→RestoreLighting恢复上一breathing100→SetLighting回原静态目标。每步保存command、apply、settled_snapshot及readback_confirmed；只有即时正式Applied、原始ACK及延迟读回均成立才继续。最后无论试验成功或失败，始终尝试独立checkpoint六步恢复原始完整状态，防止正式“上一操作”记录被后续请求覆盖。没有前端原始帧接口。

新增7项模拟测试覆盖正常五步、每一步在状态已改变后失败、读失败、Applied标签但ACK缺失、恢复缺步、最终读失败及checkpoint拒绝；不连接WMI。实际测试共93项通过（32协议、5设备、56探针），build与help退出0。日志verified-lighting-path-{tests.log,build.log,help.txt,binary.json}；EXE UTC2026-11-01T11:30:34.2250808Z，SHA256 C778000E809D17F1D1EA28DCC6EA77F5633919A636860A04CD472896C36A09BC。命令交主代理执行，本后端对话未执行。

主代理核对上述C778000E…二进制后已运行该命令，报告退出0。本后端随后只读核对evidence/lighting-verified-path-restore.json：outcome=readbacks_confirmed，五步static_to_breathing_brightness_0、breathing_brightness_100、dynamic_to_wave、restore_previous_breathing_100、dynamic_to_original_static的readback_confirmed均true；最终restore_readback_confirmed=true，errors=[]。这确认该次正式Verified接口端点与切换/恢复路径，以及独立checkpoint原始完整frame/四区恢复；未扩展为UI交互、全参数组合或长期验收。后端代码停止新增灯效改动，所有权交回主代理供UI集成。

## 依赖核对

Context7查询失败、docs.rs直连EOF后，已通过官方仓库ohadravid/wmi-rs的main/src/method.rs、connection.rs、variant.rs和result_enumerator.rs核对0.18.4 API：with_namespace_path单参数；exec_method接受输入签名实例；put_property可构造UInt8数组；WMIConnection为!Send，所以固定在创建线程。

固定硬件协议参考为references/source.json中的96f3659b748c11e98c8cf1023265a1b6b9e5ca9a，源许可保留在references目录。

交叉核对协议：archivesteak/opensense固定b5911c1712bb862bb13023bd3cc7b054b4461087的src/OpenSense.Core/Hardware/AcerProtocol.cs（状态17–20、sensor24–37、fan43–94行）及KeyboardProtocol.cs（payload86–123、RGB135–142行）。只按协议语义独立实现Rust函数，未复制项目源码。完整冲突和其他机型证据见docs/reverse-research.md。
