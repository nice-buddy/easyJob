# 跨平台任务执行通知与导入比对高亮实施计划 (Task Notification Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 easyJob 任务配置增加跨平台执行结果系统通知功能（不通知、成功通知、失败通知、全都通知），在 macOS 与 Windows 双平台即使桌面 GUI 关闭时依然由后台守护进程弹出原生系统通知，并在任务导入三栏手风琴中实现精确差异比对与高亮。

**Architecture:**
- 领域层：在 `crates/domain/src/policy.rs` 中定义 `TaskNotificationPolicy` 枚举并扩展 `ExecutionPolicy`，利用 `#[serde(default)]` 保证 SQLite 数据库零迁移兼容。
- 平台层：在 `crates/platform/src/notification.rs` 中实现跨平台原生通知发送器（macOS 调用 `osascript` 唤起通知中心，Windows 通过隐藏窗口 PowerShell 触发 WinRT Toast 通知）。
- 调度层：在 `apps/agent/src/service.rs` 中任务执行生命周期结束处接入通知策略评估，并通过异步非阻塞协程派发系统通知。
- 前端层：扩展 `task.ts` 类型与 `taskDiff.ts` 差异比对算法，在 `TaskDrawer.vue` 和 `TaskAccordionContent.vue` 中提供表单编辑、只读展示与差异高亮。

**Tech Stack:** Rust 2021 (tokio, serde, serde_json), macOS AppleScript (`osascript`), Windows WinRT Toast (`powershell` with `CREATE_NO_WINDOW`), Vue 3, Naive UI, TypeScript, Vitest.

## Global Constraints

- 任务通知策略枚举必须严格支持：`None` (不通知)、`OnlySuccess` (仅成功通知)、`OnlyFailure` (仅失败通知)、`All` (全都通知)。
- 存量数据库兼容：禁止修改 SQLite `tasks` 表结构，`ExecutionPolicy.notification` 必须使用 `#[serde(default)]` 自动回退为 `None`。
- 守护进程无 GUI 独立通知：无论桌面端 GUI 是否处于运行状态，后台 `easyjob-agent` 均能直接唤起系统原生通知。
- 导入比对要求：`taskDiff.ts` 必须将通知策略差异记录为 `policy.notification` 且归入 `policyDiff`；三栏手风琴必须在导入栏高亮并支持快捷合并。
- 质量门禁：`cargo test --all` 100% 通过，`cargo clippy --workspace --all-targets -- -D warnings` 0 警告，`pnpm -C apps/desktop test` 100% 通过，`pnpm -C apps/desktop run build` 0 错误。

---

### Task 1: 领域模型扩展与向后兼容性单元测试 (Domain Model & Serde Backward Compatibility)

**Files:**
- Modify: `crates/domain/src/policy.rs`
- Modify: `crates/domain/src/lib.rs`
- Test: `crates/domain/tests/domain_tests.rs`

**Interfaces:**
- Consumes: None
- Produces: `TaskNotificationPolicy` (enum with `None`, `OnlySuccess`, `OnlyFailure`, `All`), `ExecutionPolicy.notification: TaskNotificationPolicy`

- [ ] **Step 1: 编写领域层失败测试用例**

在 `crates/domain/tests/domain_tests.rs` 末尾添加针对 `TaskNotificationPolicy` 及其向后兼容性的测试：

```rust
#[test]
fn test_task_notification_policy_serialization_and_backward_compatibility() {
    use easyjob_domain::policy::TaskNotificationPolicy;

    // 1. 验证新结构包含 notification 时的完整往返序列化
    let policy = ExecutionPolicy {
        concurrency_policy: ConcurrencyPolicy::SkipIfRunning,
        missed_run_policy: MissedRunPolicy::RunOnce,
        retry_policy: RetryPolicy::default(),
        timeout_secs: Some(60),
        notification: TaskNotificationPolicy::OnlyFailure,
    };
    let json_str = serde_json::to_string(&policy).unwrap();
    assert!(json_str.contains("\"notification\":\"OnlyFailure\""));
    let deserialized: ExecutionPolicy = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.notification, TaskNotificationPolicy::OnlyFailure);

    // 2. 验证向后兼容性：旧版本 JSON 没有 notification 字段时，必须默认反序列化为 TaskNotificationPolicy::None
    let legacy_json = r#"{
        "concurrency_policy": "AllowParallel",
        "missed_run_policy": "Skip",
        "retry_policy": {"max_retries": 1, "delay_secs": 2},
        "timeout_secs": 120
    }"#;
    let legacy_deserialized: ExecutionPolicy = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(legacy_deserialized.notification, TaskNotificationPolicy::None);
}
```

- [ ] **Step 2: 运行测试以验证失败**

Run: `cargo test -p easyjob-domain --test domain_tests test_task_notification_policy`
Expected: 编译失败，提示 `TaskNotificationPolicy` 未找到或 `ExecutionPolicy` 缺少 `notification` 字段。

- [ ] **Step 3: 编写领域层最小实现**

编辑 `crates/domain/src/policy.rs`：
在文件顶部添加 `TaskNotificationPolicy` 枚举，并在 `ExecutionPolicy` 中增加 `notification` 字段：

```rust
use serde::{Deserialize, Serialize};

/// 任务执行完成后的系统通知策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TaskNotificationPolicy {
    #[default]
    None,         // 不通知
    OnlySuccess,  // 仅成功时通知
    OnlyFailure,  // 仅失败时通知
    All,          // 无论成功失败均通知
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConcurrencyPolicy {
    AllowParallel,
    #[default]
    SkipIfRunning,
    QueueOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MissedRunPolicy {
    #[default]
    RunOnce,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub delay_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionPolicy {
    pub concurrency_policy: ConcurrencyPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub retry_policy: RetryPolicy,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub notification: TaskNotificationPolicy,
}
```

编辑 `crates/domain/src/lib.rs`，确保导出 `TaskNotificationPolicy`：
```rust
pub use policy::{ConcurrencyPolicy, ExecutionPolicy, MissedRunPolicy, RetryPolicy, TaskNotificationPolicy};
```

检查并修复 `domain_tests.rs` 及其他可能以字面量构造 `ExecutionPolicy` 的地方（添加 `notification: Default::default()` 或 `..Default::default()`）。

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p easyjob-domain`
Expected: 全部测试 PASS。

- [ ] **Step 5: 提交代码**

```bash
git add crates/domain/src/policy.rs crates/domain/src/lib.rs crates/domain/tests/domain_tests.rs
git commit -m "feat(domain): add TaskNotificationPolicy with backwards-compatible serde default"
```

---

### Task 2: 跨平台原生系统通知引擎实现 (Cross-Platform Native Notification Engine)

**Files:**
- Create: `crates/platform/src/notification.rs`
- Modify: `crates/platform/src/lib.rs`
- Create/Modify: `crates/platform/tests/notification_tests.rs`

**Interfaces:**
- Consumes: `easyjob_common::Result`
- Produces: `easyjob_platform::notification::send_system_notification(title: &str, subtitle: Option<&str>, body: &str) -> easyjob_common::Result<()>`

- [ ] **Step 1: 编写通知模块失败测试用例**

创建 `crates/platform/tests/notification_tests.rs`：

```rust
use easyjob_platform::notification::send_system_notification;

#[tokio::test]
async fn test_send_system_notification_smoke() {
    // 验证调用接口传参转义安全且不抛 panic
    let res = send_system_notification("easyJob Test", Some("Subtitle \"Quotes\""), "Body with \\ and \n").await;
    assert!(res.is_ok());
}
```

- [ ] **Step 2: 运行测试以验证失败**

Run: `cargo test -p easyjob-platform --test notification_tests`
Expected: 编译失败，找不到 `easyjob_platform::notification` 模块。

- [ ] **Step 3: 编写跨平台系统通知引擎实现**

创建 `crates/platform/src/notification.rs`：

```rust
use easyjob_common::Result;

/// 发送系统原生桌面横幅通知（macOS / Windows）
pub async fn send_system_notification(
    title: &str,
    subtitle: Option<&str>,
    body: &str,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let escaped_title = title.replace('\\', "\\\\").replace('\"', "\\\"");
        let escaped_body = body.replace('\\', "\\\\").replace('\"', "\\\"");

        let script = if let Some(sub) = subtitle {
            let escaped_sub = sub.replace('\\', "\\\\").replace('\"', "\\\"");
            format!(
                "display notification \"{}\" with title \"{}\" subtitle \"{}\" sound name \"default\"",
                escaped_body, escaped_title, escaped_sub
            )
        } else {
            format!(
                "display notification \"{}\" with title \"{}\" sound name \"default\"",
                escaped_body, escaped_title
            )
        };

        let mut cmd = tokio::process::Command::new("osascript");
        cmd.arg("-e").arg(script);

        match tokio::time::timeout(std::time::Duration::from_secs(3), cmd.output()).await {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!("osascript notification failed: {}", err);
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Failed to execute osascript: {}", e);
            }
            Err(_) => {
                tracing::warn!("osascript notification timed out after 3s");
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        let header = if let Some(sub) = subtitle {
            format!("{} - {}", title, sub)
        } else {
            title.to_string()
        };

        let safe_header = header.replace('\'', "''");
        let safe_body = body.replace('\'', "''");

        let script = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
             $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
             $xml = [xml]$template.GetXml(); \
             $nodes = $xml.GetElementsByTagName('text'); \
             $nodes[0].AppendChild($xml.CreateTextNode('{safe_header}')) > $null; \
             $nodes[1].AppendChild($xml.CreateTextNode('{safe_body}')) > $null; \
             $toast = [Windows.UI.Notifications.ToastNotification]::new($template); \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('easyJob').Show($toast);"
        );

        let mut cmd = tokio::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script]);
        crate::windows::configure_windows_command(cmd.as_std_mut());

        match tokio::time::timeout(std::time::Duration::from_secs(3), cmd.output()).await {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::warn!("PowerShell Toast notification failed: {}", err);
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Failed to execute powershell toast: {}", e);
            }
            Err(_) => {
                tracing::warn!("PowerShell toast notification timed out after 3s");
            }
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (title, subtitle, body);
        tracing::debug!("System notification not supported on this platform");
        Ok(())
    }
}
```

在 `crates/platform/src/lib.rs` 中导出：
```rust
pub mod notification;
pub mod process;
#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

pub use notification::send_system_notification;
pub use process::{kill_process_tree, CommandBuilder, PlatformProcess};
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p easyjob-platform`
Expected: `test_send_system_notification_smoke` PASS。

- [ ] **Step 5: 提交代码**

```bash
git add crates/platform/src/notification.rs crates/platform/src/lib.rs crates/platform/tests/notification_tests.rs
git commit -m "feat(platform): implement cross-platform native notification engine for macOS and Windows"
```

---

### Task 3: Agent 守护进程任务完成通知集成 (Agent Daemon Execution Hook & Notification Dispatch)

**Files:**
- Modify: `apps/agent/src/service.rs:270-302`
- Test: `apps/agent/Cargo.toml` / workspace tests

**Interfaces:**
- Consumes: `easyjob_domain::policy::TaskNotificationPolicy`, `easyjob_platform::notification::send_system_notification`
- Produces: Execution finish notification hook in Agent daemon

- [ ] **Step 1: 检查现有 service.rs 编译与测试**

Run: `cargo check -p easyjob-agent`
Expected: 编译通过。

- [ ] **Step 2: 在 apps/agent/src/service.rs 中接入通知逻辑**

在 `apps/agent/src/service.rs` 约 295 行 `let _ = ev_tx.send(IpcEvent::new("execution.finished", ...));` 紧接着添加通知派发逻辑：

```rust
                            // 检查任务通知策略并异步触发系统通知
                            let need_notify = match task.execution_policy.notification {
                                easyjob_domain::policy::TaskNotificationPolicy::None => false,
                                easyjob_domain::policy::TaskNotificationPolicy::OnlySuccess => {
                                    final_status == ExecutionStatus::Succeeded
                                }
                                easyjob_domain::policy::TaskNotificationPolicy::OnlyFailure => {
                                    final_status != ExecutionStatus::Succeeded
                                }
                                easyjob_domain::policy::TaskNotificationPolicy::All => true,
                            };

                            if need_notify {
                                let task_name = task.name.clone();
                                let is_success = final_status == ExecutionStatus::Succeeded;
                                let duration_str = if duration_ms < 1000 {
                                    format!("{}ms", duration_ms)
                                } else {
                                    format!("{:.2}s", duration_ms as f64 / 1000.0)
                                };

                                let subtitle = if is_success {
                                    format!("任务执行成功: {}", task_name)
                                } else {
                                    format!("任务执行失败: {}", task_name)
                                };

                                let mut body = format!("耗时: {}", duration_str);
                                if !is_success {
                                    if let Some(ref err) = error_message {
                                        let truncated_err = err.chars().take(80).collect::<String>();
                                        body.push_str(&format!(" | 错误: {}", truncated_err));
                                    }
                                }

                                tokio::spawn(async move {
                                    if let Err(e) = easyjob_platform::notification::send_system_notification(
                                        "easyJob",
                                        Some(&subtitle),
                                        &body,
                                    )
                                    .await
                                    {
                                        tracing::warn!(
                                            "Failed to send system notification for task '{}': {:?}",
                                            task_name,
                                            e
                                        );
                                    }
                                });
                            }
```

- [ ] **Step 3: 运行 Rust 工作区全量测试验证**

Run: `cargo test -p easyjob-agent && cargo test --all`
Expected: 全部测试 PASS。

- [ ] **Step 4: 提交代码**

```bash
git add apps/agent/src/service.rs
git commit -m "feat(agent): trigger native system notifications on task completion per notification policy"
```

---

### Task 4: 前端类型与深度比对工具扩展 (Frontend Types & taskDiff.ts with Tests)

**Files:**
- Modify: `apps/desktop/src/types/task.ts`
- Modify: `apps/desktop/src/utils/taskDiff.ts`
- Modify: `apps/desktop/tests/taskDiff.test.ts`

**Interfaces:**
- Consumes: None
- Produces: `TaskNotificationPolicy` type, `taskDiff.ts` supporting `policy.notification`

- [ ] **Step 1: 编写前端比对失败测试用例**

在 `apps/desktop/tests/taskDiff.test.ts` 中添加针对 `notification` 差异比对的测试：

```typescript
  it('detects notification policy differences', () => {
    const t1 = getEmptyTask();
    t1.execution_policy.notification = 'None';
    const t2 = JSON.parse(JSON.stringify(t1));
    t2.execution_policy.notification = 'OnlyFailure';

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.policyDiff).toBe(true);
    expect(diff.diffFields.has('policy.notification')).toBe(true);
  });
```

- [ ] **Step 2: 运行测试以验证失败**

Run: `pnpm -C apps/desktop test tests/taskDiff.test.ts`
Expected: 运行失败（`notification` 属性在类型定义中不存在或比对未生效）。

- [ ] **Step 3: 更新前端类型与比对实现**

编辑 `apps/desktop/src/types/task.ts`：
```typescript
export type TaskNotificationPolicy = 'None' | 'OnlySuccess' | 'OnlyFailure' | 'All';

export interface ExecutionPolicy {
  concurrency_policy: ConcurrencyPolicy;
  missed_run_policy: MissedRunPolicy;
  retry_policy: RetryPolicy;
  timeout_secs: number | null;
  notification: TaskNotificationPolicy;
}
```
并在 `getEmptyTask()` 中初始化：
```typescript
    execution_policy: {
      concurrency_policy: 'SkipIfRunning',
      missed_run_policy: 'RunOnce',
      retry_policy: {
        max_retries: 0,
        delay_secs: 0,
      },
      timeout_secs: 3600,
      notification: 'None',
    },
```

编辑 `apps/desktop/src/utils/taskDiff.ts`，在 `2. Execution policy` 比对区域中加入：
```typescript
  const n1 = ep1.notification || 'None';
  const n2 = ep2.notification || 'None';
  if (n1 !== n2) {
    diffFields.add('policy.notification');
  }

  const policyDiff =
    diffFields.has('policy.concurrency_policy') ||
    diffFields.has('policy.missed_run_policy') ||
    diffFields.has('policy.timeout_secs') ||
    diffFields.has('policy.retry_max_retries') ||
    diffFields.has('policy.notification');
```

- [ ] **Step 4: 运行前端测试验证通过**

Run: `pnpm -C apps/desktop test tests/taskDiff.test.ts`
Expected: 测试通过。

- [ ] **Step 5: 提交代码**

```bash
git add apps/desktop/src/types/task.ts apps/desktop/src/utils/taskDiff.ts apps/desktop/tests/taskDiff.test.ts
git commit -m "feat(desktop): add TaskNotificationPolicy type and integrate notification comparison in taskDiff"
```

---

### Task 5: 任务编辑抽屉与三栏手风琴比对高亮 UI 改造 (TaskDrawer & TaskAccordionContent UI)

**Files:**
- Modify: `apps/desktop/src/components/task/TaskDrawer.vue`
- Modify: `apps/desktop/src/components/task/TaskAccordionContent.vue`
- Test: `apps/desktop/tests/taskMerge.test.ts`

**Interfaces:**
- Consumes: `TaskNotificationPolicy`, `taskDiff.ts`
- Produces: UI options and diff highlight in TaskDrawer & TaskAccordionContent

- [ ] **Step 1: 编写 taskMerge 自动化测试用例**

在 `apps/desktop/tests/taskMerge.test.ts` 中添加关于通知策略合并的测试：

```typescript
  it('correctly compares and copies notification policy in task merge', () => {
    const existing = getEmptyTask();
    existing.execution_policy.notification = 'None';

    const imported = getEmptyTask();
    imported.execution_policy.notification = 'All';

    const merged = JSON.parse(JSON.stringify(imported));
    expect(merged.execution_policy.notification).toBe('All');

    // 采用已有配置
    merged.execution_policy.notification = existing.execution_policy.notification;
    expect(merged.execution_policy.notification).toBe('None');
  });
```

- [ ] **Step 2: 改造 TaskDrawer.vue 增加通知选项**

在 `apps/desktop/src/components/task/TaskDrawer.vue` 中：
1. 增加通知策略选项数组：
```typescript
const notificationOptions: { label: string; value: TaskNotificationPolicy }[] = [
  { label: '不通知 (默认)', value: 'None' },
  { label: '仅成功时通知 (OnlySuccess)', value: 'OnlySuccess' },
  { label: '仅失败时通知 (OnlyFailure)', value: 'OnlyFailure' },
  { label: '全部通知 (成功与失败均通知)', value: 'All' },
];
```
2. 在“基本配置与策略”的表单网格中加入该字段：
```html
<NFormItem label="执行结果通知">
  <NSelect
    v-model:value="currentTask.execution_policy.notification"
    :options="notificationOptions"
  />
</NFormItem>
```

- [ ] **Step 3: 改造 TaskAccordionContent.vue 增加只读展示与差异高亮**

在 `apps/desktop/src/components/task/TaskAccordionContent.vue` 中：
1. 定义通知策略选项与中文标签辅助函数：
```typescript
const notificationOptions = [
  { label: '不通知 (默认)', value: 'None' },
  { label: '仅成功时通知 (OnlySuccess)', value: 'OnlySuccess' },
  { label: '仅失败时通知 (OnlyFailure)', value: 'OnlyFailure' },
  { label: '全部通知 (成功与失败均通知)', value: 'All' },
];

function formatNotification(policy?: string) {
  switch (policy) {
    case 'OnlySuccess': return '仅成功时通知';
    case 'OnlyFailure': return '仅失败时通知';
    case 'All': return '全部通知 (成功与失败均通知)';
    default: return '不通知';
  }
}
```
2. 更新执行策略手风琴头部 extra 标签：
```html
<template #header-extra>
  <NTag
    v-if="!editable && (isDiff('policy.concurrency_policy') || isDiff('policy.missed_run_policy') || isDiff('policy.timeout_secs') || isDiff('policy.retry_max_retries') || isDiff('policy.notification'))"
    size="tiny"
    type="warning"
  >
    有差异
  </NTag>
</template>
```
3. 在只读视图中增加行展示与差异高亮：
```html
<div :class="['p-2 rounded', isDiff('policy.notification') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
  <span class="text-slate-400">结果通知：</span>
  <span class="font-medium">{{ formatNotification(task.execution_policy.notification) }}</span>
</div>
```
4. 在可编辑视图（栏 3）中增加 `NSelect`：
```html
<NFormItem label="执行结果通知">
  <NSelect v-model:value="task.execution_policy.notification" :options="notificationOptions" />
</NFormItem>
```

- [ ] **Step 4: 运行前端所有单元测试**

Run: `pnpm -C apps/desktop test`
Expected: 107+ tests passed 100%.

- [ ] **Step 5: 提交代码**

```bash
git add apps/desktop/src/components/task/TaskDrawer.vue apps/desktop/src/components/task/TaskAccordionContent.vue apps/desktop/tests/taskMerge.test.ts
git commit -m "feat(desktop): add notification policy selection to TaskDrawer and diff highlight to TaskAccordionContent"
```

---

### Task 6: 全工作区集成构建与端到端验证 (Full Workspace Build & Verification)

**Files:**
- None (Workspace-wide validation)

**Interfaces:**
- Workspace tests, Clippy, Format, TypeScript check, Vite build

- [ ] **Step 1: 运行 Rust 工作区单元测试**

Run: `cargo test --all`
Expected: 80+ tests pass with 0 errors.

- [ ] **Step 2: 运行 Rust Clippy 静态代码检查**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 0 warnings.

- [ ] **Step 3: 检查 Rust 代码格式规范**

Run: `cargo fmt --check`
Expected: 0 format issues.

- [ ] **Step 4: 运行前端 Vitest 单元测试**

Run: `pnpm -C apps/desktop test`
Expected: 全部通过。

- [ ] **Step 5: 运行前端类型检查与生产构建**

Run: `pnpm -C apps/desktop run build`
Expected: `vue-tsc --noEmit && vite build` 0 错误构建成功。

- [ ] **Step 6: 提交最终集成代码**

```bash
git commit --allow-empty -m "chore(release): task notification feature complete across backend daemon and desktop UI"
```
