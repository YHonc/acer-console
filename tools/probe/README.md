# 设备探针与受控验收

`cargo run -p acer-probe` 输出快照、每项WMI输入和原始返回/错误。
`cargo run -p acer-probe -- --capabilities` 输出目前可读能力及未验证原因。

默认和--capabilities不执行Set方法，不自动提权、不修改服务或启动项。普通权限访问被拒时，保存原始JSON给主对话统一处理权限；不要借助参考脚本启动硬件实验。

`./tools/probe/Inspect-Signatures.ps1` 仅读取CIM类元数据，默认写signatures.local.json。该文件已在2026-11-01本机读取成功。元数据读取成功不代表实例可访问。

快照中的模式和转速映射来自固定上游源码，尚待本机与官方界面对照。温度按本机能力掩码读取；lighting_config在结构有效时解析已知灯效0..7，动态时zones为null，静态缺区同样不补默认颜色。未知值为null并保留原始返回/错误。

## 独立受控验收命令

`acer-probe --validate-fan-max-restore` 是显式硬件写入命令，只供主代理审查后统一执行；本轮开发没有运行它。

命令先确认PHN16-71 / BIOS V1.18、原模式Auto或Max，以及CPU/GPU转速均可读取。Custom、混合或未知状态拒绝执行。通过后用native AllowUnverified写Max，保存原始请求/返回及即时读回，等待3秒采样模式和RPM，再恢复原模式并再次读取。

后端在Max写入前保存原Auto/Max；后续读取失败也会尝试恢复这个已保存值，不以新的读取成功为前提。恢复接口不接受前端参数，不暴露为UI命令。中途API错误、固件拒绝、读回失败和恢复失败均保留在最终JSON。退出码0表示本次Max和原模式均在读回中观察到；1表示拒绝、验收失败或恢复未确认；2表示参数不合法。模式读回和RPM记录是一次受控实验的证据，不能代替完整功能验收。

`--validate-profile-balanced-restore`短暂Balanced后恢复原非Eco模式。`--validate-profile quiet --restore`和`--validate-profile turbo --restore`只允许这两个显式目标；缺--restore、Eco和任意方法均拒绝。要求AC在线、电量>=40%、原/目标均在支持掩码，恢复时再次确认供电条件，供电丢失则记录原模式待恢复。官方手册第53页确认AC及40%条件。

`--validate-fan-custom-restore`固定Custom、CPU70/GPU70三步，确认所有ACK、三份Custom行为和两个百分比后恢复原Auto/Max。任何中途失败也尝试恢复，不因当前Custom阻断。原百分比与全部写返回记录在快照；恢复行为不额外写旧的低百分比，缓存目标可能保持70。

`--validate-lighting-brightness-restore`只接受原静态0、getter15字节+独立gmReturn0、四区均可读。按本机DLL重构setter16（byte9=键盘1，byte15=零padding，gmReturn不拼载荷），只改brightness字段2为20，等待2秒读回，随后恢复保存的原frame。报告明确保留observed_state15、setter_frame16、映射说明和原4区；比较按设备号映射归一化，其余状态与4区必须不变。中途失败也尝试原frame恢复。本后端对话未运行该硬件命令。

本机官方SUPPORT_FAN_CONTROL IPC确认CPU/GPU范围0..100；实际实验只覆盖70点，不代表全范围物理验收。Verified控制逐次确认请求百分比，当前Custom可切回Auto/Max。百分比是配置目标，Auto/Max中可能为上次Custom缓存，不是物理占空比。

测试/build/help输出与EXE时间、哈希保存在logs/。--help仅打印用法并退出0，不连接WMI。

2026-11-03 整理：重复的 logs/formal-lighting-help.txt 已隔离至项目根目录 _deleted/tools/probe/logs/；同内容的 dynamic-effects-help.txt 保留，其他日志和原始证据未移动。历史帮助输出不保证覆盖当前全部命令。

`--validate-lighting-static-colors-restore`固定亮度20及红绿蓝白四区，原static0/4区/帧齐全才执行；每个trial ACK未知/失败即停止，恢复则完整尝试原4区及原帧全部5步，不因某步失败跳过其余。报告保存原/目标及恢复的全部WMI数据。

`--validate-battery-care-restore`仅反转健康充电mask1并恢复原bool，拒绝校准/其他功能活跃或原bool不可靠状态。ACK要求UInt16 uReturn0，读回检查目标及其他状态不变；不执行mask2校准，也不通过该实验宣称80%物理截止阈值已测。

`--validate-lighting-effect 1 --restore`的目标可逐项替换为2..7，一次一个，0不重复静态验证。原static0完整帧与4区保存后才发固定动态载荷，之后先静态亮度0停止动画，再恢复原4区和原帧；每个效果都独立完成恢复。此命令的模拟单元测试与实际硬件验收分开，主代理负责统一执行与保存实机记录。

普通Verified灯光接口及会话内恢复契约见docs/backend-api.md；探针不提供任意apply命令。应用提供0..100亮度输入，官方UI1..5档到协议值映射尚未确认。七个动态效果已由主代理逐项完成brightness20受控实机读回/恢复；正式接口端点和UI路径另行验收。

`--validate-lighting-verified-restore`用于固定的正式接口验收：原static0帧和四区完整才只读保存独立checkpoint；随后使用同一个Verified Device依次正式apply breathing亮度0→breathing100→wave20→RestoreLighting恢复breathing100→SetLighting回原静态目标。全部动态配置speed3/right1，使用颜色时橙色255/128/0。每步记录command/apply/等待2秒后的snapshot，要求正式Applied、全部ACK及目标读回一致才继续。任何失败停止后续试验，最终仍用独立checkpoint执行static-stop+原四区+原帧六步恢复，不受“上一操作”记录覆盖。报告保留每步确认、最终完整恢复和错误；退出0要求五步及最终恢复全部确认，1表示拒绝/失败/未确认，2表示参数不合法。命令由主代理显式执行，不自动运行。

主代理已于2026-11-01运行该命令并报告退出0；evidence/lighting-verified-path-restore.json中五步和最终完整恢复均确认，outcome=readbacks_confirmed、errors=[]。该结果属于正式后端路径实机证据，UI交互验收另行记录。
