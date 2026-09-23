# easyJob 系统级开机自启（用户未登录可运行）设计规范

## 1. 概述与背景

现状：`tauri-plugin-autostart` 只实现登录后自启。Windows 写 `HKCU\...\Run`，macOS 用用户级 `LaunchAgent`。Agent 数据与 IPC 都是按用户隔离的（`~/.easyjob`、按用户名区分的管道、用户目录下的 socket）。

目标：新增真正的开机自启。用户未登录时任务也照常执行。系统设置中的自启开关改为三选一。数据统一到一份系统级目录，不再保留 `~/.easyjob`。

范围：只做 macOS 与 Windows。不做 Linux。任务计划程序仍只被禁止用于业务调度；Windows 开机载体使用 Windows Service（此前规范里对任务计划程序的禁令依然有效，只是不再把 Service 视为禁用项）。

非目标：自动迁移 `~/.easyjob` 老数据；多用户隔离；Session 0 / 无会话 GUI 交互；Linux systemd。

## 2. 启动模式定义

三种模式互斥，设置页用 `NRadioGroup` 呈现：

1. `开机自启`：机器启动即运行，用户未登录也执行任务。需要管理员权限安装。Windows 以 `SYSTEM` 身份运行，macOS 以 `root` 身份运行。
2. `登录后自启`：沿用现状登录启动项。用户登录后启动桌面端并拉起 Agent。Agent 同样使用系统级数据目录。
3. `不自启`：不写任何启动项。两种载体都不存在。

默认与迁移：新安装默认 `登录后自启`（延续旧版默认开启行为）。升级迁移时读取旧状态：旧版已启用登录自启则映射为 `登录后自启`，否则映射为 `不自启`。`开机自启`永不作为默认或迁移结果，只能由用户显式选择。

互斥规则：开机模式要求系统服务存在且运行，同时登录启动项必须清除。登录模式要求登录启动项存在，同时系统服务必须不存在。禁用要求两者都不存在。切换模式时先建新侧再清旧侧，失败则回滚到原模式并报错。

## 3. 架构总览

保留单个 `easyjob-agent` 二进制，两种运行上下文，共用同一份系统库：

```text
开机自启：  系统 boot -> Windows Service / LaunchDaemon -> easyjob-agent --daemon --data-dir <系统目录>
登录后自启：用户登录 -> 登录启动项 -> 桌面端（--minimized）-> easyjob-agent --daemon --data-dir <系统目录>
禁用：      无启动项，桌面端手动启动时按需拉起 Agent（仍用系统目录）
```

关键不变量：同一时刻只允许一个 Agent 写库，以系统目录下的 `agent.lock` 保证。开机模式下桌面端只是 IPC 客户端，绝不 spawn Agent。登录或禁用模式下桌面端连不上 Agent 时才 spawn，且必须带 `--data-dir <系统目录>`。

桌面端退出时行为区分归属：只有本会话内由桌面端 spawn 的 Agent 才允许在退出时 shutdown。开机模式下关闭桌面端绝不关闭系统服务。

## 4. 系统数据目录与权限

统一规则：代码不再读取 `~/.easyjob`。旧文件留在磁盘不动，首次切换到开机模式时 UI 提示可用导出导入手动迁移或直接丢弃。

- Windows：`<安装目录>\data`（例如 `C:\Program Files\easyJob\data`）。安装时若安装包能力允许则直接建目录并放开 ACL；否则首次切换模式时提权修复。ACL 目标：`SYSTEM` 与 `Authenticated Users` 可读写（SQLite 需要写与建临时文件）。DB 路径 `<data>\easyjob.db`，锁 `<data>\agent.lock`，日志 `<data>\logs\`。
- macOS：固定 `/Library/Application Support/EasyJob`。提权创建并放开给登录用户可读写（目录归 `root`，通过 ACL 或组权限保证登录用户可写）。DB、锁、日志与 socket 都在其下。
- 解析规则：Windows 以服务安装时写入的 `--data-dir` 为准，桌面端按可执行文件所在目录推导同一路径，两者不一致时以服务注册值为准并报错提示重装。macOS 直接用固定路径。
- Windows NSIS 安装包使用 `perMachine` 安装模式，并通过 `installerHooks` 在卸载前询问是否保留 `<安装目录>\data`；若使用 MSI 或其他安装包，卸载默认保留，文档写明手动删除路径。

## 5. 系统 IPC 通道

`default_ipc_path` 改为系统级固定通道，不再按 `USERNAME` 或 `HOME` 拼路径：

- Windows：固定 `\\.\pipe\easyjob-system`。服务进程建管道时显式放行 `Authenticated Users` 连接。
- macOS：`/Library/Application Support/EasyJob/easyjob.sock`。daemon 建 socket 后保证登录用户可读写。
- 测试与开发可用显式 IPC 路径覆盖。macOS 的 socket 跟随 `--data-dir`。Windows 的管道名固定，不跟随 `--data-dir` 变化。产品行为以固定通道为准。
- 安全边界：本机通道，不做跨机认证。连接被拒、服务未运行、DB 被锁都要映射为中文错误，桌面端明确提示而不是静默另起 Agent。

## 6. Windows Service 设计

Agent 新增服务形态：

- 新增 `--service` 入口，处理 SCM 的启动与停止回调。停止时走现有优雅关闭语义。
- 新增特权子命令：`--install-service --data-dir <dir>` 与 `--uninstall-service`。安装内容：服务名 `easyJobAgent`，显示名 `easyJob Agent`，启动类型自动，以 `LocalSystem` 运行，`ImagePath` 为 `"...easyjob-agent.exe" --service --data-dir "<...>\data"`。同时配置失败自动重启（例如 5 秒后重启）。
- 桌面端本身不提权常驻。切换到开机模式时，桌面端用 `runas` 方式重跑 `easyjob-agent --install-service ...` 触发 UAC。卸载服务同理。
- 卸载：安装包卸载时弹窗询问是否保留 `data`，默认保留。保留则删除服务注册与程序文件，保留 `data`。不保留则连服务带 `data` 一并删除。

## 7. macOS LaunchDaemon 设计

- plist 路径：`/Library/LaunchDaemons/com.easyjob.agent.plist`，归 `root:wheel`，权限 `644`。
- 内容：`Label` 为 `com.easyjob.agent`，`RunAtLoad=true`，`KeepAlive=true`，`ProgramArguments` 为 agent 全路径加 `--daemon --data-dir /Library/Application Support/EasyJob`，标准输出与错误重定向到系统数据目录下的 `logs/daemon.out.log` 与 `logs/daemon.err.log`。
- 安装：设置页选择开机模式后，桌面端弹系统密码框提权（`osascript ... with administrator privileges`），先创建 `/Library/Application Support/EasyJob` 与 `logs` 并授予 `staff` 组读写，再写入 plist 并执行 `launchctl bootstrap system ...`。停用时执行 `bootout system ...` 并删除 plist。
- 数据默认保留。macOS 没有统一卸载器，停用开机自启即卸载 daemon，数据目录不动，文档说明手动删除路径。

## 8. 桌面端改动

后端（`apps/desktop/src-tauri`）：

- 新增 Tauri 命令：`service_status`、`service_install`、`service_uninstall`、`prepare_data_dir`。前端 `startupMode.ts` 编排三态切换与回滚；登录启动项继续用 `tauri-plugin-autostart`。
- `AgentManager` 改为连接系统级 IPC。登录或禁用模式连不上才 spawn（带 `--data-dir`）。开机模式连不上只报错，绝不 spawn。记录本会话是否由桌面端 spawn，用于退出时决定是否 shutdown。
- `ExitRequested`：仅当本会话 spawn 过 Agent 才 shutdown，否则只关闭窗口不断开服务。

前端：

- `SettingsView.vue` 的 `NSwitch` 换成 `NRadioGroup`，三项各带一句话说明，开机项注明需要管理员权限以及未登录以 `SYSTEM`/`root` 运行。
- 新增 `apps/desktop/src/services/startupMode.ts`（或在现有 `autostart.ts` 上扩展），封装模式查询、切换、确认框、失败回滚。旧 `localStorage` 初始化键保留用于一次性迁移映射，迁移后不再作为状态来源。
- 切换到开机模式先弹确认框，说明提权与运行身份。UAC 或密码框取消视为失败，回滚选项并提示。

## 9. 切换流程

- 切到开机：确认框 -> 提权安装服务或 daemon -> 等待运行 -> 清登录启动项 -> 切模式。任一步失败回滚并保留原模式。
- 切到登录：停服务并删除服务注册或 `bootout` 加删 plist -> 写登录启动项 -> 切模式。
- 切到禁用：两边都清。桌面端当前会话的 Agent 归属不变，避免正在执行的任务被误杀；仅阻止下次自动启动。

## 10. 错误处理

全部转为中文提示并回滚选项：用户取消提权；服务安装成功但启动失败；管道或 socket 连接被拒；系统目录不可写；DB 被锁；服务注册值与桌面端推导路径不一致。开机模式下桌面端连不上服务时提示检查系统服务状态，不得另起第二个 Agent。

已知环境差异必须在文档与确认框中写明：系统身份运行的任务拿不到用户 `PATH`、用户级环境变量、网络驱动器与 GUI 会话能力。任务失败按既有 `MissedRunPolicy` 与重启恢复语义处理，不新增语义。

## 11. 测试与验证

- Rust 单测：系统数据目录解析、系统 IPC 路径、Windows 服务命令行拼装、macOS plist 内容生成、提权命令拼装、模式状态机互斥规则。更新现有 IPC 路径相关测试。
- 前端 `vitest`：三选一状态、切换失败回滚、提权取消回滚、开机模式连接失败不 spawn。
- 全量：`cargo test` 与桌面端 `vitest` 通过。CI 内不真实安装服务或 daemon。
- 手工：Windows 关机重启后不登录，确认任务照常执行，登录后桌面端能看到未登录期间的执行记录；macOS 同理；Windows 卸载时确认保留数据的弹窗行为（若安装包不支持弹窗则确认默认保留加文档说明）。

## 12. 打包与文档更新

- release 流程不变。Windows 安装包若支持则加入 `data` 目录创建、ACL 与卸载保留提示；不支持则运行时提权修复兜底。
- 更新 README 与旧版自启 spec 中过时的表述：登录自启保留 `HKCU Run` 与 `LaunchAgent` 描述；新增开机模式的 Service 与 LaunchDaemon 描述；写明 `~/.easyjob` 不再使用以及手动迁移方式。
