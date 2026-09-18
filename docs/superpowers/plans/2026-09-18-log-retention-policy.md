# 日志保留策略与执行日志重命名实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 easyJob 日志生命周期保留策略（系统默认 7 天、任务独立配置、任务完成后非阻塞自动清理 SQLite 运行日志与级联输出流）、将菜单文案“执行记录”更名为“执行日志”，并全流程适配任务导入导出与差异合并。

**Architecture:** 
1. 后端领域层扩展 `LogRetentionPolicy` 与 `SystemSettings` 模型，`ExecutionPolicy` 增补保留策略并通过 Serde default 保持零迁移向前向后兼容；
2. 持久化层新增 `SettingsRepository` 操作 SQLite `settings` 表，并在 `ExecutionRepository` 增加 `purge_expired_runs`，利用 `ON DELETE CASCADE` 物理连带删除 `run_outputs`；
3. Agent 守护进程增加 `settings.get` / `settings.set` IPC 接口，在调度器与立即执行两大执行终点挂载异步清理 Hook；
4. Tauri 层透传设置命令；前端扩展类型、`taskDiff`、系统设置管理、任务编辑抽屉及三栏合并比对高亮，并统一重命名“执行日志”。

**Tech Stack:** Rust (Tokio, sqlx, Serde, SQLite), Tauri v2, Vue 3, TypeScript, Naive UI, TailwindCSS, Vitest.

## Global Constraints

- **向前向后兼容**：存量任务 JSON 及旧导出文件无 `log_retention` 字段时必须无感缺省回退为 `SystemDefault`，不引起反序列化异常。
- **级联删除完整性**：删除 `task_runs` 历史记录必须通过外键级联物理清理 `run_outputs`，杜绝孤儿终端日志。
- **非阻塞性能保障**：任务完成后的日志清理逻辑必须在 `tokio::spawn` 独立后台任务中执行，绝不延迟任务槽位释放（`release_slot`）与 IPC 事件派发。
- **文案一致性**：左侧主导航链接、日志列表页主标题、刷新按钮文案统一为“执行日志”。
- **质量门禁**：
  - `cargo test --all` 100% 通过；
  - `cargo clippy --all-targets -- -D warnings` 0 警告；
  - `cargo fmt --check` 合规；
  - `pnpm -C apps/desktop test` 100% 通过。

---

### Task 1: Domain 领域模型与日志保留策略扩展 (`crates/domain`)

**Files:**
- Modify: `crates/domain/src/policy.rs`
- Modify: `crates/domain/src/lib.rs`
- Test: `crates/domain/tests/domain_tests.rs`

**Interfaces:**
- Consumes: `easyjob_domain::policy::ExecutionPolicy`
- Produces: `LogRetentionPolicy`, `SystemSettings`, `SystemLogRetention`

- [ ] **Step 1: 编写失败测试用例**

在 `crates/domain/tests/domain_tests.rs` 中新增针对 `LogRetentionPolicy` 与 `SystemSettings` 的序列化、反序列化及缺省兼容性测试：

```rust
#[test]
fn test_log_retention_policy_defaults_and_serde() {
    use easyjob_domain::policy::{ExecutionPolicy, LogRetentionPolicy, SystemLogRetention, SystemSettings};

    // 1. 验证默认策略为 SystemDefault
    let default_policy = LogRetentionPolicy::default();
    assert_eq!(default_policy, LogRetentionPolicy::SystemDefault);

    // 2. 验证序列化与反序列化
    let keep_7 = LogRetentionPolicy::KeepDays(7);
    let json_keep_7 = serde_json::to_string(&keep_7).unwrap();
    assert_eq!(json_keep_7, r#"{"mode":"KeepDays","days":7}"#);
    let parsed_keep_7: LogRetentionPolicy = serde_json::from_str(&json_keep_7).unwrap();
    assert_eq!(parsed_keep_7, keep_7);

    let perm = LogRetentionPolicy::Permanent;
    let json_perm = serde_json::to_string(&perm).unwrap();
    assert_eq!(json_perm, r#"{"mode":"Permanent"}"#);
    let parsed_perm: LogRetentionPolicy = serde_json::from_str(&json_perm).unwrap();
    assert_eq!(parsed_perm, perm);

    // 3. 验证旧版 ExecutionPolicy JSON 缺失 log_retention 时的向后兼容性
    let legacy_json = r#"{
        "concurrency_policy": "SkipIfRunning",
        "missed_run_policy": "RunOnce",
        "retry_policy": { "max_retries": 0, "delay_secs": 0 },
        "timeout_secs": 3600,
        "notification": "None"
    }"#;
    let ep: ExecutionPolicy = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(ep.log_retention, LogRetentionPolicy::SystemDefault);

    // 4. 验证 SystemSettings 默认值为 KeepDays(7)
    let sys_default = SystemSettings::default();
    assert_eq!(sys_default.default_log_retention, SystemLogRetention::KeepDays(7));
    let sys_json = serde_json::to_string(&sys_default).unwrap();
    let parsed_sys: SystemSettings = serde_json::from_str(&sys_json).unwrap();
    assert_eq!(parsed_sys, sys_default);
}
```

- [ ] **Step 2: 运行测试验证失败**

运行：`cargo test -p easyjob-domain --test domain_tests test_log_retention_policy_defaults_and_serde`
预期：编译失败，提示 `LogRetentionPolicy` / `SystemSettings` 未定义。

- [ ] **Step 3: 编写领域模型实现**

编辑 `crates/domain/src/policy.rs`，添加 `LogRetentionPolicy`、`SystemSettings` 和 `SystemLogRetention`，并在 `ExecutionPolicy` 中添加字段：

```rust
/// 任务日志保留策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// 系统全局配置模型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSettings {
    /// 全局默认日志保留策略，默认 7 天
    pub default_log_retention: SystemLogRetention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "days")]
pub enum SystemLogRetention {
    #[serde(rename = "KeepDays")]
    KeepDays(u32),
    #[serde(rename = "Permanent")]
    Permanent,
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            default_log_retention: SystemLogRetention::KeepDays(7),
        }
    }
}
```

在 `ExecutionPolicy` 结构体中添加字段：
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionPolicy {
    pub concurrency_policy: ConcurrencyPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub retry_policy: RetryPolicy,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub notification: TaskNotificationPolicy,
    #[serde(default)]
    pub log_retention: LogRetentionPolicy,
}
```

编辑 `crates/domain/src/lib.rs`，导出新增的结构与枚举：
```rust
pub use policy::{
    ConcurrencyPolicy, ExecutionPolicy, LogRetentionPolicy, MissedRunPolicy, RetryPolicy,
    SystemLogRetention, SystemSettings, TaskNotificationPolicy,
};
```

- [ ] **Step 4: 运行测试验证通过**

运行：`cargo test -p easyjob-domain`
预期：所有测试 100% 通过。

- [ ] **Step 5: 提交**

```bash
git add crates/domain/
git commit -m "feat(domain): add LogRetentionPolicy and SystemSettings with backward compatibility"
```

---

### Task 2: Persistence 仓储扩展与日志清理方法 (`crates/persistence`)

**Files:**
- Create: `crates/persistence/src/settings_repo.rs`
- Modify: `crates/persistence/src/execution_repo.rs`
- Modify: `crates/persistence/src/lib.rs`
- Test: `crates/persistence/tests/repository_tests.rs`

**Interfaces:**
- Consumes: `easyjob_domain::{SystemSettings, SystemLogRetention}`, `easyjob_common::TaskId`
- Produces: `SettingsRepository`, `SqliteSettingsRepository`, `ExecutionRepository::purge_expired_runs`

- [ ] **Step 1: 编写失败测试用例**

在 `crates/persistence/tests/repository_tests.rs` 中新增针对 `SettingsRepository` 与 `purge_expired_runs` 的集成测试：

```rust
#[tokio::test]
async fn test_settings_repo_crud() {
    let pool = setup_test_db().await;
    let repo = SqliteSettingsRepository::new(pool);

    // 1. 初始获取返回默认值 (KeepDays(7))
    let settings = repo.get_system_settings().await.unwrap();
    assert_eq!(settings.default_log_retention, SystemLogRetention::KeepDays(7));

    // 2. 更新设置为 30 天
    let updated = SystemSettings {
        default_log_retention: SystemLogRetention::KeepDays(30),
    };
    repo.save_system_settings(&updated).await.unwrap();

    let fetched = repo.get_system_settings().await.unwrap();
    assert_eq!(fetched.default_log_retention, SystemLogRetention::KeepDays(30));

    // 3. 更新为永久保留
    let perm = SystemSettings {
        default_log_retention: SystemLogRetention::Permanent,
    };
    repo.save_system_settings(&perm).await.unwrap();
    let fetched_perm = repo.get_system_settings().await.unwrap();
    assert_eq!(fetched_perm.default_log_retention, SystemLogRetention::Permanent);
}

#[tokio::test]
async fn test_purge_expired_runs_and_cascade_outputs() {
    let pool = setup_test_db().await;
    let exec_repo = SqliteExecutionRepository::new(pool.clone());
    let task_repo = SqliteTaskRepository::new(pool.clone());

    // 准备一个任务
    let task = make_dummy_task("purge-test-task");
    task_repo.save(&task).await.unwrap();

    let now = chrono::Utc::now();
    let ten_days_ago = now - chrono::Duration::days(10);
    let two_days_ago = now - chrono::Duration::days(2);

    // 1. 创建 10 天前的已完成运行及输出
    let mut old_run = Execution::new(task.id, None, Some(ten_days_ago));
    old_run.status = ExecutionStatus::Succeeded;
    old_run.started_at = ten_days_ago;
    old_run.finished_at = Some(ten_days_ago + chrono::Duration::seconds(5));
    exec_repo.create_run(&old_run).await.unwrap();
    exec_repo.append_output(&old_run.id, "stdout", "old log content").await.unwrap();

    // 2. 创建 2 天前的已完成运行及输出
    let mut recent_run = Execution::new(task.id, None, Some(two_days_ago));
    recent_run.status = ExecutionStatus::Succeeded;
    recent_run.started_at = two_days_ago;
    recent_run.finished_at = Some(two_days_ago + chrono::Duration::seconds(5));
    exec_repo.create_run(&recent_run).await.unwrap();
    exec_repo.append_output(&recent_run.id, "stdout", "recent log content").await.unwrap();

    // 3. 创建 10 天前但仍在 Running 状态的运行
    let mut running_old_run = Execution::new(task.id, None, Some(ten_days_ago));
    running_old_run.status = ExecutionStatus::Running;
    running_old_run.started_at = ten_days_ago;
    exec_repo.create_run(&running_old_run).await.unwrap();

    // 4. 以 7 天前为 cutoff 执行清理
    let cutoff = now - chrono::Duration::days(7);
    let deleted_count = exec_repo.purge_expired_runs(&task.id, cutoff).await.unwrap();
    assert_eq!(deleted_count, 1, "只应清理 1 条 10 天前已结束的运行");

    // 5. 验证已删除旧运行与输出
    assert!(exec_repo.find_run_by_id(&old_run.id).await.unwrap().is_none());
    let old_outputs = exec_repo.get_outputs(&old_run.id).await.unwrap();
    assert!(old_outputs.is_empty(), "外键级联删除输出记录");

    // 6. 验证近期的运行与仍在运行中的记录完好
    assert!(exec_repo.find_run_by_id(&recent_run.id).await.unwrap().is_some());
    assert!(exec_repo.find_run_by_id(&running_old_run.id).await.unwrap().is_some());
}
```

- [ ] **Step 2: 运行测试验证失败**

运行：`cargo test -p easyjob-persistence --test repository_tests`
预期：编译失败，缺少 `SqliteSettingsRepository` 与 `purge_expired_runs`。

- [ ] **Step 3: 编写 SettingsRepository 与 ExecutionRepository::purge_expired_runs 实现**

创建 `crates/persistence/src/settings_repo.rs`：
```rust
use async_trait::async_trait;
use chrono::Utc;
use easyjob_common::{Error, Result};
use easyjob_domain::{SystemLogRetention, SystemSettings};
use sqlx::{Row, SqlitePool};

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get_system_settings(&self) -> Result<SystemSettings>;
    async fn save_system_settings(&self, settings: &SystemSettings) -> Result<()>;
}

pub struct SqliteSettingsRepository {
    pool: SqlitePool,
}

impl SqliteSettingsRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn get_system_settings(&self) -> Result<SystemSettings> {
        let row = sqlx::query("SELECT value_json FROM settings WHERE key = 'system_settings'")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        match row {
            Some(r) => {
                let json_str: String = r.get("value_json");
                let settings: SystemSettings = serde_json::from_str(&json_str)?;
                Ok(settings)
            }
            None => Ok(SystemSettings::default()),
        }
    }

    async fn save_system_settings(&self, settings: &SystemSettings) -> Result<()> {
        let json_str = serde_json::to_string(settings)?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO settings (key, value_json, updated_at)
             VALUES ('system_settings', ?, ?)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
        )
        .bind(json_str)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(())
    }
}
```

在 `crates/persistence/src/execution_repo.rs` 中的 `ExecutionRepository` trait 增加：
```rust
async fn purge_expired_runs(&self, task_id: &TaskId, before: chrono::DateTime<Utc>) -> Result<u64>;
```

在 `SqliteExecutionRepository` 中实现该方法：
```rust
    async fn purge_expired_runs(&self, task_id: &TaskId, before: chrono::DateTime<Utc>) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM task_runs
             WHERE task_id = ?
               AND started_at < ?
               AND status != '\"Running\"'",
        )
        .bind(task_id.to_string())
        .bind(before.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }
```

在 `crates/persistence/src/lib.rs` 导出：
```rust
pub mod db;
pub mod execution_repo;
pub mod recovery;
pub mod settings_repo;
pub mod task_repo;

pub use db::{init_pool, run_migrations, DbPool};
pub use execution_repo::{ExecutionOutputRecord, ExecutionRepository, SqliteExecutionRepository};
pub use recovery::recover_dangling_executions;
pub use settings_repo::{SettingsRepository, SqliteSettingsRepository};
pub use task_repo::{SqliteTaskRepository, TaskRepository};
```

- [ ] **Step 4: 运行测试验证通过**

运行：`cargo test -p easyjob-persistence`
预期：所有测试 100% 通过。

- [ ] **Step 5: 提交**

```bash
git add crates/persistence/
git commit -m "feat(persistence): implement SettingsRepository and purge_expired_runs with cascade delete"
```

---

### Task 3: Agent 守护进程 IPC 与任务完成清理 Hook (`apps/agent`)

**Files:**
- Modify: `apps/agent/src/service.rs`
- Test: `apps/agent/tests/service_tests.rs`

**Interfaces:**
- Consumes: `SettingsRepository`, `SqliteSettingsRepository`, `ExecutionRepository::purge_expired_runs`, `LogRetentionPolicy`
- Produces: `settings.get` / `settings.set` IPC 路由, 任务完成非阻塞日志自动清理

- [ ] **Step 1: 编写失败测试用例**

在 `apps/agent/tests/service_tests.rs` 中新增针对 `settings.get` / `settings.set` IPC 以及任务完成后自动触发日志清理的测试：

```rust
#[tokio::test]
async fn test_agent_settings_ipc() {
    let (handler, _dir) = setup_test_agent_handler().await;

    // 1. settings.get
    let req = IpcRequest::new("settings.get", serde_json::json!({}));
    let res = handler.handle_request(req).await;
    assert!(res.success);
    let settings: easyjob_domain::SystemSettings = serde_json::from_value(res.data.unwrap()).unwrap();
    assert_eq!(settings.default_log_retention, easyjob_domain::SystemLogRetention::KeepDays(7));

    // 2. settings.set
    let updated = easyjob_domain::SystemSettings {
        default_log_retention: easyjob_domain::SystemLogRetention::KeepDays(14),
    };
    let set_req = IpcRequest::new("settings.set", serde_json::json!({ "settings": updated }));
    let set_res = handler.handle_request(set_req).await;
    assert!(set_res.success);

    // 3. 再次 get 验证更新生效
    let verify_req = IpcRequest::new("settings.get", serde_json::json!({}));
    let verify_res = handler.handle_request(verify_req).await;
    let current: easyjob_domain::SystemSettings = serde_json::from_value(verify_res.data.unwrap()).unwrap();
    assert_eq!(current.default_log_retention, easyjob_domain::SystemLogRetention::KeepDays(14));
}
```

- [ ] **Step 2: 运行测试验证失败**

运行：`cargo test -p easyjob-agent --test service_tests test_agent_settings_ipc`
预期：失败，提示方法 `settings.get` not found。

- [ ] **Step 3: 实现 Agent 守护进程设置管理与清理 Hook**

在 `apps/agent/src/service.rs` 中：
1. 为 `AgentService` 与 `AgentRpcHandler` 引入 `settings_repo: Arc<SqliteSettingsRepository>`。
2. 在 `AgentRpcHandler::handle_request` 中增加：
```rust
            "settings.get" => match self.settings_repo.get_system_settings().await {
                Ok(s) => match serde_json::to_value(s) {
                    Ok(v) => IpcResponse::success(req.id, v),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                },
                Err(e) => IpcResponse::error(req.id, e.to_string()),
            },
            "settings.set" => {
                let settings_val = if req.params.is_object() && req.params.get("settings").is_some() {
                    &req.params["settings"]
                } else {
                    &req.params
                };
                let settings: easyjob_domain::SystemSettings = match serde_json::from_value(settings_val.clone()) {
                    Ok(s) => s,
                    Err(e) => return IpcResponse::error(req.id, format!("Invalid settings parameter: {}", e)),
                };
                match self.settings_repo.save_system_settings(&settings).await {
                    Ok(()) => match serde_json::to_value(&settings) {
                        Ok(v) => IpcResponse::success(req.id, v),
                        Err(e) => IpcResponse::error(req.id, e.to_string()),
                    },
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
```

3. 提炼并接入异步清理辅助函数：
```rust
pub(crate) fn trigger_log_retention_purge(
    task: &easyjob_domain::task::Task,
    settings_repo: Arc<SqliteSettingsRepository>,
    exec_repo: Arc<SqliteExecutionRepository>,
) {
    let policy = task.execution_policy.log_retention;
    let task_id = task.id;

    tokio::spawn(async move {
        let retention_days: Option<u32> = match policy {
            easyjob_domain::policy::LogRetentionPolicy::SystemDefault => {
                let sys = settings_repo.get_system_settings().await.unwrap_or_default();
                match sys.default_log_retention {
                    easyjob_domain::policy::SystemLogRetention::KeepDays(days) => Some(days),
                    easyjob_domain::policy::SystemLogRetention::Permanent => None,
                }
            }
            easyjob_domain::policy::LogRetentionPolicy::KeepDays(days) => Some(days),
            easyjob_domain::policy::LogRetentionPolicy::Permanent => None,
        };

        if let Some(days) = retention_days {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
            match exec_repo.purge_expired_runs(&task_id, cutoff).await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Purged {} expired runs for task {}", count, task_id);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to purge expired runs for task {}: {:?}", task_id, e);
                }
            }
        }
    });
}
```

在两处任务执行完成更新记录后调用该辅助函数：
- 调度器 `dispatcher_handle`（更新 `exec` 后）
- `task.trigger_now`（更新 `finished_exec` 后）

- [ ] **Step 4: 运行测试验证通过**

运行：`cargo test -p easyjob-agent`
预期：所有测试 100% 通过。

- [ ] **Step 5: 提交**

```bash
git add apps/agent/
git commit -m "feat(agent): support settings IPC and non-blocking log retention purge on task completion"
```

---

### Task 4: Tauri 命令与原生系统桥接 (`apps/desktop/src-tauri`)

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Agent IPC `settings.get` / `settings.set`
- Produces: Tauri 命令 `get_system_settings`, `save_system_settings`

- [ ] **Step 1: 编写 Tauri 命令**

在 `apps/desktop/src-tauri/src/commands.rs` 中添加：
```rust
use easyjob_domain::SystemSettings;

#[tauri::command]
pub async fn get_system_settings(
    manager: State<'_, Arc<AgentManager>>,
) -> Result<SystemSettings, String> {
    let val = manager.call("settings.get", serde_json::json!({})).await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_system_settings(
    settings: SystemSettings,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<SystemSettings, String> {
    let val = manager
        .call("settings.set", serde_json::json!({ "settings": settings }))
        .await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: 在 Tauri generate_handler 中注册**

在 `apps/desktop/src-tauri/src/lib.rs` 中的 `generate_handler!` 列表加入：
- `get_system_settings`
- `save_system_settings`

同步将 `apps/desktop/src-tauri/src/lib.rs` 中的相关注释中的“执行记录”更新为“执行日志”。

- [ ] **Step 3: 运行 Rust 全局检查**

运行：`cargo test --all && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
预期：全部编译通过，0 警告，格式完全合规。

- [ ] **Step 4: 提交**

```bash
git add apps/desktop/src-tauri/
git commit -m "feat(tauri): add get_system_settings and save_system_settings commands"
```

---

### Task 5: 前端类型、API 与 taskDiff 比对适配 (`apps/desktop`)

**Files:**
- Modify: `apps/desktop/src/types/task.ts`
- Modify: `apps/desktop/src/services/tauri.ts`
- Modify: `apps/desktop/src/utils/taskDiff.ts`
- Test: `apps/desktop/tests/taskDiff.test.ts`

**Interfaces:**
- Consumes: `Task`, `ExecutionPolicy`, `SystemSettings`
- Produces: `compareTasks` detecting `policy.log_retention` differences

- [ ] **Step 1: 编写 taskDiff 失败测试用例**

在 `apps/desktop/tests/taskDiff.test.ts` 中新增针对 `log_retention` 差异比对及向后兼容的测试用例：

```typescript
it('should detect differences in policy.log_retention', () => {
  const taskA = getEmptyTask();
  taskA.execution_policy.log_retention = { mode: 'SystemDefault' };

  const taskB = getEmptyTask();
  taskB.execution_policy.log_retention = { mode: 'KeepDays', days: 14 };

  const res1 = compareTasks(taskA, taskB);
  expect(res1.hasDiff).toBe(true);
  expect(res1.policyDiff).toBe(true);
  expect(res1.diffFields.has('policy.log_retention')).toBe(true);

  // 相同 KeepDays 但天数不同
  const taskC = getEmptyTask();
  taskC.execution_policy.log_retention = { mode: 'KeepDays', days: 30 };
  const res2 = compareTasks(taskB, taskC);
  expect(res2.diffFields.has('policy.log_retention')).toBe(true);

  // 缺失 log_retention 时的 fallback 兼容
  const legacyTask = getEmptyTask();
  delete (legacyTask.execution_policy as any).log_retention;
  const res3 = compareTasks(taskA, legacyTask);
  expect(res3.diffFields.has('policy.log_retention')).toBe(false);
});
```

- [ ] **Step 2: 运行测试验证失败**

运行：`pnpm -C apps/desktop test`
预期：失败，提示 `log_retention` 不存在或未比对。

- [ ] **Step 3: 更新类型、API 与 taskDiff 实现**

1. 在 `apps/desktop/src/types/task.ts` 中新增类型：
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
```
在 `ExecutionPolicy` 中增加 `log_retention: LogRetentionPolicy;`。
在 `getEmptyTask()` 中初始化 `log_retention: { mode: 'SystemDefault' }`。

2. 在 `apps/desktop/src/services/tauri.ts` 中添加：
```typescript
import type { SystemSettings } from '../types/task';

export async function getSystemSettings(): Promise<SystemSettings> {
  return await invoke<SystemSettings>('get_system_settings');
}

export async function saveSystemSettings(settings: SystemSettings): Promise<SystemSettings> {
  return await invoke<SystemSettings>('save_system_settings', { settings });
}
```

3. 在 `apps/desktop/src/utils/taskDiff.ts` 中增加比对：
```typescript
  const r1 = ep1.log_retention || { mode: 'SystemDefault' };
  const r2 = ep2.log_retention || { mode: 'SystemDefault' };
  if (JSON.stringify(r1) !== JSON.stringify(r2)) {
    diffFields.add('policy.log_retention');
  }

  const policyDiff =
    diffFields.has('policy.concurrency_policy') ||
    diffFields.has('policy.missed_run_policy') ||
    diffFields.has('policy.timeout_secs') ||
    diffFields.has('policy.retry_max_retries') ||
    diffFields.has('policy.retry_delay_secs') ||
    diffFields.has('policy.notification') ||
    diffFields.has('policy.log_retention');
```

- [ ] **Step 4: 运行测试验证通过**

运行：`pnpm -C apps/desktop test`
预期：全部前端单元测试 100% 通过。

- [ ] **Step 5: 提交**

```bash
git add apps/desktop/
git commit -m "feat(desktop): add LogRetentionPolicy types, tauri APIs, and diff comparison"
```

---

### Task 6: 前端界面、文案重命名与全流程集成 (`apps/desktop`)

**Files:**
- Modify: `apps/desktop/src/components/layout/AppSidebar.vue`
- Modify: `apps/desktop/src/views/ExecutionsView.vue`
- Modify: `apps/desktop/src/views/SettingsView.vue`
- Modify: `apps/desktop/src/components/task/TaskDrawer.vue`
- Modify: `apps/desktop/src/components/task/TaskAccordionContent.vue`
- Modify: `apps/desktop/src/components/task/TaskImportModal.vue`
- Modify: `apps/desktop/src/components/task/TaskMergeModal.vue`

- [ ] **Step 1: 更新文案为“执行日志”**

1. `apps/desktop/src/components/layout/AppSidebar.vue`：
   将 `<span>执行记录</span>` 修改为 `<span>执行日志</span>`。
2. `apps/desktop/src/views/ExecutionsView.vue`：
   - 标题 `<h1 class="text-xl font-bold">执行记录</h1>` 修改为 `<h1 class="text-xl font-bold">执行日志</h1>`。
   - 刷新按钮文案“刷新记录”修改为“刷新日志”。

- [ ] **Step 2: 在系统设置页添加日志保留策略卡片**

在 `apps/desktop/src/views/SettingsView.vue` 中引入 `getSystemSettings` 与 `saveSystemSettings`：
- 加载系统设置并绑定至响应式变量；
- 下拉支持：3 天、7 天 (默认推荐)、14 天、30 天、90 天、自定义天数、永久保留；
- 当选择自定义天数时，展示正整数数字输入框（天）；
- 变更后调用 `saveSystemSettings` 并通过 `message.success` 反馈。

- [ ] **Step 3: 任务编辑抽屉支持日志保留策略**

在 `apps/desktop/src/components/task/TaskDrawer.vue` 的“基本配置与策略”中增加“日志保留策略”表单项：
- 模式下拉框：
  - 跟随系统设置 (默认) (`SystemDefault`)
  - 自定义保留天数 (`KeepDays`)
  - 永久保留 (从不清理) (`Permanent`)
- 当选中 `KeepDays` 时，显示天数输入框（默认 7，最小 1 天）；
- 监听 `props.task` 变更时，如缺失 `log_retention` 自动补充缺省值 `{ mode: 'SystemDefault' }`。

- [ ] **Step 4: 差异比对、合并弹窗与导入兼容**

1. `apps/desktop/src/components/task/TaskAccordionContent.vue`：
   - 增加日志保留策略格式化函数（例如：“跟随系统设置”、“保留 7 天”、“永久保留”）；
   - 在执行策略折叠面板中展示保留策略；
   - 差异识别条件增加 `isDiff('policy.log_retention')`，在只读列展示高亮；
   - 可编辑列提供下拉与天数输入框。
2. `apps/desktop/src/components/task/TaskImportModal.vue`：
   - 解析导入任务 JSON 时，若无 `log_retention` 字段，默认赋予 `{ mode: 'SystemDefault' }`。
3. `apps/desktop/src/components/task/TaskMergeModal.vue`：
   - 确保块级复制 `copyExistingSection('policy')` 与 `restoreImportedSection('policy')` 正确包含 `log_retention` 深度克隆。

- [ ] **Step 5: 全量验证质量门禁**

运行：
1. `cargo test --all`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo fmt --check`
4. `pnpm -C apps/desktop test`
5. `pnpm -C apps/desktop build`

预期：所有测试通过，0 警告，前端正常构建。

- [ ] **Step 6: 提交**

```bash
git add apps/desktop/
git commit -m "feat(ui): rename execution records to execution logs, add retention settings, and update merge drawer"
```
