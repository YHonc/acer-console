# 校准原生原语核验

2026-11-01。沿用 `battery-calibration-protocol.md`，不重复mask2研究。本次只读SDK、厂商PE和已安装Rust源码，并运行powercfg查询；未订阅事件、校准或修改电源计划。未改Rust/UI/既有文档。

## 1. 六项电源设置

来源简称：

- **A**：`C:\Windows\System32\AcerCCAgent.exe` 1.5.21，SHA256 `33dc36902e885d249f6bf69fc25bb98ade3678e701d1af46990c1c6b723ecbed`。
- **S**：`C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um\winnt.h`。

以下为Windows GUID字符串格式，不能把字符串顺序直接当作PE中的16字节顺序。A的指令以r8传subgroup、r9传setting；AC/DC使用相同GUID。

| subgroup简称 | GUID | A数据RVA | S行号 |
|---|---|---|---|
| B：电池 | `e73a048d-bf27-4f12-9731-8b2076e8891f` | `0x4F51C0` | 17049 `GUID_BATTERY_SUBGROUP` |
| H：睡眠 | `238c9fa8-0aad-41ed-83f4-97be242c8f20` | `0x4F5190` | 16757 `GUID_SLEEP_SUBGROUP` |
| V：显示 | `7516b95f-f776-4464-8c53-06167f40cc99` | `0x4F51B0` | 16570 `GUID_VIDEO_SUBGROUP` |

| Acer字段 | subgroup | setting GUID | A数据RVA / 文件偏移 | SDK符号或Windows名称 |
|---|---|---|---|---|
| CriticalAction | B | `637ea02f-bbcb-4015-8e2c-a1c7b9c0b546` | `0x4F53C8 / 0x4F45C8` | S:17061 `GUID_BATTERY_DISCHARGE_ACTION_0`；BATACTIONCRIT，关键级别电池操作 |
| CriticalLevel | B | `9a66d8d7-4ff7-4ef9-b5a2-5a326ca2a469` | `0x4F50A8 / 0x4F42A8` | S:17062 `GUID_BATTERY_DISCHARGE_LEVEL_0`；BATLEVELCRIT |
| ReserveLevel | B | `f3c5027d-cd16-4930-aa6b-90db844a8f00` | `0x4F5D10 / 0x4F4F10` | powercfg显示“保留电池电量”；本机SDK全部`.h`未找到对应GUID/符号 |
| HibernateAfter | H | `9d7815a6-7ee4-497e-8888-515a05f02364` | `0x4F4FB8 / 0x4F41B8` | S:16786 `GUID_HIBERNATE_TIMEOUT`；HIBERNATEIDLE |
| SleepAfter | H | `29f6c1db-86da-48c5-9fdb-f2b67b1f44da` | `0x4F4EE8 / 0x4F40E8` | S:16771 `GUID_STANDBY_TIMEOUT`；STANDBYIDLE |
| TurnOfDisplayAfter | V | `3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e` | `0x4F51A0 / 0x4F43A0` | S:16578 `GUID_VIDEO_POWERDOWN_TIMEOUT`；VIDEOIDLE |

**勿混淆：** ReserveLevel不是BATLEVELLOW（`8183ba9a-e910-48da-8769-14ae6dc1170a`）；CriticalAction也不是BATACTIONLOW。SDK使用`DISCHARGE_*_0`名称，Acer使用业务字段名；应按上述实参绑定，不按名称猜测。

调用位置已对照指令与IAT：A的`0x3B346A..0x3B36DF`用`PowerReadACValueIndex`/`PowerReadDCValueIndex`读取；`0x33CA90..0x33CB8D`写AC，`0x33C980..0x33CA7D`写DC，随后`PowerSetActiveScheme`。Reserve的四个r9加载点为`0x3B34DF`、`0x3B3600`、`0x33CAF5`、`0x33C9E5`，均指向`0x4F5D10`。数组六项顺序与表一致，仍须分别保存AC/DC，不能以某一组恢复两组。

本次只读快照：活动方案`48c2b03a-8e61-4837-a3ce-621f85306ee3`（Acer）。依表顺序，AC/DC索引分别为`2/2、5/5、7/7、43200/10800、0/7200、18000/7200`；后三项单位秒，两个level为百分数。powercfg显示action索引2为休眠。快照不是恢复默认值，执行时须重新读取并保存方案GUID及全部12值。

复现均只读：

```powershell
powercfg /aliases
powercfg /qh SCHEME_CURRENT SUB_BATTERY
powercfg /qh SCHEME_CURRENT SUB_SLEEP
powercfg /qh SCHEME_CURRENT SUB_VIDEO
```

静态GUID复现：使用现有`tools/reverse/venv/Scripts/python.exe`，以`pefile.PE(A).get_data(RVA,16)`读取表中位置，再用`uuid.UUID(bytes_le=...)`解释；Reserve原始字节为`7d02c5f316cd3049aa6b90db844a8f00`。此过程不加载执行A。

## 2. wmi 0.18.4订阅与停止

源码根 **W**：`%USERPROFILE%\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\wmi-0.18.4\`。

| 原语 | 实际源码事实 |
|---|---|
| 同步订阅 | `src/notification.rs:20..36`的`exec_notification_query`返回迭代器；`src/result_enumerator.rs:209..223`的`next()`调用`IEnumWbemClassObject::Next(WBEM_INFINITE,...)`。公开接口没有Next超时参数。 |
| WITHIN | `src/query.rs:154..161`只生成WQL `WITHIN`，不是等待超时或取消令牌。不能用它保证停止响应。 |
| 异步订阅 | `src/notification.rs:150..180`的`exec_notification_query_async`持有sink及连接clone；`:207..220`的`async_raw_notification<T>`提供反序列化流，无需Tokio。 |
| 停止清理 | `src/query_sink.rs:70..73`在流Drop中调用`CancelAsyncCall(&sink)`，但忽略HRESULT；没有公开的可报告结果`cancel()`。`connection.rs:99`的svc是pub(crate)，外部不能直接调用它。 |
| 线程 | `connection.rs:95..100`明确使连接!Send；`:85..88`说明必要时自动初始化MTA，但Drop不反初始化COM。连接和流应留在同一拥有线程。 |

**最小建议**：独立常驻线程创建ROOT/WMI连接，订阅固定`SELECT EventDetail FROM APGeEvent`。以`futures::channel::mpsc`有界停止通道，与流事件竞争；使用本线程`futures::executor::block_on`，不迁入多线程执行器。W的Cargo.toml:95..96已经依赖futures 0.3；实现项目仍需声明自己的直接依赖。

```text
worker创建con，events = Box::pin(con.async_raw_notification::<Event>(固定WQL)?)
block_on循环：select_biased! {
  stop_rx.next().fuse() => break（含sender关闭）,
  events.next().fuse() => 转发事件；Err/None报告监听异常并退出
}
drop(events)   // 拥有流的Box；触发CancelAsyncCall
drop(con)
通知helper线程已经退出清理
```

这里停止通道能唤醒空闲异步等待；只丢弃一次`.next()` future或Pin引用并不会取消持有的订阅。应由拥有线程drop整个流，再join；不要用无限阻塞同步next，或仅在next前检查AtomicBool。以上为未编译方案，未建立真实订阅。

**界限**：Drop中的COM取消本身没有源码级超时保证，也不报告成功；不能声称停止必在固定毫秒内完成。若验收要求取消HRESULT或硬性截止，需原生持有IWbemServices/sink或修改上游接口，不能靠当前公开API虚构。

另两项实装注意：`query_sink.rs:174..180`追加事件、`:118`却`pop_back()`，积压/批量事件不保证FIFO；`:185..200`忽略SetStatus的HRESULT。因此流结束不是校准完成，事件顺序与监听错误需另核验。若要附带`TIME_CREATED`用于排序，追加查询字段及设备实际返回仍待验证；状态机/journal取舍由主代理处理。
