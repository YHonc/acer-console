> **历史阶段文档（2026-11-02整理标记）**：下文保留当时的计划和证据，不代表当前范围或最新包状态。UI现已选定并集成Gemini/daisyUI，电池校准取消，服务插件已接入，功能实测由用户负责。当前结论以[当前进度](当前进度.md)及[目标与验收](目标与验收.md)为准。旧产物路径见迁移记录。

# QuickAccess 屏幕设置实现依据

2026-11-02，只读源码核对，未发送设置请求。Sol 子代理定位，主代理复核真实 adapter 的护眼、屏保超时及服务端口片段。

本机提取资源：`references/research/reverse-continuation/official-app/main.be349a7a7c9ce11c.js`。文件另含 SimulatorMode，以下使用 `tr.instance.sendRequestPacket` 的真实 adapter，不采用模拟对象的 writeData 作为硬件证据。

## 固定接口

服务绑定 `tr` 的实现是 `let tr=(()=>{class a extends zS...`，ServiceName=QuickAccess。基类固定连接 `wss://localhost:5141`，不是明文ws。需要官方认证/会话握手；认证材料不写入本文。请求沿基类 sendRequestPacket：PacketType=Request、Version默认1、Session为UUID、Command为固定功能名；值为null时Action=Get，否则Action=Set、Param1为值。收到Set响应后官方再发Get，不能以发送成功代替读回。

|功能|已确认 setter 参数|结果与边界|
|---|---|---|
|BluelightShield|选择值原样传入Param1|Result.Value；五个选项为Customized、Low、Medium、High reduction、Long Hour Mode。0–4映射仍属推断，需服务证据或读回验证|
|DarkMode|布尔|Result.Value；UI取反当前值|
|TransparentEffect|布尔|Result.Value；位于OLED相关设置模板|
|AutoHideTaskbar|布尔|Result.Value；位于OLED相关设置模板|
|ScreenSaver|布尔|Result.Value；官方文案明确Acer Screen Saver，不能直接等同Windows默认屏保|
|ScreenSaverTimeout|固定600|官方setter不接受任意值；单位尚未由服务确认|
|HDR|未找到本bundle对应实现|既有日志报告支持，不等同存在本机可见入口或已确认setter|

定位：请求构造约字符827401，WebSocket发送约828938，真实adapter约846302–847300，UI分发约1218427–1219300，OLED模板约5441293。压缩文件偏移随版本变化，应同时按符号定位。

## 实现顺序

1. 只读核实本机5141服务进程、版本和受支持命令，区分能力列表与实际可见入口。
2. 检查认证/会话处理及响应错误，保留官方接口规则，不绕过认证；原始认证材料不进入日志。
3. 实现固定命令/参数的Rust客户端和严格响应解析，先Get，再做独立Set/Get/恢复验收。
4. 若需脱离QuickAccess服务，再检查服务二进制确定真实Windows API和用户会话要求；不能按常见注册表经验冒称与官方等价。

## 本机连接检查

**后续证据更正（2026-11-03 整理）**：现存 `evidence/quickaccess-read.json` 显示 authenticated=true、writes_sent=0；Customized 实际返回值 5，ScreenSaverTimeout 的响应说明单位为 seconds。因此上文 0–4 映射推断不成立，下文 TLS 失败为早期尝试，已被后续只读成功结果替代。没有设置写入验收，相关功能未纳入当前产品。

2026-11-02：5141监听进程AcerQAAgent.exe，PID5780；服务AcerQAAgentSvis处于Running，路径 `C:\Windows\System32\AcerQAAgent.exe`，文件版本1.5.24。Authenticode状态Valid，发布者为Microsoft Windows Hardware Compatibility Publisher。EXE SHA256为 `e7bc1695abf4700712b40d23f7642fbef4ac9c8ea3bf13aef65d9062ac0cac4b`，未找到内嵌PEM证书文本。签名有效不等于网络端点证书已验证。

新增 `scripts/Read-QuickAccess.ps1`：固定本地WSS端点，沿官方AES-ECB/默认PKCS7握手构造，只发送FunctionQuery与固定Get。认证值在内存从本机资源读取，不写入日志；保留响应会话匹配、大小和时间限制。当前TLS握手报PartialChain，认证未完成、设置查询尚未发出、setter为0。证据在 `evidence/quickaccess-read.json`；不可把监听端口存在或进程退出之外的任何状态说成查询成功。

下一步定位官方端点证书来源，采用匹配的信任验证，不采用接受所有证书的回调或修改系统信任库来绕过该失败。尚无真实设置值或写回样本。沿此接口实现需要保留QuickAccess服务；是否能完全去除服务尚未确认。
