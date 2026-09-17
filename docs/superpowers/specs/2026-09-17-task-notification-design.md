# easyJob 跨平台任务执行通知与导入比对高亮设计规范

**日期**: 2026-09-17  
**状态**: Approved (已评审确认)  
**作者**: easyJob 核心架构组  

---

## 1. 概述与背景

easyJob 目前支持丰富的任务定时调度与跨平台执行控制。为了让用户在任务执行完成后及时获知执行结果，需要在任务配置中增加系统通知能力：
1. **多档通知策略**：支持 `不通知 (None)`、`仅成功通知 (OnlySuccess)`、`仅失败通知 (OnlyFailure)`、`全都通知 (All)` 四种可配置选项。
2. **跨平台原生支持**：支持 macOS 与 Windows 双平台。在桌面 GUI 客户端未启动或退出、仅后台 `easyjob-agent` 守护进程运行时，依然能直接通过系统底层 API 弹出原生系统通知横幅。
3. **导入导出比对与合并**：在任务配置导入比对弹窗（三栏纵向手风琴）中，支持对该通知选项进行精准比对与差异高亮，并允许在合并栏中编辑或采用。
4. **历史数据 100% 兼容**：通过 Serde 默认值实现存量 SQLite 数据库零迁移成本升级。

---

## 2. 领域模型与数据结构设计

### 2.1 枚举与结构体定义 (`crates/domain/src/policy.rs`)

将任务通知策略作为执行策略（`ExecutionPolicy`）的核心子属性：

```rust
use serde::{Deserialize, Serialize};

/// 任务执行完成后的系统通知策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TaskNotificationPolicy {
    #[default]
    None,         // 不通知（默认）
    OnlySuccess,  // 仅成功时通知
    OnlyFailure,  // 仅失败时通知
    All,          // 无论成功失败均通知
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionPolicy {
    pub concurrency_policy: ConcurrencyPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub retry_policy: RetryPolicy,
    pub timeout_secs: Option<u64>,
    /// 执行通知策略（Serde 默认值为 None，确保完全向后兼容旧版本持久化数据）
    #[serde(default)]
    pub notification: TaskNotificationPolicy,
}
```

### 2.2 持久化与向后兼容性保证

- **存储形式**：SQLite `tasks` 表中的 `execution_policy_json` 字段存储为 JSON 文本。
- **兼容策略**：使用 `#[serde(default)]`。反序列化旧数据时，缺失 `notification` 键将自动填充为 `TaskNotificationPolicy::None`。
- **数据库架构**：无需执行任何 `ALTER TABLE` 迁移，存量用户数据库平滑无缝升级。

### 2.3 前端 TypeScript 类型定义 (`apps/desktop/src/types/task.ts`)

```typescript
export type TaskNotificationPolicy = 'None' | 'OnlySuccess' | 'OnlyFailure' | 'All';

export interface ExecutionPolicy {
  concurrency_policy: ConcurrencyPolicy;
  missed_run_policy: MissedRunPolicy;
  retry_policy: RetryPolicy;
  timeout_secs?: number | null;
  notification: TaskNotificationPolicy;
}
```

---

## 3. 原生通知引擎实现 (`crates/platform`)

在 `crates/platform` 中新增 `notification.rs` 模块，对外暴露通用的异步接口：

```rust
pub async fn send_system_notification(
    title: &str,
    subtitle: Option<&str>,
    body: &str,
) -> easyjob_common::Result<()>;
```

### 3.1 macOS 实现 (`#[cfg(target_os = "macos")]`)

macOS 平台的后台守护进程（Daemon）在脱离 Dock 或处于无窗口状态时，使用系统内置的 `osascript` 唤起通知：

```rust
#[cfg(target_os = "macos")]
pub async fn send_system_notification(
    title: &str,
    subtitle: Option<&str>,
    body: &str,
) -> easyjob_common::Result<()> {
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

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        cmd.output(),
    )
    .await
    .map_err(|_| easyjob_common::Error::Process("Notification command timed out".to_string()))?
    .map_err(|e| easyjob_common::Error::Process(format!("Failed to execute osascript: {}", e)))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("osascript notification failed: {}", err);
    }

    Ok(())
}
```

### 3.2 Windows 实现 (`#[cfg(windows)]`)

Windows 10/11 平台通过无窗 PowerShell 执行 WinRT Toast 通知，结合 `CREATE_NO_WINDOW` 保证完全静默无控制台弹窗：

```rust
#[cfg(windows)]
pub async fn send_system_notification(
    title: &str,
    subtitle: Option<&str>,
    body: &str,
) -> easyjob_common::Result<()> {
    let header = if let Some(sub) = subtitle {
        format!("{} - {}", title, sub)
    } else {
        title.to_string()
    };
    
    // 转义 PowerShell 单引号字符
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

    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        cmd.output(),
    )
    .await;

    Ok(())
}
```

### 3.3 Linux / 其他平台回退 (`#[cfg(not(any(target_os = "macos", windows)))]`)

记录跟踪日志，可选尝试调用 `notify-send` 命令。

---

## 4. Agent 守护进程通知调度 (`apps/agent/src/service.rs`)

在任务执行流程的生命周期末端（`final_status` 与 `duration_ms` 计算完成后）：

```rust
// 1. 判断是否需要发送通知
let need_notify = match task.execution_policy.notification {
    TaskNotificationPolicy::None => false,
    TaskNotificationPolicy::OnlySuccess => final_status == ExecutionStatus::Succeeded,
    TaskNotificationPolicy::OnlyFailure => final_status != ExecutionStatus::Succeeded,
    TaskNotificationPolicy::All => true,
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

    // 非阻塞异步发送通知
    tokio::spawn(async move {
        if let Err(e) = easyjob_platform::notification::send_system_notification(
            "easyJob",
            Some(&subtitle),
            &body,
        ).await {
            tracing::warn!("Failed to send system notification for task '{}': {:?}", task_name, e);
        }
    });
}
```

---

## 5. 前端 UI 与导入比对实现

### 5.1 任务编辑抽屉 (`TaskDrawer.vue`)

- 在“基本配置与策略”表单中，添加“执行结果通知”单选/下拉选择项。
- 选项列表：
  - `None`: `不通知 (默认)`
  - `OnlySuccess`: `仅成功时通知 (OnlySuccess)`
  - `OnlyFailure`: `仅失败时通知 (OnlyFailure)`
  - `All`: `全部通知 (成功与失败均通知)`
- 初始化空任务模版增加 `notification: 'None'`。

### 5.2 深度比对工具 (`taskDiff.ts`)

比对逻辑新增：
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

### 5.3 三栏手风琴合并弹窗 (`TaskAccordionContent.vue`)

1. **差异标签**：在手风琴标题的头部增加 `isDiff('policy.notification')` 判断，若存在差异则标注黄色“有差异”Tag。
2. **只读对比**：
   - 栏 1（已有配置）展示当前已有策略中文名称。
   - 栏 2（导入配置）展示导入策略中文名称；若与栏 1 不一致，应用黄色边框与琥珀色背景高亮。
3. **编辑合并**：
   - 栏 3（合并结果）提供 `NSelect` 选择器供用户调整。
   - 【采用已有配置】与【还原导入配置】均包含 `notification` 字段的完整覆盖同步。

---

## 6. 测试与验证标准

1. **Rust 单元测试**：
   - `crates/domain/tests/domain_tests.rs`：测试 `TaskNotificationPolicy` 序列化与向后兼容反序列化。
   - `crates/platform/tests/platform_tests.rs`：测试通知参数格式化与转义逻辑。
2. **前端单元测试**：
   - `apps/desktop/tests/taskDiff.test.ts`：测试 `policy.notification` 比对与 `diffFields` 正确性。
3. **全工作区验收指标**：
   - `cargo test --all` 100% 通过。
   - `cargo clippy --workspace --all-targets -- -D warnings` 0 警告。
   - `pnpm -C apps/desktop test` 100% 通过。
   - `pnpm -C apps/desktop run build` 0 错误。
