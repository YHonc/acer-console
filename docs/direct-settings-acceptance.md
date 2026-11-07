# 三项硬件设置的直接 WMI 验收

日期：2026-11-01。机型 Predator PHN16-71、BIOS V1.18。本机官方 5.0.1341 反汇编常量已由主对话和独立只读审查核对，详见 `reverse-continuation.md`。

## 原始只读样本

`evidence/direct-settings-read.json` 是普通桌面发起的管理员只读探针结果。三个固定 GET 均没有 `ReturnValue`，仅返回对应无符号输出字段，调用错误为空：

| 请求 | 原始输出 | 解码 |
|---|---:|---|
| AcerGamingFunction.GetGamingProfile(0) | 281474993487872 | LCD 加速开启 |
| AcerGamingFunction.GetGamingMiscSetting(6) | 256 | 启动声音开启 |
| APGeAction.GetFunction(0x88401) | 32985349357568 | 超时 30 秒开启，亮度 0 |

初版探针将上述 ACK 标为 unknown，并未执行 SET。审查本机样本与官方调用后，仅为这三个固定请求、单一输出字段、结构可解码值添加缺 ReturnValue 兼容。

## 第一次直接写入与恢复

`evidence/direct-lcd_overdrive-restore.json`、`direct-boot_sound-restore.json`、`direct-backlight_timeout-restore.json`：分别关闭再恢复开启，目标与原值两次独立 GET 均确认。背光亮度始终为 0，未使用官方查询失败时回退 100 的行为。

三项 SET 都返回单一无符号输出 0，没有 ReturnValue。因为当时 setter 规则尚未收录该格式，原记录诚实保留 `readbacks_confirmed_acks_unknown`、`status=unconfirmed` 和退出码 1；同时 `original_state_restored=true`。这些原始记录不被后续兼容规则覆盖。

后续只允许与这些固定请求及输出一致的 ACK 规则，要求独立 GET；显式失败、未知字段、非法 selector 和不明亮度仍阻止写入或确认。实际重启声音、屏幕响应时间和 30 秒背光物理行为另列物理验收，不能由寄存器读回代替。

## 集成状态

已实现 `acer-controls::direct_settings` 串行 COM worker，broker 已切换至该固定接口；没有自动回退至官方 Agent。粘滞键单独由当前桌面用户 Win32 接口控制。

20:39 构建的 `artifacts/AcerConsole` 完成实际桌面验收：在已授权的 Release 内逐项点击 LCD、启动声音、背光超时关闭，再分别恢复开启。六次界面均经后端 ACK 与独立 GET 返回“设备读回一致”；最后三项均开启，静态灯光仍为亮度 0、四区 #00AEC7。截图 `evidence/release-direct-settings-desktop.jpg`，对应构建清单 `evidence/release-direct-ui-manifest.json`。console SHA256=`547E86AA2ABE595657B31A18827F7B4DF0310985B3F679094D8AE9F818302E13`，helper SHA256=`62FAC8E3EC2E40C8E1B020C368215F9DF9EEF374CCB2318876C2960B89EA1408`。

完整构建通过 171 项工作区测试，前端 0 errors / 0 warnings。灯光统一颜色控件恢复为紧凑色块，已实际查看；同时发现无恢复记录时被泛化为“尚待验证”，后续仅修正该提示文案，不改变硬件控制路径。

当前官方启动任务仍启用，音频等功能仍在补齐，不能因此停止所有官方服务。

20:52 提示文案修正版已重新构建并替换包内 console：SHA256=`49362A8FEA39871D81B6F981ED28A96B7BA3F1EAE5D7EB1E23E2B5816D11EFAB`，helper 未变。已重新启动、授权并实际查看灯光页，确认紧凑色块与“本次连接尚无可恢复的灯光记录”提示；截图 `evidence/release-lighting-layout-final.jpg`。此次只改能力提示文案，没有重复执行硬件往返。
