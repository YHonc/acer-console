# 退出恢复临时风扇 API

模块：apps/desktop/src-tauri/src/exit_restore.rs，主代理在 main 声明 mod 并 manage(ExitState::default())。ExitState 可 clone，共享 receipt、allow_exit 和 in_progress。默认关闭，不持久化，不自动提权、退出或操作窗口。

## 接入顺序

1. 用户显式打开“退出恢复”时调用 set_enabled(true, Some(&最新真实快照))；会验证原风扇为 Auto/Max，或 Custom 且 CPU/GPU 百分比均在 0..100。原 Custom 缺任一值时拒绝开启。若已经有首次写入 receipt，则以该原状态校验，不用当前状态替换原值。关闭调用 set_enabled(false, None)，保留原 receipt。
2. 每次实际 fan apply 返回后，调用 record(&command, &report)，包括 Failed/Unconfirmed。仅 Command::SetFanMode/SetFanSpeeds 且 report.writes 中出现固定风扇 setter、且状态不是 Rejected 才视为已尝试写入。拒绝或无 writes 不记录。不凭 Applied 字样猜测有无写入。
3. 首次尝试的 before 决定整个进程内的恢复点：Auto/Max 存 SetFanMode，Custom 存精确双百分比 SetFanSpeeds。缺 before 或缺原 Custom 数据时存不可恢复原因，后续报告不能覆盖，避免误把已经改变后的状态当作原状态。无论开关当时是否启用都会捕获首次实际写入，用户后来开启恢复也不会丢失原状态。
4. 主代理处理明确退出时先读取 pending()。allow_exit=true 可以继续既有退出流程；in_progress=true 不再启动第二次恢复。未开启或无 receipt 时无需设备操作；可以调用 restore() 获得 NotNeeded 并设置允许退出，或由主桥接直接走非恢复退出分支。
5. enabled 且有 receipt 时，主桥接阻止当前退出并在 blocking 工作线程调用 restore(&已有 BrokerClient)。只通过该 client.apply 固定恢复 Command，仍由 native Verified 策略与读回判断。没有 helper 时明确报错和提供授权入口，不在此模块自动弹 UAC；不要在 restore 之前重新调用 record 记录恢复动作。
6. 只有返回 Restored（ApplyStatus::Applied）才清除 receipt 并 allow_exit=true；失败、Rejected、Unconfirmed 或传输失败均保留 receipt、清除进行中标记并禁止静默退出，供用户显式重试。超时可能已经写入，先检查实际快照；本模块从不自动重试。
7. 用户明确选择“跳过恢复并退出”时调用 permit_exit_without_restore()，保留 receipt 并 allow_exit=true；恢复进行中不允许跳过。随后 app.exit/窗口行为由主代理统一执行。本模块不处理异常终止、关机强杀或进程崩溃恢复。

## 并发边界

restore 仅在准备/收尾时持内部 mutex，不跨 broker RPC 持锁；重复退出请求得到 InProgress。主桥接必须在退出进行中暂停新的风扇写入，并串行化风扇 apply→record 与退出入口，保证一次已发出的风扇写入在其报告登记前不会被判定为“没有 receipt”。已获得的 allow_exit 也不能与新风扇写入并行。模块不会替代主桥接的设备队列或退出事件契约。

已提供 5 个纯测试：首次 before 保留、拒绝不记、自定义双百分比、失败/未确认保留、不可恢复原状态不被覆盖、单次恢复及关闭时不写。待主代理将模块加入 main 后统一运行测试；本对话不另建 crate、不运行硬件恢复或操控窗口。
