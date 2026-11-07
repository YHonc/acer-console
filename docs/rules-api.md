# AC/DC 规则与唤醒 API

crate：acer-rules，仅纯状态机，无线程、WMI、进程启动、文件或提权调用。主代理负责加入 workspace 与桌面/core；构造不会执行任何命令。

## 最小接入

1. 保存/读取 `RuleConfig { enabled, ac_profile: Option<Profile>, dc_profile: Option<Profile> }`，默认 enabled=false、两目标为空。由用户显式开启；reconfigure 重置基线并建议重读，不立即套用当前电源目标。
2. native 持有一个 RuleEngine，串行接收新完成的真实快照，调用 `observe_snapshot(&snapshot, &capabilities, &verified_targets)`。电源事件可调用 observe(Observation)，power 未知传 None。不要把旧缓存当作唤醒后的新观测。
3. `VerifiedTargets { ac, dc }` 必须来自 native 已验收的机型和 AC/DC 模式限制，不能来自用户配置或 WebView JSON。默认空列表阻止所有模式；同时必须 capabilities.matched_model=true，profile 项 writable=true 且 status=Verified。当前 helper 的 Auto/Max 写入验收不等于性能模式已获授权；未验证 profile 时规则保持 CapabilityBlocked。
4. Decision.action 只有确认从已知 AC 到已知 DC（或反向）且能力允许时才包含 `RuleAction { id, power, target, command: Command::SetProfile }`。首次读取只建基线；未知电源中断基线，下一次已知值也只建基线。已是目标模式则不发命令。
5. core 在实际派发前检查 `is_current(id)`，再检查最新 capability/电源/目标限制；通过原有串行设备操作队列派发 command，使用实际 ApplyReport 映射到 finish(id, Applied/Failed/Unconfirmed)。规则不会自己调用设备。失败、未确认均消费本次动作，不自动重试。
6. 电源在操作进行中再次切换时返回 Busy，不缓存待执行命令；该次转换被消费。尚未派发的旧动作 is_current=false，core 应 finish 为 Unconfirmed/Failed 并读快照；已经派发的请求无法靠此状态机撤销，返回后仍须确认实际状态。不要在同一快照上循环调用试图重试。
7. 收到系统 resume/wake 通知调用 on_wake()，只设置 refresh_requested=true、清空基线并使旧动作失效。core 随后刷新电源、capabilities 和快照；第一次真实已知值重新建基线，不盲目重放休眠前写入。底层 WMI/helper 的重连仍由主设备层负责，不自动请求 UAC。

## 观测与错误

Reason 表示 Disabled、Baseline、UnknownPower、Unchanged、NoTarget、CapabilityBlocked、TargetBlocked、AlreadySelected、Busy、Emit、WakeRefresh、Reconfigured 或 IdExhausted，可供日志和 UI 说明。last_completion() 保留最近一次被接受的完成结果；迟到/重复的 finish 返回 false。只读规则状态不等于硬件状态，必须继续用真实快照显示实际模式。

ID 单调递增，唤醒/改配置不会复用旧 ID。规则只做转换判定，不实现计时重试或连续写入。core 必须按事件顺序送入新观测，合并重复系统电源通知，并在执行队列仍有硬件请求时保持串行，不向状态机并发注入旧快照。当前未增加延时去抖，快速而真实的多次插拔在动作完成后可各产生一次新转换。

## 验证范围

新增 7 个纯逻辑测试覆盖默认关闭、首次基线、重复观测、失败不重试、未知状态、能力及 AC/DC 目标限制、唤醒/改配置、进行中转换、已是目标、空目标和 ID 耗尽。尚待主代理安排 Cargo 编译测试；不并发争用当前构建锁。没有修改 apps/根 Cargo，没有做电源切换或硬件写入实验。真实休眠事件接入和模式控制验收仍由主代理负责。
