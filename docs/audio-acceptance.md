# 音频控制验收

2026-11-01，Predator PHN16-71 / BIOS V1.18，官方 PredatorSense 5.0.1341。来源与 GitHub 固定参考见 `audio-implementation-research.md`。

## 已实现路径

Rust 固定枚举覆盖音乐、电影、语音、策略、角色扮演、射击、自定义、自动。通过现有 Acer Agent 的固定 127.0.0.1:46933 协议读取能力/状态及发送音效 SET，保留 DTS 音效组件依赖。不存在任意 endpoint、Function、数值模式或原始 JSON 的前端接口。

Tauri → 同用户受限 helper → AudioClient。原始机型和 BIOS 由实际设备核对；未授权、未知当前模式、不支持的目标和不可用输出状态均不开放写入。界面先编辑草稿，点击应用后才 SET，并再次读取确认。

## 异步切换与恢复

首轮 `evidence/audio-music-first-restore.json` 保留了真实时序：服务 SET 回执为 `{"request":"SOUND_MODE","result":"0"}`，但立即 GET 仍返回旧模式。程序按当时规则报告未确认，没有把回执当成实际切换。随后 `audio-after-first-restore.json` 独立查询确认已经恢复电影。

修正后每次只发送一次 SET，最多四次独立 GET，间隔 100ms；全部尝试进入报告。GET/初始化的单次网络总时限为 750ms，SET 为 3 秒。超时或不匹配不能升级为成功。受控验证即使试验失败，也尝试恢复不可变的原模式；正式 UI 写入不暗中反复发送 SET。

## 本机模式往返

`evidence/audio-{music,voice,strategy,rpg,shooter,custom,automatic}-verified-restore.json` 七份记录全部 `outcome=confirmed`、`target_readback_confirmed=true`、`setter_acks_confirmed=true`、`original_state_restored=true`，原模式均为 movie。因此八个枚举的模式控制/读回已覆盖，结束时仍是电影。

探针 SHA256：`0286666850B810E00AC0C57E826F420EA4390577334189C7C7F795A0511C5C6C`。旧失败记录未被覆盖。模块纯测试覆盖真实格式、异步旧→新、始终旧/未知、SET 超时后恢复、恢复失败、非法模式及有界请求；它们补充而非替代这些实机证据。

本轮没有切换默认播放设备，没有主观听感验证。自定义选项只切换驱动已有预设，不提供未经证实的均衡器编辑。当前输出返回模式 9 的实机场景尚未遇到，仅按现有证据保留不可用态。

## 桌面验收

21:16 Release 构建完成，188 项工作区测试通过，前端 0 errors / 0 warnings。实际桌面看到八项下拉选项；将电影改选音乐时，当前读数仍保持电影，仅草稿变为音乐；点击应用后显示音乐且读回确认。随后按同样流程恢复电影，独立运行只读探针再次确认电影，记录于 `evidence/audio-after-desktop.json`。截图为 `evidence/release-audio-desktop.jpg`，构建清单为 `evidence/release-audio-manifest.json`。

console SHA256=`C777D4A2414DC8A94CA726DFF4824C2F5581A2B30F2B35426622327F75B8A448`；helper SHA256=`B0784676FB3686322F03DCA3CC8B14AB9CD4B35F374F7A60C209754E0B33494F`。

官方音效服务继续保留；本模块不代表已替代 DTS 信号处理或可以卸载音频驱动。
