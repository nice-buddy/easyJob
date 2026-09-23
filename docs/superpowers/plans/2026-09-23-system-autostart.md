# 系统级开机自启 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现用户未登录也可运行任务的系统级开机自启，设置页提供开机自启 / 登录后自启 / 不自启三选一，数据统一到系统级目录。

**Architecture:** 单个 `easyjob-agent` 二进制新增 `--service` / `--install-service` / `--uninstall-service` 形态；Windows 用 Windows Service（SYSTEM），macOS 用 LaunchDaemon（root）；IPC 与数据目录收敛到系统级固定位置；桌面端在开机模式下只做 IPC 客户端，退出时不关闭系统服务。

**Tech Stack:** Rust 2021 (tokio, clap, serde), `windows-service` (仅 Windows), `sc.exe` / `icacls` / `launchctl` / `osascript` 系统工具调用，Tauri 2 命令，Vue 3 + Naive UI (`NRadioGroup`)，vitest。

**Spec:** [docs/superpowers/specs/2026-09-23-system-autostart-design.md](../specs/2026-09-23-system-autostart-design.md)

## Global Constraints

- 只支持 macOS 与 Windows。不做 Linux（`#[cfg(target_os = "linux")]` 路径保持现状行为，不得引入 systemd 逻辑）。
- 任务计划程序仍禁止用于业务调度；Windows 开机载体只允许 Windows Service。
- 代码不再读取 `~/.easyjob`；不做老数据自动迁移；旧文件留在磁盘不动。
- 开机模式下桌面端绝不 spawn Agent；退出桌面端绝不关闭系统服务。
- 同一时刻只允许一个 Agent 写库，以系统目录下的 `agent.lock` 保证（沿用现有 `SingleInstanceLock`）。
- 所有面向用户的错误文案为中文；切换失败必须回滚选项。
- CI 内不真实安装 Windows Service / LaunchDaemon。

---

## File Structure

- 修改 `crates/ipc/src/transport.rs`：系统级 IPC 路径（Windows 固定管道名，Unix 跟随 data dir）。被 agent 与桌面端共用。
- 修改 `crates/ipc/src/server.rs`：系统通道建 socket/管道时放行登录用户连接（Unix socket 权限放宽，Windows 命名管道加显式 ACL —— 若 `ServerOptions` 不支持 ACL 则记录并由安装时 `icacls` 兜底，行为以测试为准）。
- 扩展 `crates/platform`：新增 `src/startup.rs`（`StartupMode` 枚举、系统数据目录解析、Windows Service 参数拼装、`sc query` 状态解析、macOS plist 内容生成、提权命令拼装，纯函数优先以便跨平台单测）。`src/lib.rs` 导出。
- 修改 `apps/agent/src/main.rs`：CLI 新增 `--service` / `--install-service` / `--uninstall-service`；默认 data dir 改为系统目录；Windows Service 控制循环。
- 修改 `apps/desktop/src-tauri`：新增 `src/startup.rs`（`startup_get_mode` / `startup_set_mode` 命令、提权执行、登录项管理调用），`agent_manager.rs` 改为系统 IPC + 归属跟踪，`lib.rs` 接线与退出语义，`Cargo.toml` 加 `easyjob-platform` 依赖。
- 前端：新增/扩展 `apps/desktop/src/services/startupMode.ts`，`SettingsView.vue` 开关换三选一，`App.vue` 迁移旧初始化键。
- 文档：`README.md` 更新，旧 spec 顶部加 supersede 说明。
- 测试：`crates/ipc/tests/transport_tests.rs` 更新；`crates/platform/tests/startup_tests.rs` 新增；`apps/desktop/tests/startupMode.test.ts` 新增。

---

### Task 1: 系统级 IPC 路径

**Files:**
- Modify: `crates/ipc/src/transport.rs`
- Test: `crates/ipc/tests/transport_tests.rs` (`test_default_ipc_path` 更新 + 新增用例)

**Interfaces:**
- Consumes: 无（`std::path::{Path, PathBuf}`）。
- Produces: `pub fn system_ipc_path() -> PathBuf`；`pub fn ipc_path_for_data_dir(data_dir: &Path) -> PathBuf`；`default_ipc_path()` 行为改为返回系统级路径（保持签名兼容调用方）。

- [ ] **Step 1: Write the failing test**

```rust
// crates/ipc/tests/transport_tests.rs
use easyjob_ipc::transport::{ipc_path_for_data_dir, system_ipc_path};

#[test]
fn test_system_ipc_path_is_fixed() {
    let a = system_ipc_path();
    let b = system_ipc_path();
    assert_eq!(a, b);
    #[cfg(windows)]
    assert_eq!(a.to_string_lossy(), r"\\.\pipe\easyjob-system");
}

#[test]
fn test_ipc_path_for_data_dir_unix_follows_dir() {
    #[cfg(unix)]
    {
        let dir = std::path::Path::new("/Library/Application Support/EasyJob");
        assert_eq!(
            ipc_path_for_data_dir(dir),
            dir.join("easyjob.sock")
        );
    }
}

#[test]
fn test_ipc_path_for_data_dir_windows_ignores_dir() {
    #[cfg(windows)]
    {
        let dir = std::path::Path::new(r"C:\Program Files\easyJob\data");
        assert_eq!(
            ipc_path_for_data_dir(dir).to_string_lossy(),
            r"\\.\pipe\easyjob-system"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p easyjob-ipc --test transport_tests test_system_ipc_path_is_fixed`
Expected: FAIL with "unresolved import" / "function not found".

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/ipc/src/transport.rs
use std::path::{Path, PathBuf};

pub const MAX_FRAME_LENGTH: usize = 4 * 1024 * 1024; // 4MB

/// Windows: fixed system pipe, shared by service and all desktop sessions.
/// macOS: socket under the fixed system data dir.
pub fn system_ipc_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(r"\\.\pipe\easyjob-system")
    }
    #[cfg(not(target_os = "windows"))]
    {
        system_data_dir_fallback().join("easyjob.sock")
    }
}

/// Unix socket follows the data dir. Windows pipe name is fixed and ignores `data_dir`.
pub fn ipc_path_for_data_dir(data_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let _ = data_dir;
        PathBuf::from(r"\\.\pipe\easyjob-system")
    }
    #[cfg(not(target_os = "windows"))]
    {
        data_dir.join("easyjob.sock")
    }
}

#[cfg(not(target_os = "windows"))]
fn system_data_dir_fallback() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/EasyJob")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".easyjob")
    }
}

/// Back-compat entry point: now resolves to the system channel.
pub fn default_ipc_path() -> PathBuf {
    system_ipc_path()
}
```

同时更新 `test_default_ipc_path` 旧断言：unix 改为断言以后缀 `EasyJob/easyjob.sock` 结尾（macOS）——注意该测试在 macOS CI 跑，旧断言 `.easyjob/easyjob.sock` 必须改掉；windows 改为精确等于 `\\.\pipe\easyjob-system`。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p easyjob-ipc`
Expected: PASS（全部用例）。

- [ ] **Step 5: Commit**

```bash
git add crates/ipc/src/transport.rs crates/ipc/tests/transport_tests.rs
git commit -m "feat(ipc): 系统级固定 IPC 通道，Windows 管道名固定、Unix socket 跟随数据目录"
```

---

### Task 2: platform 启动核心（纯函数，可跨平台单测）

**Files:**
- Create: `crates/platform/src/startup.rs`
- Modify: `crates/platform/src/lib.rs`（加 `pub mod startup;` + 按需重导出）
- Test: `crates/platform/tests/startup_tests.rs`

**Interfaces:**
- Consumes: `std::path::{Path, PathBuf}`。
- Produces（后续 Task 4/5 直接使用，签名冻结）:
  - `pub enum StartupMode { Boot, Login, Disabled }`（派生 `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`，serde 小写 rename）
  - `pub const WINDOWS_SERVICE_NAME: &str = "easyJobAgent"`
  - `pub const WINDOWS_SERVICE_DISPLAY_NAME: &str = "easyJob Agent"`
  - `pub const WINDOWS_SYSTEM_PIPE: &str = r"\\.\pipe\easyjob-system"`
  - `pub const MACOS_DAEMON_LABEL: &str = "com.easyjob.agent"`
  - `pub const MACOS_SYSTEM_DATA_DIR: &str = "/Library/Application Support/EasyJob"`
  - `pub fn system_data_dir(exe_dir: Option<&Path>) -> PathBuf`（Windows：`exe_dir/data`；macOS：固定；Linux：沿用 `~/.easyjob` 不动）
  - `pub fn windows_service_image_path(agent_exe: &Path, data_dir: &Path) -> String`
  - `pub fn windows_install_elevated_ps(agent_exe: &Path, data_dir: &Path) -> (String, Vec<String>)`（返回 (`powershell.exe`, args)，含 `-Verb RunAs -Wait`）
  - `pub fn windows_uninstall_elevated_ps(agent_exe: &Path) -> (String, Vec<String>)`
  - `pub enum WindowsServiceState { Running, Stopped, NotInstalled }`
  - `pub fn parse_sc_query_state(output: &str) -> WindowsServiceState`（解析 `sc query` 文本里的 `STATE` 行，含 `RUNNING` 即 Running；含 `FAILED`/`STOPPED` 即 Stopped；含 `1060` / `specified service does not exist` 即 NotInstalled）
  - `pub fn macos_daemon_plist(agent_exe: &Path, data_dir: &Path) -> String`（plist XML，含 `RunAtLoad=true`、`KeepAlive=true`、日志重定向）
  - `pub fn macos_install_script(plist_path: &str, plist_content: &str) -> String`（供 `osascript ... with administrator privileges` 执行的 shell 脚本，调用方负责转义外层引号）
  - `pub fn quote_ps_arg(s: &str) -> String`（单引号包裹，内部单引号双写）

- [ ] **Step 1: Write the failing tests**

```rust
// crates/platform/tests/startup_tests.rs
use easyjob_platform::startup::*;
use std::path::Path;

#[test]
fn test_service_image_path_contains_service_and_data_dir() {
    let img = windows_service_image_path(
        Path::new(r"C:\Program Files\easyJob\easyjob-agent.exe"),
        Path::new(r"C:\Program Files\easyJob\data"),
    );
    assert!(img.contains("--service"), "{img}");
    assert!(img.contains(r"C:\Program Files\easyJob\data"), "{img}");
}

#[test]
fn test_parse_sc_query_running_and_missing() {
    assert_eq!(
        parse_sc_query_state("STATE : 4 RUNNING"),
        WindowsServiceState::Running
    );
    assert_eq!(
        parse_sc_query_state("[SC] OpenService FAILED 1060: 指定的服务未安装。"),
        WindowsServiceState::NotInstalled
    );
}

#[test]
fn test_macos_plist_has_run_at_load_and_keep_alive() {
    let plist = macos_daemon_plist(
        Path::new("/Applications/easyJob.app/Contents/Resources/easyjob-agent"),
        Path::new("/Library/Application Support/EasyJob"),
    );
    assert!(plist.contains("com.easyjob.agent"));
    assert!(plist.contains("<key>RunAtLoad</key>"));
    assert!(plist.contains("<key>KeepAlive</key>"));
    assert!(plist.contains("--data-dir"));
}

#[test]
fn test_ps_arg_quoting() {
    assert_eq!(quote_ps_arg("C:\\a'b"), "'C:\\a''b'");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p easyjob-platform --test startup_tests`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: Write minimal implementation**

按 Interfaces 签名实现 `crates/platform/src/startup.rs`，要点：
- `system_data_dir`: `#[cfg(target_os = "windows")]` 用 `exe_dir.join("data")`（`exe_dir=None` 时回退到当前 exe 所在目录）；`#[cfg(target_os = "macos")]` 返回固定路径忽略参数；其他平台返回 `~/.easyjob`。
- `windows_service_image_path` 格式：`"<exe>" --service --data-dir "<dir>"`。
- `parse_sc_query_state` 按字符串包含判断（大小写敏感匹配 `RUNNING`；`1060` 或 `does not exist` 或 `未安装` 判 NotInstalled；其他判 Stopped）。
- `macos_daemon_plist` 手写 XML，`ProgramArguments` 依次为 agent 路径、`--daemon`、`--data-dir`、数据目录；`StandardOutPath`/`StandardErrorPath` 指向 `<data>/logs/daemon.out.log` 等。
- 所有函数不得执行系统调用（纯拼装），提权执行由桌面端 Task 5 负责。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p easyjob-platform`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/platform/src/startup.rs crates/platform/src/lib.rs crates/platform/tests/startup_tests.rs
git commit -m "feat(platform): 启动模式核心纯函数（服务参数、plist 生成、sc 状态解析、提权命令拼装）"
```

---

### Task 3: IPC 服务端放行登录用户

**Files:**
- Modify: `crates/ipc/src/server.rs`
- Test: `crates/ipc/tests/transport_tests.rs`（扩展 `test_ipc_dead_socket_cleanup_and_permissions`）

**Interfaces:**
- Consumes: Task 1 的 `ipc_path_for_data_dir`；Task 2 的 `system_data_dir`（仅测试里拼路径）。
- Produces: `IpcServer::bind` 系统路径行为——Unix 下若 socket 父目录为系统目录则权限放宽到 `0o666`（登录用户可读写），否则保持 `0o600`；Windows 保持 `ServerOptions` 创建（ACL 若 API 不支持则不阻塞，以安装时 `icacls` 兜底）。

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn test_ipc_system_socket_permissions_shared() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("easyjob.sock");
    let server = IpcServer::bind(&socket_path, Arc::new(EchoHandler)).await.unwrap();
    tokio::spawn(server.run());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&socket_path).unwrap().permissions().mode() & 0o777;
        // 系统通道必须允许同机其他用户连接：至少 group/other 可读写
        assert!(mode & 0o066 != 0, "shared socket mode must include group/other rw, got {mode:o}");
    }
    let client = IpcClient::connect(&socket_path).await.unwrap();
    let resp = client.call("ping", serde_json::json!({})).await.unwrap();
    assert_eq!(resp, serde_json::json!("pong"));
}
```

注意：该测试要求 `bind` 对**所有**路径都放宽，还是只对系统路径放宽，Task 3 实现时二选一定死。若选“只对系统路径放宽”，则测试里需把 socket 建在 Task 2 的 `system_data_dir` 下（macOS CI 即 `/Library/...` 写不进去，改用 `ipc_path_for_data_dir(dir.path())` 并给 `bind` 加显式 `bind_shared` 入口）。决策：给 `IpcServer` 加 `pub async fn bind_shared(path, handler)`，系统通道统一走它；普通 `bind` 保持 `0o600`。旧测试保持 0600 断言不变，新测试走 `bind_shared` 断言 `0o666`。

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p easyjob-ipc --test transport_tests test_ipc_system_socket_permissions_shared`
Expected: FAIL（`bind_shared` 不存在）。

- [ ] **Step 3: Write minimal implementation**

```rust
impl IpcServer {
    /// System channel: socket readable/writable by logged-in users.
    pub async fn bind_shared(path: &Path, handler: Arc<dyn RequestHandler>) -> Result<Self> {
        let (event_tx, _) = broadcast::channel(1024);
        Self::bind_with_event_tx_and_mode(path, handler, event_tx, SocketMode::Shared).await
    }
}
```

`bind` / `bind_with_event_tx` 内部转调 `bind_with_event_tx_and_mode(..., SocketMode::Private)`；unix 分支按 mode 设 `0o600` / `0o666`。Windows 分支：Shared 模式暂与 Private 同行为（记录 `tracing::debug!`），真正 ACL 由安装步骤保证。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p easyjob-ipc`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/ipc/src/server.rs crates/ipc/tests/transport_tests.rs
git commit -m "feat(ipc): 系统通道 bind_shared，Unix 共享 socket 权限 0666"
```

---

### Task 4: Agent 服务形态与系统数据目录

**Files:**
- Modify: `apps/agent/src/main.rs`（CLI + 分发），`apps/agent/Cargo.toml`（Windows 加 `windows-service = "0.7"`）
- Modify: `apps/agent/src/service.rs`（若 service 停止回调需要入口：加 `pub async fn shutdown(&self)` 或复用现有 `shutdown_notify`，以实际编译为准，只做最小改动）
- Test: `cargo test -p easyjob-agent` 全量 + 新增 CLI 解析单测（`--service` 与 `--data-dir` 共存解析）

**Interfaces:**
- Consumes: Task 1 (`ipc_path_for_data_dir`)，Task 2 (`system_data_dir`)，Task 3 (`bind_shared` 由 `AgentService::init` 调用方选择——决定：`main.rs` 里系统路径恒走 `bind_shared`？`AgentService::init` 内部调 `bind_with_event_tx`。最小改动方案：`main.rs` 计算 `is_system: bool`（data_dir == system_data_dir），把 flag 透传给 `AgentService::init(..., shared: bool)`，init 内按 flag 选 `bind` / `bind_shared`）。
- Produces: CLI `--service`（Windows Service 前台入口）、`--install-service --data-dir <dir>`、`--uninstall-service`；默认 data dir = `system_data_dir(exe_dir)`；`--service` 与 `--daemon` 互斥（clap `conflicts_with`）。

- [ ] **Step 1: Write the failing test**

```rust
// apps/agent/tests/cli_tests.rs（新建）
use std::process::Command;

#[test]
fn test_service_and_daemon_conflict() {
    let out = Command::new(env!("CARGO_BIN_EXE_easyjob-agent"))
        .args(["--service", "--daemon"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p easyjob-agent --test cli_tests`
Expected: FAIL（ `--service` 参数不存在，clap 报 unknown argument，进程退出码非零但断言方向……注意：clap 对未知参数也会非零退出，测试会误 PASS。改为断言 stderr 包含 `--daemon` 冲突提示或先只跑 `--help` 包含 `--service`：`assert!(help.contains("--service"))`，此时必 FAIL）。

最终测试写成：

```rust
#[test]
fn test_help_lists_service_flags() {
    let out = Command::new(env!("CARGO_BIN_EXE_easyjob-agent"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("--service"), "{help}");
    assert!(help.contains("--install-service"), "{help}");
    assert!(help.contains("--uninstall-service"), "{help}");
}
```

- [ ] **Step 3: Write minimal implementation**

`main.rs` 改动：
- clap 新增三个 flag；`--install-service` / `--uninstall-service` 为 `bool`，需要提权，由桌面端拉起。
- 默认 data dir：`cli.data_dir.unwrap_or_else(|| system_data_dir(exe_dir))`，其中 `exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(...))`。
- Windows `--service`：用 `windows-service` crate 跑 `service_dispatcher::start(SERVICE_NAME, ffi_service_main)`，`ffi_service_main` 内执行现有 `run_agent(data_dir, max_concurrent)` 并注册 stop/shutdown handler（收到 stop 时触发 `shutdown_notify` / 直接 `std::process::exit(0)` 走优雅关闭，以最小可用为准）。
- `--install-service`：调用 `sc.exe create easyJobAgent binPath= "<image>" start= auto obj= LocalSystem` + `sc.exe failure easyJobAgent reset= 0 actions= restart/5000` + `sc.exe start easyJobAgent`（`binPath=` 后空格为 sc 语法要求）；`--uninstall-service`：`sc.exe stop`（忽略失败）+ `sc.exe delete`。输出中文错误。
- 非 Windows 的 `--service`：直接报错“仅 Windows 支持”。
- macOS/Linux：`--daemon` 行为不变，只是默认 data dir 变了。
- `AgentService::init` 加 `shared: bool` 参数，所有既有调用点（`service_tests.rs` 等）同步更新为 `false`。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p easyjob-agent`
Expected: PASS（macOS 本机跑；Windows Service 分支 `#[cfg(windows)]`，本机只验证 CLI 解析与 install 命令拼装单测）。

- [ ] **Step 5: Commit**

```bash
git add apps/agent/src/main.rs apps/agent/Cargo.toml apps/agent/src/service.rs apps/agent/tests/cli_tests.rs
git commit -m "feat(agent): Windows Service 形态与系统数据目录默认，install/uninstall-service 子命令"
```

---

### Task 5: 桌面端 Tauri 启动命令与 Agent 归属

**Files:**
- Create: `apps/desktop/src-tauri/src/startup.rs`
- Modify: `apps/desktop/src-tauri/src/agent_manager.rs`（系统 IPC + `allow_spawn` + `spawned_by_us` 跟踪）
- Modify: `apps/desktop/src-tauri/src/lib.rs`（注册命令、启动时按模式设 `allow_spawn`、退出时仅当 `spawned_by_us` 才 shutdown）
- Modify: `apps/desktop/src-tauri/Cargo.toml`（加 `easyjob-platform = { path = "../../../crates/platform" }`）
- Test: `apps/desktop/src-tauri/tests/bridge_tests.rs` 追加（模式推导纯逻辑单测；提权命令不真执行）

**Interfaces:**
- Consumes: Task 1/2/4 的函数；`tauri-plugin-autostart` 的 `enable/disable/is_enabled`（登录项侧，沿用）。
- Produces:
  - `#[tauri::command] pub async fn startup_get_mode() -> Result<StartupModeInfo, String>`
  - `#[tauri::command] pub async fn startup_set_mode(mode: String) -> Result<StartupModeInfo, String>`（`"boot" | "login" | "disabled"`，非法值返回中文错误）
  - `pub struct StartupModeInfo { pub mode: StartupMode, pub detail: String }`（`Serialize`）
  - `AgentManager::set_allow_spawn(&self, bool)`，`AgentManager::spawned_by_us(&self) -> bool`（内部 `AtomicBool`）。

- [ ] **Step 1: Write the failing test**

```rust
// bridge_tests.rs 追加
use easyjob_desktop_lib::startup::derive_mode;

#[test]
fn test_derive_mode_boot_requires_login_item_absent() {
    // service 运行中 + 登录项存在 => 非法组合，推导仍为 Boot 但 detail 告警（或直接报错，以实现为准，测试锁定行为）
    let info = derive_mode(true, true);
    assert_eq!(info.mode, easyjob_platform::startup::StartupMode::Boot);
}
```

`derive_mode(service_running: bool, login_enabled: bool) -> StartupModeInfo` 为纯函数，方便单测。

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p easyjob-desktop --test bridge_tests test_derive_mode_boot_requires_login_item_absent`
Expected: FAIL（`startup` 模块不存在）。

- [ ] **Step 3: Write minimal implementation**

`startup.rs` 要点：
- `startup_get_mode`：Windows 跑 `sc query easyJobAgent` → `parse_sc_query_state`；macOS 查 plist 存在 + `launchctl print system/com.easyjob.agent` 退出码；再查登录项 `is_enabled`（经 plugin 的 Rust 侧？plugin 命令走前端 JS，前端把 login 状态传进来或后端调 auto-launch？最小方案：后端只判定 service 侧，login 侧由前端 plugin 查询后一起拼——决定：`startup_get_mode` 返回 `{ service: ... }`，最终 mode 由前端 `startupMode.ts` 综合判定。把 `derive_mode` 放前端 TS 做纯函数，Rust 只暴露 `service_status()` + `set_service_enabled(bool)`。修正 Step 1 测试对象：Rust 侧单测改为 `parse` 相关已在 Task 2 覆盖，本 Task Rust 测试改为提权命令拼装调用 Task 2 函数的集成断言 + `AgentManager` 默认 `allow_spawn=true`。）
- 为避免过度设计，冻结最终分工：
  - Rust `startup.rs`：`service_status() -> { installed: bool, running: bool, data_dir: Option<String> }`、`service_install()`（提权拉起 agent `--install-service`，Windows `powershell -Verb RunAs -Wait`，macOS `osascript ... with administrator privileges`）、`service_uninstall()`、`login_set_enabled(bool)`（调 auto-launch？桌面端 Rust 侧无 auto-launch 直接依赖——用 `tauri-plugin-autostart` 的 Rust API？该 plugin 的 Rust 侧主要暴露命令给前端，无直接 enable API。决定：登录项侧继续由前端 plugin JS 调用，Rust 只管 service 侧 + data dir 推导 + AgentManager 行为。`startup_set_mode` 仍由前端编排多步，Rust 提供原子单步命令。）
- 因此 Tauri 命令定为：`service_status`、`service_install`、`service_uninstall`、`agent_spawn_mode(allow: bool)`（内部）——前端 `startupMode.ts` 编排切换流程与回滚。`startup_get/set_mode` 的命名按 spec 保留为前端编排函数名，不作为 Tauri 命令名，避免名实不符。
- `AgentManager`：`ipc_path` 默认 `system_ipc_path()`；`ensure_connected` 仅当 `allow_spawn` 才 spawn；spawn 成功后置 `spawned_by_us=true`；spawn 命令追加 `--data-dir <system_data_dir>`；`restart_agent` 在 `!allow_spawn` 时返回中文错误“开机自启模式下请通过系统服务管理器重启”。
- `lib.rs`：setup 里调 `service_status`（同步阻塞版，失败视为未安装）设 `allow_spawn = mode != Boot`；`ExitRequested` 仅当 `spawned_by_us` 为真才 `shutdown_agent`。

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p easyjob-desktop` 与 `cargo test --workspace`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/startup.rs apps/desktop/src-tauri/src/agent_manager.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/tests/bridge_tests.rs
git commit -m "feat(desktop): 系统服务管理命令与 Agent 归属跟踪，开机模式只连不拉起"
```

---

### Task 6: 前端三选一与迁移

**Files:**
- Create: `apps/desktop/src/services/startupMode.ts`
- Modify: `apps/desktop/src/views/SettingsView.vue`（`NSwitch` → `NRadioGroup`）
- Modify: `apps/desktop/src/App.vue`（旧 `initAutostartDefault` 改走新编排）
- Test: `apps/desktop/tests/startupMode.test.ts`

**Interfaces:**
- Consumes: 后端 `service_status/service_install/service_uninstall`（`invoke`），前端 plugin `enable/disable/isEnabled`（登录项），`getAgentStatus`（就绪等待）。
- Produces: `export type StartupMode = 'boot' | 'login' | 'disabled'`；`export async function getStartupMode(): Promise<StartupMode>`；`export async function setStartupMode(mode: StartupMode): Promise<StartupMode>`（内部先建新侧再清旧侧，失败回滚并抛中文错误）；`export function deriveMode(svc: {running: boolean}, login: boolean): StartupMode`（纯函数，单测锁定互斥表）。

- [ ] **Step 1: Write the failing test**

```ts
// apps/desktop/tests/startupMode.test.ts
import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-autostart', () => ({
  enable: vi.fn(), disable: vi.fn(), isEnabled: vi.fn(),
}));

import { deriveMode } from '../src/services/startupMode';

describe('deriveMode', () => {
  it('service running wins boot', () => {
    expect(deriveMode({ running: true }, true)).toBe('boot');
  });
  it('login item only maps login', () => {
    expect(deriveMode({ running: false }, true)).toBe('login');
  });
  it('neither maps disabled', () => {
    expect(deriveMode({ running: false }, false)).toBe('disabled');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm --dir apps/desktop vitest run tests/startupMode.test.ts`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: Write minimal implementation**

`startupMode.ts` 要点：
- `getStartupMode`: `invoke('service_status')` + `isEnabled()` → `deriveMode`。任一失败抛中文错误。
- `setStartupMode('boot')`: 确认框由调用方（SettingsView `NDialog`) 完成，本函数只做：`service_install` → 轮询 `service_status.running`（最多 15s）→ `disable()` 登录项 → 返回 `'boot'`；任一步失败则尝试回滚（`service_uninstall`）后抛错。
- `setStartupMode('login')`: `service_uninstall`（忽略 NotInstalled）→ `enable()` → 返回。
- `setStartupMode('disabled')`: `service_uninstall` → `disable()` → 返回。
- 保留旧 `autostart.ts` 的 `localStorage` 键做一次性迁移：`App.vue` 启动时若键不存在，先 `getStartupMode()`；若为 `disabled` 则 `setStartupMode('login')`（延续默认开启），然后写键。之后该键不再作为状态来源。
- `SettingsView.vue`: `NRadioGroup` 三项 + 说明文案；开机项标注“需管理员权限，未登录以 SYSTEM / root 运行”；切换失败回滚 `v-model` 并 `message.error`。

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm --dir apps/desktop test`
Expected: PASS（全量 vitest）。

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/services/startupMode.ts apps/desktop/src/views/SettingsView.vue apps/desktop/src/App.vue apps/desktop/tests/startupMode.test.ts
git commit -m "feat(desktop): 自启三选一（开机/登录/禁用）与切换回滚"
```

---

### Task 7: 打包与文档

**Files:**
- Modify: `README.md`（自启章节重写：登录自启保留 HKCU/LaunchAgent 描述；新增 Service/LaunchDaemon 章节；`~/.easyjob` 停用与手动迁移说明）
- Modify: `docs/superpowers/specs/2026-09-15-autostart-icon-and-settings-horizontal-design.md`（顶部加 supersede 注记，不改原文其余部分）
- Modify: `.github/workflows/release.yml`（仅当安装包框架支持时加 data 目录/ACL/卸载提示；不支持则不改文件，把“默认保留 + 手动删除路径”写进 README。本 Task 先做文档部分，打包改动以实际框架能力为准、无能力则留 TODO？不允许 TODO——决定：本 Task 只做文档，若框架不支持则明确写“运行时提权修复兜底”，不碰 workflow。）

- [ ] **Step 1: Write the failing check**：`rg -n "HKCU|LaunchAgent|Windows Service|LaunchDaemon" README.md` 必须同时命中四者，否则 FAIL。
- [ ] **Step 2: Run**：当前 README 只命中前两者 → FAIL。
- [ ] **Step 3: Implement**：重写自启章节。
- [ ] **Step 4: Re-run rg**：四者皆命中。
- [ ] **Step 5: Commit**

```bash
git add README.md docs/superpowers/specs/2026-09-15-autostart-icon-and-settings-horizontal-design.md
git commit -m "docs: 系统级开机自启说明与旧自启 spec  supersede 注记"
```

---

### Task 8: 全量验证

- [ ] **Step 1**: Run `cargo test --workspace`，Expected: PASS。
- [ ] **Step 2**: Run `pnpm --dir apps/desktop test`，Expected: PASS。
- [ ] **Step 3**: Run `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --check`，Expected: 无警告无 diff（有则修复后重跑）。
- [ ] **Step 4**: 手工矩阵（本机 macOS 可做部分；Windows 需真机/虚拟机，不在 CI 做）：
  1. 登录模式重启登录，确认托盘静默启动且任务正常。
  2. 切开机模式（提权），重启后不登录等待任务下一次触发，登录后在执行记录里看到未登录期间的执行。
  3. 切禁用，重启后确认不启动。
  4. Windows 卸载时确认保留数据弹窗（若安装包不支持，确认默认保留）。
- [ ] **Step 5**: 更新 CHANGELOG（按仓库既有格式加一条），提交。

```bash
git add CHANGELOG.md
git commit -m "chore(release): 系统级开机自启验证通过说明"
```

---

## Self-Review

- Spec 覆盖：§2 模式定义→Task 5/6；§3 架构→Task 4/5；§4 数据目录→Task 2/4；§5 IPC→Task 1/3；§6 Windows Service→Task 2/4/5；§7 LaunchDaemon→Task 2/5（plist 生成 Task 2，安装执行 Task 5 macOS 分支）；§8 桌面端→Task 5/6；§9 切换流程→Task 6；§10 错误处理→Task 5/6（中文错误+回滚）；§11 测试→各 Task + Task 8；§12 打包文档→Task 7。`~/.easyjob` 停用由 Task 4 默认 data dir 切换保证。
- 无 TODO/TBD 占位；各 Task 接口签名前后一致（`system_ipc_path`、`ipc_path_for_data_dir`、`bind_shared`、`system_data_dir`、`deriveMode`、`service_status/service_install/service_uninstall`）。
- 风险：Windows Service 控制循环需 `windows-service` 新依赖，CI 的 Windows 构建必须通过；`sc.exe`/`icacls`/`launchctl`/`osascript` 调用只做拼装单测，真机行为列入 Task 8 手工矩阵。
