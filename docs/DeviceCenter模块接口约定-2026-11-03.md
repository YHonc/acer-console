# Device Center文件夹模块接口约定

用户已选择完整独立功能窗口，不将外设协议合并到Acer进程。模块为可移动文件夹；Acer仅读取清单、校验、管理入口/启动项/状态，原应用运行全部已有功能。

## 清单v1

文件名`module.json`，结构如下，实际发行时填写真实SHA256：

```json
{"schema_version":1,"id":"device-center","name":"Device Center","version":"0.1.1","entry":"DeviceCenter.exe","files":[{"path":"DeviceCenter.exe","sha256":"64位十六进制"},{"path":"WebView2Loader.dll","sha256":"64位十六进制"}]}
```

- 仅接受本模块id及schema_version=1；entry为受支持主程序文件名，禁止命令参数、脚本、绝对/上级路径。runtime files必须包含entry且去重。
- 路径经canonicalize后必须仍在所选模块根内，校验运行文件sha256。清单哈希保存在本控制台绑定中；文件或清单改变需重绑。哈希表示一致性，不等于发布者签名信任。
- 选择文件夹不执行程序；启动必须使用已有显式确认流程，普通用户权限，不经过Acer提权helper。
- 保留原EXE绑定兼容。新Binding模块字段可缺省，旧配置仍可读取。

## 宿主能力

新增固定目录选择命令`bind_device_center_module`，其余查询/启动沿用现有受限接口。查询返回可选`module`：`{root,id,name,version}`；验证错误仍使用`binding_error`。

新增`detach`动作仅删除本控制台绑定文件，保留程序与账号数据。若绑定进程仍在运行或本控制台自启动项存在，拒绝解除并提示先退出/关闭自启动。模块丢失时仍可在关闭自启动后解除失效绑定。

默认不删除任何外部配置或源项目。用户尚未选择自动移动到待删除/纯手动删除时，只提供解除绑定和准确目录，供用户自主删除；后续明确授权后再增加删除程序选项。

## 构建与发行

模块目录`modules/device-center/`包含完整独立EXE、运行所需DLL、原许可证/第三方说明、对应源码归档及module.json。宿主包可附该目录，删除它后宿主仍能运行并显示模块缺失。不得复制原项目.tools、node_modules、.git、用户账号配置到发行包。模块启动/硬件功能由用户实测，构建与离线检查不等于设备验收。
