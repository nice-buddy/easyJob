# 原生应用身份通知与生命周期协同实施计划 (Native Notification & Coordinated Lifecycle Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现原生 easyJob 应用身份通知（告别脚本编辑器图标与名称），支持点击通知唤醒主窗口并自动跳转至“执行记录”页面；同时对齐生命周期管理：关闭窗口仅隐藏，退出应用时协同退出后台守护进程。

**Architecture:**
- **Agent 端**：在 `apps/agent/src/service.rs` 的 `execution.finished` 事件中携带任务名称、耗时、错误信息及通知策略，并移除守护进程单独执行的 `osascript`（避免双重弹窗与脚本编辑器归属）。
- **桌面端通知**：在 `apps/desktop/src-tauri/src/events.rs` 中监听 `execution.finished`，使用 `tauri-plugin-notification` 以 `easyJob` 原生应用身份弹出通知（具备 Squircle 原生表盘图标与 easyJob 标题）。
- **点击通知导航**：在 `lib.rs` 中捕获 `RunEvent::Reopen`，激活并置顶窗口，发射 `navigate: 'executions'`；前端 `App.vue` 监听并在触发时将当前视图切换为“执行记录”并刷新数据。
- **协同退出**：在 `lib.rs` 中处理 `RunEvent::ExitRequested`，桌面端退出时同步向 Agent 发送 `agent.shutdown` RPC 请求并等待其退出。

**Tech Stack:** Tauri 2 (`tauri-plugin-notification`), Rust 2021, Vue 3, Naive UI, TypeScript, Vitest.

## Global Constraints

- 通知图标必须为 easyJob 原生图标，通知标题必须为 easyJob，禁止出现“脚本编辑器”或黄色卷轴。
- 点击通知必须能自动打开并置顶 easyJob 主窗口，并切换到“执行记录”页面。
- 点击窗口左上角关闭按钮 (X)：仅隐藏窗口，系统托盘与 Agent 守护进程必须继续常驻运行。
- Dock 栏右键退出、托盘菜单退出或 Cmd+Q：桌面端主程序与 Agent 守护进程必须协同全部退出。
- 质量门禁：`cargo test --all` 100% 通过，`cargo clippy` 0 警告，`pnpm -C apps/desktop test` 100% 通过，`pnpm -C apps/desktop run build` 0 错误。

---

### Task 1: Agent 事件负载扩充与去重 (Enrich execution.finished IPC Event)

**Files:**
- Modify: `apps/agent/src/service.rs:285-345`
- Test: `apps/agent/tests/service_tests.rs`

**Interfaces:**
- Consumes: `easyjob_domain::task::Task`, `ExecutionStatus`
- Produces: `execution.finished` IPC event containing `task_name`, `duration_ms`, `error_message`, `notification_policy`

- [ ] **Step 1: 编写失败测试用例**

在 `apps/agent/tests/service_tests.rs` 中添加断言，验证 `execution.finished` 事件的数据包中必须包含 `task_name` 与 `notification_policy` 字段：

```rust
#[tokio::test]
async fn test_execution_finished_event_contains_notification_metadata() {
    // 验证事件载荷中正确包含 task_name 与 notification_policy
}
```

- [ ] **Step 2: 运行测试以验证失败**

Run: `cargo test -p easyjob-agent --test service_tests test_execution_finished_event_contains_notification_metadata`
Expected: 失败或字段断言缺失。

- [ ] **Step 3: 扩充 service.rs 事件负载并移除后台 osascript 避免重复弹窗**

编辑 `apps/agent/src/service.rs`：
在 `execution.finished` 发送处增加元数据：
```rust
                            let _ = ev_tx.send(IpcEvent::new(
                                "execution.finished",
                                serde_json::json!({
                                    "execution_id": exec.id,
                                    "task_id": exec.task_id,
                                    "task_name": task.name,
                                    "status": exec.status,
                                    "exit_code": exec.exit_code,
                                    "duration_ms": duration_ms,
                                    "error_message": error_message,
                                    "notification_policy": task.execution_policy.notification,
                                }),
                            ));
```
移除 `apps/agent/src/service.rs` 中直接调用 `easyjob_platform::notification::send_system_notification` 的代码（统一移交桌面端主应用以原生 easyJob 身份发送）。

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p easyjob-agent`
Expected: 全部测试通过。

- [ ] **Step 5: 提交代码**

```bash
git add apps/agent/src/service.rs apps/agent/tests/service_tests.rs
git commit -m "feat(agent): enrich execution.finished event with task notification metadata"
```

---

### Task 2: 桌面端原生通知与点击跳转执行记录 (Native Notification & Click Navigation)

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/capabilities/default.json`
- Modify: `apps/desktop/src-tauri/src/events.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/App.vue`
- Test: `apps/desktop/tests/views.test.ts`

**Interfaces:**
- Consumes: `execution.finished` event from IPC
- Produces: Native app notification via `tauri-plugin-notification`, `navigate: 'executions'` event

- [ ] **Step 1: 编写前端导航事件响应测试**

在 `apps/desktop/tests/views.test.ts` 中添加测试用例，验证当接收到 `navigate: 'executions'` 时，页面将 `currentView` 设为 `'executions'`。

- [ ] **Step 2: 在 events.rs 中集成 tauri-plugin-notification 发送原生通知**

在 `apps/desktop/src-tauri/src/events.rs` 中：
```rust
use tauri_plugin_notification::NotificationExt;

// 监听 execution.finished 并判断 notification_policy
if event.event == "execution.finished" {
    if let Some(policy) = event.data.get("notification_policy").and_then(|v| v.as_str()) {
        let status = event.data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let is_success = status == "Succeeded";
        let need_notify = match policy {
            "OnlySuccess" => is_success,
            "OnlyFailure" => !is_success,
            "All" => true,
            _ => false,
        };

        if need_notify {
            let task_name = event.data.get("task_name").and_then(|v| v.as_str()).unwrap_or("未命名任务");
            let duration_ms = event.data.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);
            let duration_str = if duration_ms < 1000 {
                format!("{}ms", duration_ms)
            } else {
                format!("{:.2}s", duration_ms as f64 / 1000.0)
            };

            let title = if is_success { "easyJob - 任务执行成功" } else { "easyJob - 任务执行失败" };
            let mut body = format!("任务「{}」耗时: {}", task_name, duration_str);
            if !is_success {
                if let Some(err) = event.data.get("error_message").and_then(|v| v.as_str()) {
                    let truncated: String = err.chars().take(60).collect();
                    body.push_str(&format!(" | 错误: {}", truncated));
                }
            }

            let _ = app_handle.notification()
                .builder()
                .title(title)
                .body(body)
                .show();
        }
    }
}
```

- [ ] **Step 3: 在 lib.rs 中注册插件与处理 RunEvent::Reopen**

在 `apps/desktop/src-tauri/src/lib.rs` 中：
1. 注册 `.plugin(tauri_plugin_notification::init())`。
2. 在 `RunEvent::Reopen` 中：
```rust
tauri::RunEvent::Reopen { .. } => {
    set_dock_visible(true);
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("navigate", "executions");
    }
}
```

- [ ] **Step 4: 在 App.vue 中监听 navigate 事件**

在 `apps/desktop/src/App.vue` 中：
```typescript
import { listen } from '@tauri-apps/api/event';

let unlistenNavigate: (() => void) | null = null;

onMounted(async () => {
  // ...
  unlistenNavigate = await listen<string>('navigate', (event) => {
    if (event.payload === 'executions') {
      currentView.value = 'executions';
      executionStore.loadExecutions();
    }
  });
});

onUnmounted(() => {
  if (unlistenNavigate) unlistenNavigate();
});
```

- [ ] **Step 5: 运行前端与 Rust 测试验证**

Run: `cargo check -p easyjob-desktop && pnpm -C apps/desktop test`
Expected: 全部测试通过。

- [ ] **Step 6: 提交代码**

```bash
git add apps/desktop/src-tauri/ apps/desktop/src/App.vue apps/desktop/tests/
git commit -m "feat(desktop): implement native easyJob notifications and click-to-executions navigation"
```

---

### Task 3: 桌面退出与守护进程协同生命周期联动 (Coordinated Lifecycle: Window Close vs App Quit)

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/tray.rs`
- Test: `apps/desktop/src-tauri/tests/`

**Interfaces:**
- Consumes: `AgentManager::call("agent.shutdown", ...)`
- Produces: Graceful shutdown of agent daemon on app exit

- [ ] **Step 1: 在 lib.rs 中将 run 升级为 build + run 并处理 ExitRequested**

编辑 `apps/desktop/src-tauri/src/lib.rs`：
```rust
    let manager_for_exit = agent_manager.clone();

    builder
        .build(tauri::generate_context!())
        .expect("error while running easyJob desktop")
        .run(move |app_handle, event| {
            match event {
                tauri::RunEvent::ExitRequested { .. } => {
                    tracing::info!("Application exit requested, shutting down agent daemon...");
                    let manager = manager_for_exit.clone();
                    tauri::async_runtime::block_on(async move {
                        let _ = manager.call("agent.shutdown", serde_json::json!({})).await;
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    });
                }
                tauri::RunEvent::Reopen { .. } => {
                    set_dock_visible(true);
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                        let _ = window.emit("navigate", "executions");
                    }
                }
                _ => {}
            }
        });
```

- [ ] **Step 2: 验证托盘退出逻辑**

检查 `tray.rs` 中点击“退出 easyJob”调用 `app.exit(0)`，确保触发 `RunEvent::ExitRequested` 并协同关闭守护进程。

- [ ] **Step 3: 运行 Rust 工作区检查**

Run: `cargo check -p easyjob-desktop`
Expected: 编译通过。

- [ ] **Step 4: 提交代码**

```bash
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/tray.rs
git commit -m "feat(desktop): synchronize app exit with agent daemon shutdown while preserving tray on window close"
```

---

### Task 4: 全工作区验证与端到端构建 (Workspace Verification & Build)

**Files:**
- Workspace-wide verification

- [ ] **Step 1: 运行 Rust 单元测试**
Run: `cargo test --all`
Expected: 全部测试通过。

- [ ] **Step 2: 运行 Rust Clippy 检查**
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 0 警告。

- [ ] **Step 3: 检查格式规范**
Run: `cargo fmt --check`
Expected: 100% 合规。

- [ ] **Step 4: 运行前端单元测试**
Run: `pnpm -C apps/desktop test`
Expected: 100% 通过。

- [ ] **Step 5: 运行前端构建与类型检查**
Run: `pnpm -C apps/desktop run build`
Expected: 0 错误构建成功。

- [ ] **Step 6: 提交发布代码**
```bash
git commit --allow-empty -m "chore(release): native notifications and coordinated lifecycle complete"
```
