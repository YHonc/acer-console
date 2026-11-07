> **历史阶段文档（2026-11-02整理标记）**：下文保留当时的计划和证据，不代表当前范围或最新包状态。UI现已选定并集成Gemini/daisyUI，电池校准取消，服务插件已接入，功能实测由用户负责。当前结论以[当前进度](当前进度.md)及[目标与验收](目标与验收.md)为准。旧产物路径见迁移记录。

# 前端实现与验证记录

更新于 2026-11-01。本记录只覆盖前端，不代表完整替代产品或硬件功能验收完成。

## 已实现

- `apps/desktop`：Svelte 5 + TypeScript + Vite；无 SSR、Node 后台服务、大型组件库或外部字体请求。
- 概览：当前性能模式、电源状态、CPU/GPU 温度与转速，安静/平衡/性能/极速/节能模式草稿和应用入口。
- 散热：实际风扇模式与传感器读数，自动/最高/独立 CPU/GPU 自定义比例；范围与能力均来自原生接口。
- 灯光：四区静态颜色、亮度和紧凑键盘示意，真实状态与待应用草稿分开；缺失字段或能力未开放时禁用。
- 设置：主题、采样、登录启动、关闭窗口保留后台、退出恢复、电池保护、AC/DC 规则、官方启动备份恢复、能力与操作记录。退出恢复错误通过事件和原生状态查询补回持续显示；跳过恢复退出需显式点击。
- 原生语义表单、明确焦点、跳转内容链接、Tab 操作、减少动态效果、强制颜色模式和窄窗口布局。
- 无硬件连接时读数为未知；浏览器预览明确显示未连接桌面设备。能力未开放时控件禁用，展示后端原因。能力 `supported=false` 显示“暂未开放”，不据此断言硬件不支持。
- 写入显示应用中/已应用/失败/未确认；硬件读数不乐观更新，后端返回后重新读取。日志仅记录真实操作结果，最多保留本会话 20 条。
- 前端轮询不重叠；页面不可见时跳过轮询，重新可见时刷新；组件销毁时释放定时器与监听。

## 桥接契约

完整类型与调用在 `apps/desktop/src/lib/api.ts`。JSON 输出字段使用 snake_case，Tauri 调用参数使用命令签名要求的参数名：

|命令|参数|返回|
|---|---|---|
|get_snapshot|无|Snapshot|
|get_capabilities|无|Capabilities|
|authorize_device|无|void（失败或取消时 reject）|
|set_profile|`{profile}`|OperationResult|
|set_fan_mode|`{mode}`|OperationResult|
|set_fan_speeds|`{cpuPercent, gpuPercent}`|OperationResult|
|set_lighting|`{lighting: {enabled, brightness, zones}}`|OperationResult（单色 color 仅保留旧接口兼容）|
|set_lighting_animation|`{lighting: {effect, speed, brightness, direction, color}}`，effect 不含 static|OperationResult|
|restore_lighting|无|OperationResult；恢复 helper 保存的上一步真实灯光，不传 raw frame|
|set_battery_care|`{enabled}`|OperationResult|
|get_settings|无|Settings|
|save_settings|`{settings}`|保存后的 Settings|
|get_startup|无|`{enabled, entry_present, mismatch}` 系统读回状态|
|set_startup|`{enabled}`|同上，写入后的系统读回状态|
|get_rules / save_rules|无 / `{config}`|规则配置、状态与原生名单 / void，保存后重新 get_rules|
|official_task|`{action}`，query/backup/disable/restore|任务状态及原生核验的备份有效性|
|get_exit_status|无|原生持续保留的退出错误与待恢复状态|
|exit_without_restore|无|void，仅退出恢复失败后的显式操作|
|get_vendor_controls|无|official_service 依赖说明及四项 enabled/writable/error/raw|
|set_vendor_control|`{setting, enabled}`|OperationResult，随后重新 get_vendor_controls 确认|
|get_usb_charge|无|state / writable / ack / error / backend=direct_wmi / raw|
|set_usb_charge|`{state: {enabled, threshold}}`，threshold 为10/20/30|OperationResult + state，仍须独立 get_usb_charge 读回确认|

`Snapshot.errors` 为 `string[]`；传感器、模式、电源和灯光未知值为 `null`。`timestamp` 兼容 ISO 日期和毫秒十进制字符串。`OperationResult.status` 为 `applied`、`failed` 或 `unconfirmed`，不能仅凭 invoke resolve 宣告硬件应用成功。

`Capability.status` 为可选字段：`verified`、`implemented_unverified`、`protocol_pending`、`permission_blocked`、`hardware_unsupported`。设置页分别展示已验收、已实现待实测、协议待逆向、权限受阻、硬件明确不支持，并保留原因。旧桥接缺少 status 时回退为可用/暂未开放，未知协议不会归为硬件不支持。

前端不负责决定硬件写入权限，所有控制继续遵守 native capability gate。本前端任务未执行硬件写入、系统启动修改或退出操作；原生端与主对话的实验不混同为本轮前端验收。

## 已完成的验证

- `npm install` 完成，生成锁文件；没有另行修改用户 npm registry 配置。
- `npm run build`：`svelte-check` 0 errors / 0 warnings，Vite 成功生成 dist。
- 首轮产物约 JS 60.9 kB、CSS 10.8 kB；这是前端静态产物大小，不是进程内存或资源实测成绩。
- 使用本地 Vite 与 IAB 实际打开页面：概览/设置/散热/灯光导航可用；未连接设备时未知状态、原因与禁用控件可见；主题选择即时变更为深色。
- 查看 960×680、1280×800、375×812 的页面截图，未发现主要内容遮挡；375 宽散热页面 DOM 测量为文档宽 360、视口宽 375，没有横向溢出。临时视口覆盖已重置。
- Browser Harness 的 daemon 无法启动，因此视觉验证改用当前客户端 IAB；未使用或导航用户的其他标签页。

上述验证未注入模拟设备快照，未发出硬件写入命令。桌面 WebView 中的真实数据、控制结果、托盘生命周期、DPI 和系统高对比度仍需集成验收。

## 后续接入

按 `docs/目标与验收.md` 持续完成完整替代目标。自定义独立风扇、四区静态/动态灯光与恢复、电池保护、规则、启动与退出恢复入口均已接入真实命令；仍需主对话完成新界面的实机验收，不据此宣布完整替代验收完成。

## 本地使用

在 `apps/desktop` 执行 `npm run dev` 启动 127.0.0.1:1420 浏览器预览；`npm run build` 做类型/可访问性静态检查及生产构建。桌面构建走主对话维护的 Tauri/Rust 流程。

## 只读授权入口增量

2026-11-01 接入主对话新增的 `authorize_device`：

- API 为无参数 `authorizeDevice(): Promise<void>`，唯一调用位置为显式按钮事件；初始化、定时轮询、可见性恢复和刷新均不调用授权命令。
- 仅在 telemetry 状态为 `permission_blocked`，或读取错误/原因包含明确拒绝访问、权限错误或对应 HRESULT 时，在概览和设置显示授权入口。
- 说明 Windows 将请求管理员权限、组件用途为只读温度/转速/状态读取，取消不自动重试。请求处理中锁住重复提交。
- 命令成功后并行刷新 capabilities 与 snapshot；若读取仍失败或仍有权限受阻，结果显示“未确认”。授权成功使用“授权完成”，不会混同为硬件写入已应用。
- 命令取消或失败保留具体错误和本会话操作记录。刷新设备能力不重置用户的设置草稿。
- 增量构建已通过，Svelte 检查 0 errors / 0 warnings。未实际调用授权命令或触发 UAC；桌面授权成功、用户取消及读取恢复场景仍由主对话集成实测。

## 状态同步与断线恢复修正

主对话实际授权读取后发现：当前模式卡已显示“性能”，选项仍保留初始化的“平衡”。现已移除性能/风扇的预选默认值，所有读快照路径（包括轮询、首次授权和重新授权）统一同步性能模式、风扇模式和已读回灯光状态。

用户改选会建立独立草稿并显示“待应用”，后台读数不会覆盖该草稿；设备读回匹配后清除待应用标记。灯光读取未知或存在草稿时，键盘预览明确标注“不代表当前灯光”，避免把示意颜色当作真实状态。

明确 broker/Transport/pipe/管道错误会显示“重新授权读取硬件”。仍仅由显式点击调用提权，初始化与轮询不重试授权。旧快照保留时，界面显示读取失败、连接不可用及最后读取时间“已过期”。托盘设置继续禁用，等待主对话实测确认。

该修正完成针对性生产构建与 Svelte 检查；未重复大范围视觉验证，也未代替主对话操作桌面授权或硬件。

## 登录启动与托盘设置增量

设置页新增独立的“登录后启动”开关，仅使用 `get_startup` / `set_startup` 的系统读回结果，不写入或读取 settings JSON 中的启动布尔值。初始化及手动刷新只读取，唯一写入路径是用户显式切换开关。请求期间保持上次读回的勾选状态，返回后再同步，失败时显示原因并重新读取系统状态。

界面区分启用、关闭、启动项存在但未启用、目标不匹配与读取失败；读取失败时禁用开关。系统读回与请求不一致时显示“未确认”。成功仅说明系统启动项状态，明确标注实际登录启动尚待系统重新登录验收。本次未调用真实启动写入接口。

按主对话最新接入状态，“关闭窗口时保留托盘”已开放修改及通过“保存设置”保存，标注生命周期待实际验证；`restore_on_exit` 继续禁用。前文“托盘开关禁用”是早期状态，以本增量为准。

## 2026-11-01 视觉一致性收尾（当前状态）

本节覆盖上文早期阶段的禁用/待接入描述。`close_to_tray` 与 `restore_on_exit` 目前均已开放保存，由原生校验是否具备读取和恢复条件。

依据 `ui-reference-review.md` 中已查看的 G-Helper、Macs Fan Control、Stats 与 Linear 真实参考，保留紧凑侧栏、短工具栏和设备分组；没有新增营销页面、模拟设备、依赖库或装饰动画。

|发现|证据|本次处理|
|---|---|---|
|P1：控件半径/高度/禁用透明度散落，规则不一（§1.5、§1.4.3）|源码核对|四个组件共同使用 small-radius、control-height、disabled-opacity；开关统一尺寸|
|P1：多轮 CSS 覆盖与 10px 窄窗文字（§4.1、§3.2）|源码和实际截图|合并全局样式，保留 12/14px 辅助与正文，不再压至 10px|
|P1：普通成功反馈占用过大的持久面板（§2.4）|主对话实际桌面反馈、源码|改为可关闭的紧凑状态行，退出恢复错误仍保持醒目、持续|
|P1：未知与编辑默认值的视觉权重容易混淆（§8.4、§5.7）|浏览器实际灯光页|未知颜色采用中性键盘示意；实际颜色与草稿、待应用标记分开|
|P2：相同错误重复、无读数却显示“同步/过期”|浏览器实际未知状态|去除重复设置错误；无历史快照显示等待连接；仅已有快照标过期|

组件保持原有真实调用链：FanControls / LightingControls 的 Promise resolve 不作为硬件已生效证据，读回匹配才同步草稿；PowerRules 只保存配置；OfficialStartup 只使用原生 backup_valid 与原始状态，禁用仅针对官方界面登录启动，不卸载或停服务。

### 本轮实际覆盖

- `npm run build` 完成：Svelte 检查 0 errors / 0 warnings，生产构建成功。
- 在 IAB 使用真实前端、本地 Vite 和浏览器预览的实际无原生连接状态，未注入模拟快照。
- 实际查看 1280×800 浅色概览、960×680 散热/设置、深色灯光，以及 375×812 窄窗浅/深主题。四个页面的 document.scrollWidth 均不超过视口；短散热页无纵向滚动条，因此 clientWidth 为375，其他长页面为360，均无横向溢出。
- 窄窗可见正文/标签/导航测得为 12px 或14px。Tab 从设置导航移动到“刷新设备状态”，焦点框 computed outline 为 solid / 2px。
- 从实际页面 CSS 变量读取并计算基础 text/secondary/muted 在 canvas/surface/raised 上的文字对比度：浅色最低 4.55:1，深色最低 5.35:1。这仅覆盖这些基础色对，不等于完整无障碍审计。
- 实际观察了未知读数、禁用控件、原生 API 不可用错误、主题预览及页面导航。浏览器控制台本轮读取无 error/warn。
- 已保存预览截图：`%USERPROFILE%/.codex/visualizations/2026/11/01/01a19615-548e-7bd0-a245-395944b4d63a/control-light-polish.jpg`、`control-dark-lighting-polish.jpg`。截图为浏览器预览、未知状态，不是硬件成功证据。临时视口覆盖已重置。

### 尚未由本轮实际窗口覆盖

- 本轮未进入 native WebView 操作，也未执行授权、硬件写入、退出、启动项、备份或恢复变更。
- PowerRules 的真实可编辑状态、自定义风扇滑杆的真实能力开放状态，以及实际 busy、成功反馈、退出恢复异常只做源码/构建检查；未为补截图而伪造接口或事件。
- 重新打包后的稳定 Release 桌面窗口、Windows DPI 125/150/200%、系统高对比度、屏幕阅读器和各托盘菜单逐项验收仍由主对话继续。
- 主对话报告已实测登录启动 on→注册表核对→off、退出风扇 Max→Auto 恢复、关闭窗口释放 WebView 后保留进程与再次运行重建同一进程窗口。本轮仅据这些消息更新“关闭窗口保留后台”说明，未将其扩大为所有托盘菜单或所有电源/恢复分支已验收。

## 厂商设备设置接入（2026-11-01）

新增 `src/lib/VendorControls.svelte`，在设置页按现有紧凑样式显示 LCD响应加速、启动声音、粘滞键、闲置30秒关闭键盘背光。仅说明依赖官方 Acer 后台服务，不显示 dependency 内的连接细节、端口或 raw 数据。启动声音注明开关状态已验证，重启时的声音效果尚待实测。

原生查询与写入契约见上表，setting 使用固定的四项联合类型。只有设备访问授权有效、对应 enabled 为 boolean、writable=true、无该项读取错误、状态新鲜且没有忙碌请求时才开放开关。点击先恢复当前实际勾选状态，写入后重新查询；只有 applied 且新读回匹配才显示确认。请求失败、读取失败或结果不一致时保留上次状态并标记过期，锁定开关，允许显式刷新恢复。

查询由进入设置页、授权命令完成、页头手动刷新或组件手动刷新触发。没有接入传感器定时轮询；普通 device-refresh 全量同步也不递增本组件查询版本。并发刷新合并，卸载后不再更新组件状态，也不在卸载后启动写入完成后的后续查询。仅明确用户切换会调用 set_vendor_control，没有自动重试写入。

保留主对话新增的 previewTheme API 及 theme effect，未修改 Rust、主题 tokens 或其他组件。

验证：`npm run build` 通过，Svelte 检查 0 errors / 0 warnings；在真实浏览器预览的无原生连接状态下，实际查看 960×680 浅深主题、四项未知/禁用、服务读取错误和刷新按钮焦点，样式与既有分组一致。预览截图保存为本任务 visualizations 目录中的 `vendor-controls-light.jpg`、`vendor-controls-dark.jpg`，临时视口覆盖已重置。未注入模拟数据、未点击真实设备开关。本轮未验收 native 成功/读回不匹配/异步卸载等运行分支，这些仅经源码与构建核对；主对话继续实机验收。

## 正式动态灯效桥接（2026-11-01）

新增 `LightingEffects.svelte` 并接入同一灯光页，四区静态编辑继续由 `LightingControls.svelte` 承担。当前效果、亮度值、适用速度/方向/颜色只来自 `snapshot.lighting_config`；该字段缺失为未知。前端不会把动态效果的 `zones=null` 显示成真实白色/黑色四区。

支持的正式 effect 为 static、breathing、neon、wave、shifting、zoom、meteor、twinkling。动态编辑速度1–9、亮度应用输入0–100；仅波浪/渐变显示方向1/2，霓虹/波浪不提供单色控制。未确认方向的物理左右映射，因此界面使用方向1/方向2；亮度值只按桥接输入范围呈现，不宣称已确认官方档位换算或实际光强百分比。动态亮度0/100的本机效果仍待主对话实测。

已知原生状态用于初始化草稿；缺少依据的速度5、亮度50、方向1、白色只属于明确标注的待应用参数。效果和输入改动只编辑草稿，不发送请求；按钮才发正式类型化命令。收到 Promise resolve 不自动宣布生效，只有当前配置与所选适用参数一致才提示读回一致，失败明确显示。

切换到 static 时，如果当前不是静态或缺少真实四区，四个颜色草稿初始均为“未选择”，不再自动填入白色；可逐区选色，或显式点击“填充四区”使用统一颜色。全部四区为有效十六进制颜色且亮度为有效整数前禁止应用。此操作只填草稿，不自动写入。

静态、动态和恢复分别使用 lighting_control、lighting_dynamic_control、lighting_restore_control 的 supported gate，并要求存在新鲜快照且无权限受阻。restore_lighting 为无参数显式调用，恢复 helper 保存的上一状态；前端不保存或传入 raw frame。灯光操作后刷新真实快照与能力，更新恢复入口。

保留 previewTheme API/effect、Capability.diagnostic_reason、设置页“技术详情”折叠块；能力列表新增动态灯效与灯光恢复，没有修改 Rust 或执行硬件写入。生产构建与 Svelte 检查通过（0 errors / 0 warnings）；本轮以类型、逻辑分支和构建检查为证据，未模拟 native 数据或事件，未将之前的静态截图当作动态参数页验收。新界面实际效果切换、静态完整选四区、读回、恢复与亮度边界均待主对话验收。

## USB关机供电接入（2026-11-01）

新增 `UsbCharge.svelte`，在设置页电池保护附近单独分组。该功能控制USB关机供电及电量截止阈值，界面明确与电池充电上限保护不同，并保留“物理关机供电效果仍待测试”说明；没有宣称官方服务缓存已同步。

初始没有假状态或默认阈值：仅 get_usb_charge 的 confirmed、无错误、合法 state 可初始化草稿。enabled 开关与10/20/30%阈值分开编辑，改变阈值只改 threshold，保留 enabled（包括 false），仅显式“应用USB设置”调用 set_usb_charge。没有 state、应答 rejected/unknown、错误、过期、未授权、不可写或忙碌时均禁用编辑；保留的旧草稿明确显示未确认。

写入返回 applied 后，还必须重新 get_usb_charge 并得到 confirmed、无错误、enabled 与 threshold 均一致，才显示读回确认；不使用 setter 返回的 state 直接伪造当前值。failed/异常/未确认/读回不一致均保留错误或未确认提示并暂停编辑，允许手动刷新恢复。

沿用厂商组件查询版本：设置页挂载、授权完成、页头手动刷新及组件手动刷新时查询，不加入2秒传感器轮询；合并并发刷新，组件卸载后不更新状态或继续写入后的查询。原有主题、能力技术详情与动态灯效保持。

本轮 `npm run build` 完成，Svelte 检查 0 errors / 0 warnings。实际在 IAB 的无原生连接状态下查看960×680浅深主题，确认独立分组、未知、禁用、错误提示及焦点样式。截图为任务 visualizations 目录中的 `usb-charge-light.jpg` / `usb-charge-dark.jpg`，临时视口覆盖已重置。没有注入模拟数据、没有点击真实USB开关或阈值；原生新界面的写入/读回/恢复和真实关机供电仍由主对话验收，构建不等于完整功能验收。
