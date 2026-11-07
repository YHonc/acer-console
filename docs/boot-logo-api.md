# 开机 Logo 接口与验收边界

2026-11-01，目标仅 PHN16-71 / BIOS V1.18。

## 原生事务

普通 `Targets::inspect` 可用于读取/准备，`Plan::execute()` 仍拒绝执行。正式入口是 `native_transaction::NativePlan`：只能由内部重新发现机型、BIOS、唯一 ESP/FAT32/EFI GPT 分区及 KnownFolder 路径后构造，外部不能传入 Discovery 或目标目录来取得写入能力。

- `inspect_current()`：锁定现有目录并读取四槽，不创建目录。
- `prepare_image(path)`、`prepare_factory_reset()`、`prepare_backup_restore(id)`：准备新图、移除自定义覆盖、恢复指定备份。
- `NativePlan::preview()`：固定目标、修改前后 SHA256/存在状态、备份目录。
- `NativePlan::execute()`：消费计划一次，执行并逐槽读回；失败后尝试四槽回滚，分别保留结果。
- `list_backups()`：只读列出备份和事务状态，中断的 applying 记录不会伪装为已完成。

四槽仅为 ESP/cache 的 AcerLogo.gif 和 AcerLogo.jpg。不修改启动加载器、BCD 或固件镜像，不调用官方带删除缓存副作用的 Logo getter，也不自动重启。

## 文件与备份

Windows 守卫逐级无跟随 reparse 打开祖先及目标目录，持有不共享删除的句柄直到取消、过期或执行结束。文件读取检查打开后的句柄，禁止跟随文件 reparse，并在读取期间禁止并发写删。缺失目录在父目录锁下创建。

备份固定放在 ProgramData/AcerConsole/BootLogoBackups/<id>，保存原字节、缺失状态及 SHA256。每个文件写入并 sync，最后原子发布 manifest；binding 另外绑定机型、BIOS、ESP 卷和缓存卷路径。恢复拒绝缺 binding 或目标不匹配的备份。

事务先持久化 applying 日志，再提交四槽。每个文件用同目录独占临时文件和 MoveFileExW(REPLACE_EXISTING|WRITE_THROUGH)，不先删除目的文件；预先存在的临时文件不会被清理。结束记录 applied、rolled_back 或 recovery_required，日志写入失败单独返回。

四文件不是整体原子替换。进程/电源中断可能留下部分修改，须用原始备份显式恢复；不声称抗掉电验收完成。目录锁不阻止其他程序直接改写目录中的现有文件，因此提交前比较原始快照、每步读回，冲突不当作成功。

## 图片验证

image 0.25.10 仅启用 GIF/JPEG。JPEG 完整解码，GIF 逐帧解码释放，后续坏帧也拒绝。新图输入最多 35,000,000 字节，宽高分别不超过当前显示像素的40%。额外应用预算为4096帧、累计256,000,000画布像素、64MiB输出/画布预算；这不是厂商声明的帧数限制，也不保证涵盖解码器全部内部临时分配。原编码字节不重编码。

## 受限 helper

helper 保存不可序列化的 NativePlan。计划 ID 仅引用当前认证会话内对象；错 ID 不消费正确计划，正确提交仅执行一次，五分钟过期释放句柄。原生文件选择器产生 source，WebView 不接受任意源文件准备命令或目标/manifest/raw frame。

准备/提交预算为服务端120秒、客户端125秒，其余仍为15/20秒，调用方不能覆盖。取消不提交目标图片修改，准备时生成的备份保留。

## 验证

14项 crate 测试通过：完整解码及第二帧损坏、尺寸/预算、临时目录应用与备份恢复、不同绑定拒绝、目标变化、日志失败、回滚失败、祖先junction拒绝、目录重命名锁定等。broker八项计划生命周期/结果测试通过。这些只在临时目录或纯内存执行。

`evidence/boot-logo-locked-read.json` 使用新守卫在本机只读成功：1920×1080，上限768×432；ESP/cache GIF均存在且SHA256相同，JPG均不存在，与之前基线一致。

真实EFI图片提交、实际恢复、重启显示及最终桌面流程仍待验收，不能由隔离测试或只读发现替代。

