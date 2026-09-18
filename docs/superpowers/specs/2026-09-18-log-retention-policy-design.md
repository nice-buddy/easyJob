# easyJob 日志保留策略与执行日志重命名系统设计规范

**日期**: 2026-09-18  
**状态**: Approved (已评审确认)  
**作者**: easyJob 核心架构组  

---

## 1. 概述与背景

随着 easyJob 定时与手动任务的长期运行，数据库中累积的历史执行记录（`task_runs`）及其标准输出/错误输出（`run_outputs`）会持续消耗存储空间与查询性能。为了实现轻量、自动化的历史日志生命周期治理，同时优化用户界面语义，本功能提供：

1. **系统全局默认日志保留策略**：在“系统设置”中提供全局默认保留策略（默认为 **7 天**），支持 3 天、7 天、14 天、30 天、90 天、自定义天数以及永久保留（关闭清理）。
2. **任务独立自定义保留策略**：在任务编辑中支持为单个任务独立配置保留策略，提供**跟随系统设置（默认）**、**自定义保留天数**与**永久保留**三种模式。
3. **任务完成后的自动化过期清理**：每次任务执行完毕（包括定时调度与手动立即执行），Agent 守护进程非阻塞评估该任务的保留策略，并清理该任务过期的历史记录及级联终端输出。
4. **菜单文案更新**：将左侧主导航“执行记录”修改为“执行日志”，并同步更新日志页标题与刷新按钮文案。
5. **导入导出与合并比对无缝适配**：任务导出包含保留策略，导入旧版配置自动回退兼容，并在配置比对（`taskDiff`）与三栏合并弹窗中提供差异识别与高亮。

---

## 2. 领域模型与数据结构设计

### 2.1 任务日志保留策略 (`crates/domain/src/policy.rs`)

```rust
use serde::{Deserialize, Serialize};

/// 任务执行日志保留策略
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "days")]
pub enum LogRetentionPolicy {
    /// 跟随系统设置（默认）
    #[serde(rename = "SystemDefault")]
    SystemDefault,
    /// 自定义保留天数（例如 7 天）
    #[serde(rename = "KeepDays")]
    KeepDays(u32),
    /// 永久保留，从不自动清理
    #[serde(rename = "Permanent")]
    Permanent,
}

impl Default for LogRetentionPolicy {
    fn default() -> Self {
        Self::SystemDefault
    }
}
```

在 `ExecutionPolicy` 中增加 `log_retention` 字段：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionPolicy {
    pub concurrency_policy: ConcurrencyPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub retry_policy: RetryPolicy,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub notification: TaskNotificationPolicy,
    /// 任务日志保留策略（通过 Serde 默认值实现存量数据无缝兼容）
    #[serde(default)]
    pub log_retention: LogRetentionPolicy,
}
```

### 2.2 系统级配置模型 (`crates/domain/src/settings.rs` 或 `policy.rs`)

```rust
/// 系统全局配置模型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSettings {
    /// 全局默认日志保留策略，默认 7 天
    pub default_log_retention: SystemLogRetention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "days")]
pub enum SystemLogRetention {
    #[serde(rename = "KeepDays")]
    KeepDays(u32), // 默认 7 天
    #[serde(rename = "Permanent")]
    Permanent,     // 关闭全局自动清理
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            default_log_retention: SystemLogRetention::KeepDays(7),
        }
    }
}
```

### 2.3 前端 TypeScript 类型定义 (`apps/desktop/src/types/task.ts`)

```typescript
export type LogRetentionPolicy =
  | { mode: 'SystemDefault' }
  | { mode: 'KeepDays'; days: number }
  | { mode: 'Permanent' };

export type SystemLogRetention =
  | { mode: 'KeepDays'; days: number }
  | { mode: 'Permanent' };

export interface SystemSettings {
  default_log_retention: SystemLogRetention;
}

export interface ExecutionPolicy {
  concurrency_policy: ConcurrencyPolicy;
  missed_run_policy: MissedRunPolicy;
  retry_policy: RetryPolicy;
  timeout_secs: number | null;
  notification: TaskNotificationPolicy;
  log_retention: LogRetentionPolicy;
}
```

在 `getEmptyTask()` 中初始化：
```typescript
log_retention: { mode: 'SystemDefault' }
```

---

## 3. 持久化层与清理机制设计 (`crates/persistence`)

### 3.1 Settings 仓储实现 (`crates/persistence/src/settings_repo.rs`)

使用 SQLite 已有 `settings` 表结构：
- `key TEXT PRIMARY KEY`
- `value_json TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

```rust
#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get_system_settings(&self) -> Result<SystemSettings>;
    async fn save_system_settings(&self, settings: &SystemSettings) -> Result<()>;
}

pub struct SqliteSettingsRepository {
    pool: SqlitePool,
}
```

- **获取配置**：查询 `key = 'system_settings'`。若无记录，默认返回 `SystemSettings::default()`（即 7 天）。
- **更新配置**：执行 `INSERT INTO settings (key, value_json, updated_at) VALUES ('system_settings', ?, ?) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at`。

### 3.2 历史日志清理接口 (`crates/persistence/src/execution_repo.rs`)

在 `ExecutionRepository` 增加：
```rust
#[async_trait]
pub trait ExecutionRepository: Send + Sync {
    // ... 原有方法保持不变
    async fn purge_expired_runs(&self, task_id: &TaskId, before: DateTime<Utc>) -> Result<u64>;
}
```

SQL 删除逻辑：
```sql
DELETE FROM task_runs 
WHERE task_id = ? 
  AND started_at < ? 
  AND status != '"Running"'
```

- **外键级联清理**：SQLite 中 `run_outputs` 外键为 `REFERENCES task_runs(id) ON DELETE CASCADE`。`easyjob-persistence` 初始化连接池时启用了 `foreign_keys(true)`，删除 `task_runs` 会自动级联物理删除关联的所有终端输出记录。
- **运行安全保护**：排除处于 `Running` 状态的记录，避免影响正在执行中的任务。

---

## 4. Agent 守护进程与 IPC 协议设计

### 4.1 IPC 接口扩展 (`easyjob-ipc` / `AgentRpcHandler`)

在 `apps/agent/src/service.rs` 的 `AgentRpcHandler` 中新增路由：
1. **`settings.get`**
   - 请求：`{}`
   - 响应：返回当前 `SystemSettings` 对象
2. **`settings.set`**
   - 请求：`{ "settings": SystemSettings }`
   - 响应：返回更新后的 `SystemSettings` 对象

在桌面端 `apps/desktop/src-tauri/src/commands.rs` 新增对应 Tauri 命令：
- `get_system_settings`
- `save_system_settings(settings: SystemSettings)`

### 4.2 任务完成后的后置清理 Hook (`apps/agent/src/service.rs`)

在两个执行终点（**定时调度执行** 与 **手动 `task.trigger_now` 执行**）：
在执行记录更新（`e_repo.update_run(&finished_exec).await`）并派发 IPC 事件后：

```rust
// 1. 判定任务有效保留天数
let effective_days: Option<u32> = match task.execution_policy.log_retention {
    LogRetentionPolicy::SystemDefault => {
        let sys = settings_repo.get_system_settings().await.unwrap_or_default();
        match sys.default_log_retention {
            SystemLogRetention::KeepDays(days) => Some(days),
            SystemLogRetention::Permanent => None,
        }
    }
    LogRetentionPolicy::KeepDays(days) => Some(days),
    LogRetentionPolicy::Permanent => None,
};

// 2. 异步触发清理
if let Some(days) = effective_days {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let task_id = task.id;
    let repo = e_repo.clone();
    tokio::spawn(async move {
        match repo.purge_expired_runs(&task_id, cutoff).await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!("任务 [{}] 清理了 {} 条过期执行记录", task_id, count);
                }
            }
            Err(e) => {
                tracing::warn!("任务 [{}] 清理过期执行记录失败: {:?}", task_id, e);
            }
        }
    });
}
```

- **非阻塞保证**：使用 `tokio::spawn` 独立后台清理，不阻塞释放槽位（`release_slot`），保证响应延迟低于 1ms。

---

## 5. 前端界面与导入导出适配

### 5.1 菜单文案统一为“执行日志”

- **`apps/desktop/src/components/layout/AppSidebar.vue`**：
  将导航链接“执行记录”修改为“执行日志”。
- **`apps/desktop/src/views/ExecutionsView.vue`**：
  - 页面主标题修改为“执行日志”。
  - 刷新按钮修改为“刷新日志”。

### 5.2 系统设置页面 (`apps/desktop/src/views/SettingsView.vue`)

新增“日志保留策略”卡片：
- 选择器提供：
  - “保留 3 天”
  - “保留 7 天 (默认推荐)”
  - “保留 14 天”
  - “保留 30 天”
  - “保留 90 天”
  - “自定义天数”
  - “永久保留 (不自动清理)”
- 选择“自定义天数”时展开输入框，允许输入任意正整数（天）。
- 修改时调用 `save_system_settings` 并实时反馈。

### 5.3 任务编辑抽屉 (`apps/desktop/src/components/task/TaskDrawer.vue`)

在“基本配置与策略”的执行策略区域，新增“日志保留策略”配置项：
- 模式下拉框：
  - “跟随系统设置 (默认)”
  - “自定义保留天数”
  - “永久保留 (从不清理)”
- 选中“自定义保留天数”时，展示数字输入框（默认 7，最小 1 天）。

### 5.4 导入导出与合并比对适配

1. **导出 (`TaskExportModal.vue`)**：
   导出的 JSON 文件完整包含任务的 `log_retention` 配置。
2. **导入向后兼容 (`TaskImportModal.vue`)**：
   导入旧版无 `log_retention` 的 JSON 时，自动补齐为 `{ mode: 'SystemDefault' }`。
3. **差异比对 (`apps/desktop/src/utils/taskDiff.ts`)**：
   比较 `ep1.log_retention` 与 `ep2.log_retention`。若模式或天数不同：
   - 将 `'policy.log_retention'` 记录至 `diffFields`
   - 触发 `policyDiff = true`
4. **合并差异弹窗 (`TaskMergeModal.vue` & `TaskAccordionContent.vue`)**：
   - 在执行策略中格式化展示保留策略（如“跟随系统默认”、“保留 14 天”、“永久保留”）。
   - 存在差异时以黄色边框/标签高亮。
   - 最终合并结果中允许用户编辑或通过“采用已有/还原导入”块级一键同步。

---

## 6. 测试与验证方案

### 6.1 单元测试与集成测试
1. **Domain 测试** (`crates/domain/tests`):
   - 验证 `LogRetentionPolicy` 与 `SystemSettings` 的序列化与反序列化。
   - 验证旧版 JSON 反序列化缺省自动回退到 `SystemDefault`。
2. **Persistence 测试** (`crates/persistence/tests`):
   - 测试 `SettingsRepository` 的读取、默认回退与更新覆盖。
   - 测试 `purge_expired_runs`，插入不同时间点与状态的历史运行记录，验证仅指定任务且小于截止时间的非 Running 记录被删除，同时验证关联的 `run_outputs` 被级联删除。
3. **Agent 测试** (`apps/agent/tests`):
   - 测试 `settings.get` 与 `settings.set` IPC 接口。
   - 测试任务完成时过期记录被自动清理。
4. **前端比对测试** (`apps/desktop/tests/taskDiff.test.ts`):
   - 测试 `log_retention` 相同与不同场景下的 diff 检测。
   - 测试缺少 `log_retention` 字段时的兼容 fallback 比对。

### 6.2 质量门禁
- `cargo test --all` 100% 通过。
- `pnpm -C apps/desktop test` 100% 通过。
- `cargo clippy --all-targets -- -D warnings` 0 警告。
- `cargo fmt --check` 0 格式差异。
