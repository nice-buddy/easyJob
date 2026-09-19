# 任务复制 + 任务列表调度概览 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在任务列表新增「复制任务」入口（副本默认停用、保存后才落库），并让任务列表展示每个触发器的中文摘要、下次触发时间（Fuzzy 可手动重摇）与任务最近一次执行结果。

**Architecture:** 下次触发时间只从调度队列快照读取（`ScheduleQueue` 是唯一真源，不持久化、不进 `Task` 领域模型）；agent 新增 `task.overview` / `trigger.reroll` 两个 IPC 方法，其中重摇通过带 oneshot 回执的 `SchedulerCommand::RerollTrigger` 在调度循环内完成，保证「只重算目标触发器、同任务其它触发器下次时间不变」；最近一次执行由 `ExecutionRepository::find_latest_run_per_task` 一次窗口函数查询批量返回；Tauri 透传两个命令，前端在 `taskStore` 维护 `scheduleOverview` 快照并在 `execution.finished` 后刷新。

**Tech Stack:** Rust（tokio / sqlx / chrono / croner / serde）、Tauri 2、Vue 3 + TypeScript + naive-ui + Pinia + vitest

---

## Global Constraints

- **平台**：整个平台只考虑 macOS 与 Windows。不得新增 `#[cfg(target_os = "linux")]` 之类的分支，也不得为其它平台写适配或条件编译。
- **不持久化 `next_fire_at`**：不新增任何列、不写库，`Task` / `Trigger` 领域模型与 IPC 的 `Task` 线格式保持不变。
- **不修改 `describeTrigger` 既有输出**：任务对比视图与既有测试依赖它，本次只新增 `describeTriggerShort`。
- **质量门禁（每个后端任务结束、以及 Task 12）必须全绿**，当前 `master`（HEAD=`6124c7f`）为全绿基线：

```bash
cargo test --all
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
pnpm -C apps/desktop test
pnpm -C apps/desktop run build
```

- `-D warnings` 下 clippy 会检查 test target。写测试注释时避免在文档注释里使用续行列表（会触发 `clippy::doc_lazy_continuation`）；用连续句子或行内注释表达。
- **提交信息用中文说明「为什么」**。不要 `git push`。

## 设计偏差记录（执行前必读）

设计文档 §5.2 原给出的命令签名是 `RerollTrigger { task_id, trigger_id }`，但 §7 要求「任务未注册到调度器 / 任务或触发器已停用」时 IPC 必须返回错误，而「是否已注册」只有调度循环里的 `registered_tasks` 知道。纯单向 `mpsc::Sender` 命令无法把错误与新时间回传，因此本计划在命令里追加一个 oneshot 回执字段：

```rust
RerollTrigger {
    task_id: TaskId,
    trigger_id: TriggerId,
    reply: RerollReply, // oneshot::Sender<Result<Option<DateTime<Utc>>, String>>
}
```

**设计文档已同步更新**（§5.2 现包含 `reply` 字段与原因说明），因此这一项已不再是计划对设计的偏离，保留在此仅作背景。

另有两处设计文档措辞冲突也已在设计文档中就地修正，本计划按修正后的内容执行：
- §6.3 的周几分隔符：统一为 `、`（`Weekly` 与 `Fuzzy Weekly` 一致，示例为 `每周一、周五 09:00-10:00 之间随机`）。
- §9 原写「不加 migration」与 §5.3 要求新增 `(task_id, started_at)` 索引冲突：已明确为「不为 `next_fire_at` 加列/不加相关 migration」，而索引 migration 仍需新增。

---

## File Structure

**Create**

- `migrations/20260919000000_add_task_runs_task_started_index.sql` — `task_runs(task_id, started_at)` 复合索引，支撑「每个任务最近一次执行」窗口函数查询。
- `apps/desktop/tests/triggerShort.test.ts` — `describeTriggerShort` 表格全量断言 + 时间格式化（跨年 / 相对时间）单测。
- `apps/desktop/tests/taskClone.test.ts` — `cloneTaskForDuplicate` 单测（新 id、子 id 重建、停用、原对象不被修改）。

**Modify**

- `crates/scheduler/src/queue.rs:87-97` — 新增 `next_fire_by_trigger` / `take_task_items`；文末新增 `#[cfg(test)] mod tests`。
- `crates/scheduler/src/scheduler.rs:13-19`、`:95-132`、`:194-197` — 新增 `RerollReply` 类型别名、`SchedulerCommand::RerollTrigger`、`run` 处理分支与 `reroll_trigger` 函数。
- `crates/scheduler/tests/scheduler_loop_tests.rs:1-13`、文末 — 新增 reroll 集成测试与夹具辅助函数。
- `crates/persistence/src/execution_repo.rs:1-5`、`:14-27`、`:145-164` — 新增 `find_latest_run_per_task` trait 方法与 SQLite 实现。
- `crates/persistence/tests/repository_tests.rs` — 新增批量查询与并列 `started_at` 测试。
- `apps/agent/src/service.rs:17-24`、`:25-42`、`:54-65`、`:104-115`、`:445-458`、`:460-499`、`:926-932` — 持有调度队列 `Arc`，新增 `task.overview` / `trigger.reroll` IPC 与 DTO。
- `apps/agent/tests/service_tests.rs:635-670`、文末 — 测试夹具改为真实 `Scheduler::run`，新增两个 IPC 测试。
- `apps/desktop/src-tauri/src/commands.rs:1-8`、文末 — 新增 `task_overview` / `reroll_trigger` 两个 `#[tauri::command]`。
- `apps/desktop/src-tauri/src/lib.rs:94-108` — 注册两个新命令。
- `apps/desktop/src/services/tauri.ts:1-4`、文末 — 新增 `getTaskOverview` / `rerollTrigger`。
- `apps/desktop/src/types/task.ts:1-3`、`179-249` 之后、`286-305` 之后 — overview 类型、`describeTriggerShort`、时间格式化工具、`cloneTaskForDuplicate`。
- `apps/desktop/src/stores/taskStore.ts:1-65` — `scheduleOverview` / `loadOverview` / `rerollTrigger` / `execution.finished` 订阅。
- `apps/desktop/src/components/task/TaskDrawer.vue:5-35`、`:174-175` — 新增 `mode` prop 与「复制任务」标题。
- `apps/desktop/src/views/TasksView.vue:1-11`、`:20-58`、`:174-230` — 复制按钮、触发器摘要行、下次时间与重摇按钮、上次执行行。
- `apps/desktop/tests/stores.test.ts:13-30`、文末 — mock 补全新 API，新增 overview/reroll store 测试。
- `apps/desktop/tests/taskDrawer.test.ts:13-19`、`:57-67` — mock 补全 + `mode` prop 断言。
- `apps/desktop/tests/views.test.ts:32-48` — tauri mock 补全新 API。

---

### Task 1: `ScheduleQueue` 只读查询与按任务取出

**Files:**

- Modify: `crates/scheduler/src/queue.rs:87-97`
- Test: `crates/scheduler/src/queue.rs`（文末新增 `#[cfg(test)] mod tests`）

**Interfaces:**

- Consumes: `ScheduledItem { task_id, trigger_id, next_fire_at, generation }`、`ScheduleQueue::{is_valid, current_generation}`
- Produces: `ScheduleQueue::next_fire_by_trigger() -> HashMap<(TaskId, TriggerId), DateTime<Utc>>`、`ScheduleQueue::take_task_items(&TaskId) -> Vec<ScheduledItem>`

- [ ] **Step 1: 编写失败测试**

在 `crates/scheduler/src/queue.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn base() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-19T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn next_fire_by_trigger_returns_earliest_valid_entry_per_trigger() {
        let mut queue = ScheduleQueue::new();
        let task_a = TaskId::new();
        let task_b = TaskId::new();
        let trigger_x = TriggerId::new();
        let trigger_y = TriggerId::new();
        let t = base();
        let mk = |task_id: TaskId, trigger_id: TriggerId, offset: i64, generation: u64| {
            ScheduledItem {
                task_id,
                trigger_id,
                next_fire_at: t + Duration::seconds(offset),
                generation,
            }
        };

        // 同一触发器两条有效条目 → 取最早的一条
        queue.push(mk(task_a, trigger_x, 60, 1));
        queue.push(mk(task_a, trigger_x, 30, 1));
        // 同任务的另一个触发器
        queue.push(mk(task_a, trigger_y, 120, 1));
        // 另一个任务
        queue.push(mk(task_b, trigger_x, 10, 1));

        let map = queue.next_fire_by_trigger();
        assert_eq!(map.len(), 3);
        assert_eq!(map[&(task_a, trigger_x)], t + Duration::seconds(30));
        assert_eq!(map[&(task_a, trigger_y)], t + Duration::seconds(120));
        assert_eq!(map[&(task_b, trigger_x)], t + Duration::seconds(10));
    }

    #[test]
    fn next_fire_by_trigger_ignores_stale_generation() {
        let mut queue = ScheduleQueue::new();
        let task_id = TaskId::new();
        let trigger_id = TriggerId::new();
        let t = base();

        queue.push(ScheduledItem {
            task_id,
            trigger_id,
            next_fire_at: t + Duration::seconds(5),
            generation: 1,
        });
        queue.bump_generation(&task_id); // 旧条目失效
        queue.push(ScheduledItem {
            task_id,
            trigger_id,
            next_fire_at: t + Duration::seconds(90),
            generation: 2,
        });

        let map = queue.next_fire_by_trigger();
        assert_eq!(map.len(), 1);
        assert_eq!(map[&(task_id, trigger_id)], t + Duration::seconds(90));
    }

    #[test]
    fn take_task_items_removes_only_target_task_and_keeps_others() {
        let mut queue = ScheduleQueue::new();
        let task_a = TaskId::new();
        let task_b = TaskId::new();
        let t = base();

        queue.push(ScheduledItem {
            task_id: task_a,
            trigger_id: TriggerId::new(),
            next_fire_at: t + Duration::seconds(1),
            generation: 1,
        });
        queue.push(ScheduledItem {
            task_id: task_a,
            trigger_id: TriggerId::new(),
            next_fire_at: t + Duration::seconds(2),
            generation: 1,
        });
        queue.push(ScheduledItem {
            task_id: task_b,
            trigger_id: TriggerId::new(),
            next_fire_at: t + Duration::seconds(3),
            generation: 1,
        });

        let taken = queue.take_task_items(&task_a);
        assert_eq!(taken.len(), 2);
        assert!(taken.iter().all(|item| item.task_id == task_a));
        assert_eq!(queue.len(), 1);
        let remaining = queue.peek().expect("task_b item remains");
        assert_eq!(remaining.task_id, task_b);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

运行：`cargo test -p easyjob-scheduler --lib`
预期：编译失败，报 `no method named 'next_fire_by_trigger'` / `no method named 'take_task_items'`。

- [ ] **Step 3: 实现两个方法**

在 `crates/scheduler/src/queue.rs` 的 `impl ScheduleQueue` 内、`is_empty` 之后、`impl` 结尾大括号之前插入：

```rust
    /// 返回每个触发器的下次触发时间，只统计当前 generation 有效的条目。
    /// 同一触发器存在多条有效条目时取最早的一条。
    pub fn next_fire_by_trigger(&self) -> HashMap<(TaskId, TriggerId), DateTime<Utc>> {
        let mut result: HashMap<(TaskId, TriggerId), DateTime<Utc>> = HashMap::new();
        for item in self.heap.iter() {
            if !self.is_valid(item) {
                continue;
            }
            let key = (item.task_id, item.trigger_id);
            match result.get_mut(&key) {
                Some(existing) => {
                    if item.next_fire_at < *existing {
                        *existing = item.next_fire_at;
                    }
                }
                None => {
                    result.insert(key, item.next_fire_at);
                }
            }
        }
        result
    }

    /// 取出某任务的全部条目（其余任务的条目保留在堆中），供重摇后按新 generation 重新入队。
    pub fn take_task_items(&mut self, task_id: &TaskId) -> Vec<ScheduledItem> {
        let heap = std::mem::take(&mut self.heap);
        let mut taken = Vec::new();
        let mut kept = BinaryHeap::new();
        for item in heap {
            if item.task_id == *task_id {
                taken.push(item);
            } else {
                kept.push(item);
            }
        }
        self.heap = kept;
        taken
    }
```

- [ ] **Step 4: 运行测试确认通过**

运行：`cargo test -p easyjob-scheduler --lib`
预期：`test result: ok. 3 passed`（queue 模组），其余既有单测同样通过。

- [ ] **Step 5: 提交**

```bash
git add crates/scheduler/src/queue.rs
git commit -m "$(cat <<'EOF'
feat(scheduler): 新增队列只读快照与按任务取出条目的能力

任务列表概览需要读取「每个触发器的下次时间」但不改调度状态，
按任务取出条目则是「只重摇一个触发器」的前置步骤，避免改动时
波及其它任务的堆内条目。
EOF
)"
```

---

### Task 2: `SchedulerCommand::RerollTrigger` 与调度循环实现

**Files:**

- Modify: `crates/scheduler/src/scheduler.rs:13-19`、`:95-132`、`:194-197`
- Test: `crates/scheduler/tests/scheduler_loop_tests.rs:1-13`、文末

**Interfaces:**

- Consumes: `ScheduleQueue::{current_generation, bump_generation, take_task_items, push, len}`、`evaluate_next_occurrence`
- Produces: `SchedulerCommand::RerollTrigger { task_id, trigger_id, reply }`、`pub type RerollReply`

- [ ] **Step 1: 编写失败测试**

编辑 `crates/scheduler/tests/scheduler_loop_tests.rs` 的 import 区（:1-13）：

1. 把 `use chrono::{Duration, Utc};` 改为 `use chrono::{DateTime, Duration, NaiveTime, Utc};`
2. 把 `use easyjob_domain::trigger::{Trigger, TriggerKind};` 改为 `use easyjob_domain::trigger::{FuzzyPeriod, Trigger, TriggerKind};`
3. 在 `use std::collections::HashMap;` 之后新增 `use std::sync::Arc;`

`use easyjob_scheduler::queue::{ScheduleQueue, ScheduledItem};` 保持不变（`ScheduleQueue` 已导入，不要再重复导入，否则会报 `E0252`）。

在该文件末尾追加辅助函数与两个测试：

```rust
fn task_with_triggers(task_id: TaskId, triggers: Vec<Trigger>) -> Task {
    Task {
        id: task_id,
        name: "Reroll Test Task".to_string(),
        description: None,
        enabled: true,
        triggers,
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

async fn reroll(
    scheduler: &Scheduler,
    task_id: TaskId,
    trigger_id: TriggerId,
) -> std::result::Result<Option<DateTime<Utc>>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    scheduler
        .sender()
        .send(SchedulerCommand::RerollTrigger {
            task_id,
            trigger_id,
            reply: tx,
        })
        .await
        .unwrap();
    rx.await.unwrap()
}

async fn read_next_fire(
    queue: &Arc<tokio::sync::Mutex<ScheduleQueue>>,
    key: (TaskId, TriggerId),
) -> Option<DateTime<Utc>> {
    let q = queue.lock().await;
    q.next_fire_by_trigger().get(&key).copied()
}

#[tokio::test]
async fn test_reroll_trigger_rerolls_only_target_and_keeps_other_triggers() {
    let (event_tx, _event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());
    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue.clone(), cmd_rx, event_tx));

    let task_id = TaskId::new();
    let fuzzy_id = TriggerId::new();
    let interval_id = TriggerId::new();

    let fuzzy = Trigger {
        id: fuzzy_id,
        task_id,
        enabled: true,
        kind: TriggerKind::Fuzzy {
            period: FuzzyPeriod::Daily,
            window_start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            window_end: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            timezone: "UTC".into(),
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let interval = Trigger {
        id: interval_id,
        task_id,
        enabled: true,
        kind: TriggerKind::Interval {
            interval_secs: 3600,
            start_at: None,
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    scheduler
        .sender()
        .send(SchedulerCommand::add_task(task_with_triggers(
            task_id,
            vec![fuzzy, interval],
        )))
        .await
        .unwrap();

    // 第一次重摇的回执返回时，AddTask 必已被调度循环处理，可直接作为基线
    let fuzzy_baseline = reroll(&scheduler, task_id, fuzzy_id)
        .await
        .expect("reroll should succeed")
        .expect("daily fuzzy trigger always has a next occurrence");
    let interval_baseline = read_next_fire(&queue, (task_id, interval_id))
        .await
        .expect("interval trigger must be enqueued");

    let mut seen_different = false;
    for _ in 0..30 {
        let next = reroll(&scheduler, task_id, fuzzy_id)
            .await
            .expect("reroll should succeed")
            .expect("daily fuzzy trigger always has a next occurrence");
        if next != fuzzy_baseline {
            seen_different = true;
        }
        // 回归防护：同任务其它触发器的下次时间必须完全不变（复用 add_task 会把它推后）
        assert_eq!(
            read_next_fire(&queue, (task_id, interval_id)).await,
            Some(interval_baseline),
            "rerolling one trigger must not reschedule the other trigger"
        );
        // 不应累积或丢失条目
        assert_eq!(queue.lock().await.len(), 2);
    }
    assert!(
        seen_different,
        "30 次重摇都没有改变 Fuzzy 的随机点，说明重摇没有真正重新求值"
    );

    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_reroll_trigger_rejects_unregistered_task_and_disabled_trigger() {
    let (event_tx, _event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());
    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, event_tx));

    // 未注册任务
    let result = reroll(&scheduler, TaskId::new(), TriggerId::new()).await;
    assert!(result.is_err(), "unregistered task must be rejected");

    // 已注册任务但触发器停用（mpsc 保序，AddTask 必先于 RerollTrigger 被处理）
    let task_id = TaskId::new();
    let disabled_trigger_id = TriggerId::new();
    let task = task_with_triggers(
        task_id,
        vec![Trigger {
            id: disabled_trigger_id,
            task_id,
            enabled: false,
            kind: TriggerKind::Interval {
                interval_secs: 60,
                start_at: None,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }],
    );
    scheduler
        .sender()
        .send(SchedulerCommand::add_task(task))
        .await
        .unwrap();

    let result = reroll(&scheduler, task_id, disabled_trigger_id).await;
    assert!(result.is_err(), "disabled trigger must be rejected");

    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}
```

- [ ] **Step 2: 运行测试确认失败**

运行：`cargo test -p easyjob-scheduler --test scheduler_loop_tests reroll`
预期：编译失败，报 `no variant named 'RerollTrigger'`。

- [ ] **Step 3: 新增命令变体与处理分支**

编辑 `crates/scheduler/src/scheduler.rs`：在 `SchedulerCommand` 定义之前新增类型别名，并在枚举里新增变体（`Shutdown` 之前）：

```rust
/// 重摇回执：`Ok(Some(next))` 表示新的下次触发时间，`Ok(None)` 表示该触发器不再有下一次。
/// `Err(原因)` 表示任务未注册到调度器、任务或触发器已停用、或触发器不存在。
pub type RerollReply =
    tokio::sync::oneshot::Sender<std::result::Result<Option<DateTime<Utc>>, String>>;

#[derive(Debug)]
pub enum SchedulerCommand {
    AddTask(Box<Task>),
    RemoveTask(TaskId),
    TriggerNow(TaskId),
    RerollTrigger {
        task_id: TaskId,
        trigger_id: TriggerId,
        reply: RerollReply,
    },
    Shutdown,
}
```

在 `run` 的 `match cmd` 内、`Some(SchedulerCommand::TriggerNow(id)) => { ... }` 分支之后新增：

```rust
                        Some(SchedulerCommand::RerollTrigger { task_id, trigger_id, reply }) => {
                            let result =
                                reroll_trigger(&queue, &registered_tasks, task_id, trigger_id)
                                    .await;
                            let _ = reply.send(result);
                        }
```

- [ ] **Step 4: 实现 `reroll_trigger`**

在 `crates/scheduler/src/scheduler.rs` 文件末尾（`impl Scheduler` 之外）追加：

```rust
/// 只重排目标触发器：目标触发器以 `now` 重新求值（Fuzzy 因此得到新的随机点），
/// 同一任务其它触发器的 `next_fire_at` 保持原值。
///
/// 实现要点：先按旧的当前 generation 过滤出该任务真正有效的条目，再 `bump_generation`
/// 让残留条目失效，最后按新 generation 重新入队。绝不复用 `add_task`
/// （那会以 `now` 为基准重算同任务的 Interval 触发器，把它的下次时间推后）。
async fn reroll_trigger(
    queue: &Arc<Mutex<ScheduleQueue>>,
    registered_tasks: &HashMap<TaskId, Box<Task>>,
    task_id: TaskId,
    trigger_id: TriggerId,
) -> std::result::Result<Option<DateTime<Utc>>, String> {
    let Some(task) = registered_tasks.get(&task_id) else {
        return Err(format!(
            "Task '{}' is not registered to the scheduler",
            task_id
        ));
    };
    if !task.enabled {
        return Err(format!("Task '{}' is disabled", task_id));
    }
    let Some(trigger) = task.triggers.iter().find(|t| t.id == trigger_id) else {
        return Err(format!(
            "Trigger '{}' not found in task '{}'",
            trigger_id, task_id
        ));
    };
    if !trigger.enabled {
        return Err(format!("Trigger '{}' is disabled", trigger_id));
    }

    let mut q = queue.lock().await;
    let old_generation = q.current_generation(&task_id);
    let valid_items: Vec<ScheduledItem> = q
        .take_task_items(&task_id)
        .into_iter()
        .filter(|item| item.generation == old_generation)
        .collect();
    let new_generation = q.bump_generation(&task_id);

    let mut target_rerolled = false;
    let mut new_next: Option<DateTime<Utc>> = None;
    for mut item in valid_items {
        if item.trigger_id == trigger_id {
            target_rerolled = true;
            match evaluate_next_occurrence(&trigger.kind, Utc::now()) {
                Some(next) => {
                    item.next_fire_at = next;
                    new_next = Some(next);
                }
                None => continue,
            }
        }
        item.generation = new_generation;
        q.push(item);
    }

    // 目标触发器原本不在队列中（例如 Network、已过期的 Once）：求值一次后按需入队
    if !target_rerolled {
        if let Some(next) = evaluate_next_occurrence(&trigger.kind, Utc::now()) {
            q.push(ScheduledItem {
                task_id,
                trigger_id,
                next_fire_at: next,
                generation: new_generation,
            });
            new_next = Some(next);
        }
    }

    Ok(new_next)
}
```

- [ ] **Step 5: 运行测试确认通过**

运行：`cargo test -p easyjob-scheduler`
预期：`test result: ok`，`scheduler_loop_tests` 中两个 reroll 测试与既有测试全部通过。

- [ ] **Step 6: 提交**

```bash
git add crates/scheduler/src/scheduler.rs crates/scheduler/tests/scheduler_loop_tests.rs
git commit -m "$(cat <<'EOF'
feat(scheduler): 支持只重摇单个触发器

列表上的「重摇」必须只影响被点的那个 Fuzzy 触发器；若复用 add_task，
同任务的 Interval 触发器会以 now 为基准重算而被推后。带 oneshot 回执
的命令让调度循环能回传新时间与「未注册/已停用」错误。
EOF
)"
```

---

### Task 3: `find_latest_run_per_task` 与复合索引

**Files:**

- Create: `migrations/20260919000000_add_task_runs_task_started_index.sql`
- Modify: `crates/persistence/src/execution_repo.rs:1-5`、`:14-27`、`:145-164`
- Test: `crates/persistence/tests/repository_tests.rs`（文末新增）

**Interfaces:**

- Consumes: `ExecutionRepository`、`map_row_to_execution`、`task_runs(task_id, started_at)`
- Produces: `ExecutionRepository::find_latest_run_per_task(&self, task_ids: &[TaskId]) -> Result<HashMap<TaskId, Execution>>`

- [ ] **Step 1: 编写失败测试**

在 `crates/persistence/tests/repository_tests.rs` 文末追加：

```rust
async fn create_run_at(
    repo: &SqliteExecutionRepository,
    task_id: TaskId,
    started_at: chrono::DateTime<chrono::Utc>,
    status: ExecutionStatus,
) -> Execution {
    let mut exec = Execution::new(task_id, None, Some(started_at));
    exec.status = status;
    exec.started_at = started_at;
    exec.finished_at = Some(started_at + chrono::Duration::seconds(1));
    exec.duration_ms = Some(1000);
    repo.create_run(&exec).await.unwrap();
    exec
}

#[tokio::test]
async fn test_find_latest_run_per_task_returns_newest_per_task() {
    let pool = setup_test_db().await;
    let exec_repo = SqliteExecutionRepository::new(pool.clone());
    let task_repo = SqliteTaskRepository::new(pool.clone());

    let task_a = make_dummy_task("latest-a");
    let task_b = make_dummy_task("latest-b");
    let task_c = make_dummy_task("latest-c");
    task_repo.save(&task_a).await.unwrap();
    task_repo.save(&task_b).await.unwrap();
    task_repo.save(&task_c).await.unwrap();

    let base = chrono::Utc::now() - chrono::Duration::hours(3);
    create_run_at(&exec_repo, task_a.id, base, ExecutionStatus::Succeeded).await;
    let a_new = create_run_at(
        &exec_repo,
        task_a.id,
        base + chrono::Duration::hours(1),
        ExecutionStatus::Failed,
    )
    .await;
    let b_only = create_run_at(&exec_repo, task_b.id, base, ExecutionStatus::Succeeded).await;

    let result = exec_repo
        .find_latest_run_per_task(&[task_a.id, task_b.id, task_c.id])
        .await
        .unwrap();

    assert_eq!(result.len(), 2, "无运行记录的 task_c 不应出现在结果中");
    assert_eq!(result[&task_a.id].id, a_new.id);
    assert_eq!(result[&task_a.id].status, ExecutionStatus::Failed);
    assert_eq!(result[&task_b.id].id, b_only.id);
    assert!(!result.contains_key(&task_c.id));

    // 空入参不应构造非法 SQL
    assert!(exec_repo.find_latest_run_per_task(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn test_find_latest_run_per_task_tie_breaks_by_id_desc() {
    let pool = setup_test_db().await;
    let exec_repo = SqliteExecutionRepository::new(pool.clone());
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let task = make_dummy_task("latest-tie");
    task_repo.save(&task).await.unwrap();

    let started_at = chrono::Utc::now() - chrono::Duration::hours(1);
    let mut exec_a = Execution::new(task.id, None, Some(started_at));
    exec_a.status = ExecutionStatus::Succeeded;
    exec_a.started_at = started_at;
    let mut exec_b = Execution::new(task.id, None, Some(started_at));
    exec_b.status = ExecutionStatus::Failed;
    exec_b.started_at = started_at;

    // 让 id 字符串较大的那条后插入，确保「取较大 id」不是插入顺序导致的
    let (first, second, expected_status) = if exec_a.id.to_string() < exec_b.id.to_string() {
        (exec_a, exec_b, ExecutionStatus::Failed)
    } else {
        (exec_b, exec_a, ExecutionStatus::Succeeded)
    };
    let expected_id = second.id;
    exec_repo.create_run(&first).await.unwrap();
    exec_repo.create_run(&second).await.unwrap();

    let result = exec_repo
        .find_latest_run_per_task(&[task.id])
        .await
        .unwrap();
    assert_eq!(result[&task.id].id, expected_id);
    assert_eq!(result[&task.id].status, expected_status);
}
```

- [ ] **Step 2: 运行测试确认失败**

运行：`cargo test -p easyjob-persistence --test repository_tests find_latest_run_per_task`
预期：编译失败，报 `no method named 'find_latest_run_per_task'`。

- [ ] **Step 3: 新增 migration**

创建 `migrations/20260919000000_add_task_runs_task_started_index.sql`：

```sql
-- 支撑「每个任务最近一次执行」的窗口函数查询：
-- ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY started_at DESC, id DESC)
CREATE INDEX IF NOT EXISTS idx_runs_task_id_started_at ON task_runs(task_id, started_at);
```

- [ ] **Step 4: 新增 trait 方法与实现**

编辑 `crates/persistence/src/execution_repo.rs`：在文件顶部 import 区新增 `use std::collections::HashMap;`；在 `ExecutionRepository` trait 末尾（`purge_expired_runs` 之后）新增：

```rust
    /// 批量查询每个任务最近一次执行（无运行记录的任务不会出现在结果中）。
    async fn find_latest_run_per_task(
        &self,
        task_ids: &[TaskId],
    ) -> Result<HashMap<TaskId, Execution>>;
```

在 `impl ExecutionRepository for SqliteExecutionRepository` 的 `purge_expired_runs` 之后新增实现：

```rust
    async fn find_latest_run_per_task(
        &self,
        task_ids: &[TaskId],
    ) -> Result<HashMap<TaskId, Execution>> {
        if task_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = vec!["?"; task_ids.len()].join(", ");
        let sql = format!(
            "SELECT * FROM (
                 SELECT *,
                        ROW_NUMBER() OVER (
                            PARTITION BY task_id ORDER BY started_at DESC, id DESC
                        ) AS row_num
                 FROM task_runs
                 WHERE task_id IN ({placeholders})
             ) WHERE row_num = 1"
        );

        let mut query = sqlx::query(&sql);
        for id in task_ids {
            query = query.bind(id.to_string());
        }
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut latest = HashMap::with_capacity(rows.len());
        for row in &rows {
            let exec = map_row_to_execution(row)?;
            latest.insert(exec.task_id, exec);
        }
        Ok(latest)
    }
```

- [ ] **Step 5: 运行测试确认通过**

运行：`cargo test -p easyjob-persistence --test repository_tests`
预期：`test result: ok`，两个新测试与既有测试全部通过。

- [ ] **Step 6: 提交**

```bash
git add migrations/20260919000000_add_task_runs_task_started_index.sql crates/persistence/src/execution_repo.rs crates/persistence/tests/repository_tests.rs
git commit -m "$(cat <<'EOF'
feat(persistence): 批量查询每个任务最近一次执行

任务列表要为每个任务显示上次结果，逐任务查询会带来 N 次往返；
用窗口函数一次取回，并以 id DESC 作为并列 started_at 的稳定次级排序键，
避免结果不确定导致测试偶发失败。
EOF
)"
```

---

### Task 4: `AgentService` 持有调度队列 + `task.overview` IPC

**Files:**

- Modify: `apps/agent/src/service.rs:17-24`、`:25-42`、`:54-65`、`:104-115`、`:445-458`、`:460-499`、`:926-932`
- Test: `apps/agent/tests/service_tests.rs:635-670`、文末

**Interfaces:**

- Consumes: `Scheduler::queue()`、`ScheduleQueue::next_fire_by_trigger`、`ExecutionRepository::find_latest_run_per_task`
- Produces: IPC 方法 `task.overview`；Rust DTO `TaskOverviewEntry { task_id, triggers, last_run }`、`TriggerOverview { trigger_id, next_fire_at }`、`LastRunOverview { status, started_at, finished_at, duration_ms, exit_code, error_message }`

- [ ] **Step 1: 编写失败测试**

编辑 `apps/agent/tests/service_tests.rs`：把 `setup_test_agent_handler`（:635-670）替换为返回队列并使用真实调度循环的版本：

```rust
async fn setup_test_agent_handler() -> (
    AgentRpcHandler,
    std::sync::Arc<tokio::sync::Mutex<easyjob_scheduler::queue::ScheduleQueue>>,
    tempfile::TempDir,
) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("agent_test.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());
    let pool = easyjob_persistence::init_pool(&db_url).await.unwrap();

    let task_repo = Arc::new(easyjob_persistence::SqliteTaskRepository::new(pool.clone()));
    let exec_repo = Arc::new(easyjob_persistence::SqliteExecutionRepository::new(
        pool.clone(),
    ));
    let settings_repo = Arc::new(easyjob_persistence::SqliteSettingsRepository::new(
        pool.clone(),
    ));
    let exec_manager = Arc::new(easyjob_executor::manager::ExecutionManager::new(4));

    let (sched_event_tx, _sched_event_rx) =
        tokio::sync::mpsc::channel::<easyjob_scheduler::scheduler::TriggerEvent>(16);
    let (scheduler, scheduler_cmd_rx) = easyjob_scheduler::scheduler::Scheduler::new(sched_event_tx);
    let sched_queue = scheduler.queue();
    let scheduler_tx = scheduler.sender();
    let handle_queue = sched_queue.clone();
    let handle_event_tx = scheduler.event_sender();
    tokio::spawn(easyjob_scheduler::scheduler::Scheduler::run(
        handle_queue,
        scheduler_cmd_rx,
        handle_event_tx,
    ));

    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let active_executions = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let (event_tx, _) = tokio::sync::broadcast::channel(1024);
    let exec_cancel_token = tokio_util::sync::CancellationToken::new();

    let handler = AgentRpcHandler::new(
        task_repo,
        exec_repo,
        settings_repo,
        exec_manager,
        scheduler_tx,
        std::time::Instant::now(),
        shutdown_notify,
        active_executions,
        event_tx,
        exec_cancel_token,
        sched_queue.clone(),
    );

    (handler, sched_queue, dir)
}
```

把该文件里另外两处 `let (handler, _dir) = setup_test_agent_handler().await;`（`test_agent_settings_ipc`、`test_trigger_log_retention_purge_helper_policies`）改为：

```rust
    let (handler, _sched_queue, _dir) = setup_test_agent_handler().await;
```

在文件末尾追加：

```rust
#[tokio::test]
async fn test_task_overview_ipc_reports_next_fire_and_last_run() {
    let (handler, sched_queue, _dir) = setup_test_agent_handler().await;
    let task_repo = handler.task_repository();
    let exec_repo = handler.execution_repository();

    let task_id = TaskId::new();
    let daily_trigger_id = TriggerId::new();
    let network_trigger_id = TriggerId::new();
    let daily_time = chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    let task = Task {
        id: task_id,
        name: "Overview Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![
            Trigger {
                id: daily_trigger_id,
                task_id,
                enabled: true,
                kind: TriggerKind::Daily {
                    time: daily_time,
                    timezone: "UTC".into(),
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Trigger {
                id: network_trigger_id,
                task_id,
                enabled: true,
                kind: TriggerKind::Network {
                    events: vec![easyjob_domain::trigger::NetworkEventKind::Connect],
                    network_name: None,
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        ],
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    task_repo.save(&task).await.unwrap();

    // 无触发器、无运行记录的任务
    let empty_task_id = TaskId::new();
    let empty_task = Task {
        id: empty_task_id,
        name: "Empty Overview Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    task_repo.save(&empty_task).await.unwrap();

    // 队列中只放 Daily 触发器的条目（Network 触发器永远不会入队）
    let next_fire = chrono::DateTime::parse_from_rfc3339("2026-09-20T09:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    {
        let mut q = sched_queue.lock().await;
        q.push(easyjob_scheduler::queue::ScheduledItem {
            task_id,
            trigger_id: daily_trigger_id,
            next_fire_at: next_fire,
            generation: 1,
        });
    }

    let mut run =
        easyjob_domain::execution::Execution::new(task_id, Some(daily_trigger_id), None);
    run.status = easyjob_domain::execution::ExecutionStatus::Succeeded;
    run.started_at = chrono::Utc::now() - chrono::Duration::minutes(5);
    run.finished_at = Some(chrono::Utc::now());
    run.duration_ms = Some(1234);
    run.exit_code = Some(0);
    exec_repo.create_run(&run).await.unwrap();

    let res = handler
        .handle_request(IpcRequest::new("task.overview", serde_json::json!({})))
        .await;
    assert!(res.ok, "task.overview failed: {:?}", res.error);
    let data = res.data.unwrap();
    let entries = data.as_array().expect("overview must be an array");

    let entry = entries
        .iter()
        .find(|e| e["task_id"] == task_id.to_string())
        .expect("task entry present");
    let triggers = entry["triggers"].as_array().expect("triggers array");
    assert_eq!(triggers.len(), 2);

    let daily = triggers
        .iter()
        .find(|t| t["trigger_id"] == daily_trigger_id.to_string())
        .expect("daily trigger present");
    let daily_next: chrono::DateTime<chrono::Utc> =
        serde_json::from_value(daily["next_fire_at"].clone()).unwrap();
    assert_eq!(daily_next, next_fire);

    let network = triggers
        .iter()
        .find(|t| t["trigger_id"] == network_trigger_id.to_string())
        .expect("network trigger present");
    assert!(
        network["next_fire_at"].is_null(),
        "network trigger has no queued entry"
    );

    let last_run = &entry["last_run"];
    assert_eq!(last_run["status"], "Succeeded");
    assert_eq!(last_run["duration_ms"], 1234);
    assert_eq!(last_run["exit_code"], 0);
    assert!(last_run["error_message"].is_null());

    let empty_entry = entries
        .iter()
        .find(|e| e["task_id"] == empty_task_id.to_string())
        .expect("empty task entry present");
    assert_eq!(
        empty_entry["triggers"].as_array().map(|a| a.len()),
        Some(0)
    );
    assert!(
        empty_entry["last_run"].is_null(),
        "never-executed task must report last_run = null"
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

运行：`cargo test -p easyjob-agent --test service_tests test_task_overview_ipc_reports_next_fire_and_last_run`
预期：编译失败，报 `this function takes 10 arguments but 11 were supplied` 与 `Method 'task.overview' not found`。

- [ ] **Step 3: 让 `AgentService` 与 handler 持有队列**

编辑 `apps/agent/src/service.rs`：

1. import 区（:17-24）把 `use easyjob_common::{ExecutionId, Result, TaskId};` 改为 `use easyjob_common::{ExecutionId, Result, TaskId, TriggerId};`，并在 `use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};` 之后新增 `use easyjob_scheduler::queue::ScheduleQueue;`（`:16` 已经导入 `Scheduler` / `SchedulerCommand` / `TriggerEvent`，不要重复导入）。
2. `AgentService` 结构体（:25-42）新增字段 `sched_queue: Arc<Mutex<ScheduleQueue>>,`。
3. `init()`（:54-65）改为先把队列克隆给调度循环，自己保留一份：

```rust
        // Spawn scheduler loop first before loading tasks to avoid bounded channel deadlock
        let sched_queue = scheduler.queue();
        let sched_event_tx = scheduler.event_sender();
        let scheduler_handle = tokio::spawn(Scheduler::run(
            sched_queue.clone(),
            scheduler_cmd_rx,
            sched_event_tx,
        ));
```

4. `init()` 里构造 handler 处（:104-115）新增 `sched_queue: sched_queue.clone(),`。
5. `init()` 的 `Ok(Self { ... })`（:119-135）新增 `sched_queue,`。
6. `AgentRpcHandler` 结构体（:445-458）新增字段 `pub(crate) sched_queue: Arc<Mutex<ScheduleQueue>>,`。
7. `AgentRpcHandler::new`（:460-486）追加参数与赋值：

```rust
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task_repo: Arc<SqliteTaskRepository>,
        exec_repo: Arc<SqliteExecutionRepository>,
        settings_repo: Arc<SqliteSettingsRepository>,
        exec_manager: Arc<ExecutionManager>,
        scheduler_tx: mpsc::Sender<SchedulerCommand>,
        start_time: Instant,
        shutdown_notify: Arc<Notify>,
        active_executions: Arc<Mutex<HashMap<ExecutionId, CancellationToken>>>,
        event_tx: broadcast::Sender<IpcEvent>,
        exec_cancel_token: CancellationToken,
        sched_queue: Arc<Mutex<ScheduleQueue>>,
    ) -> Self {
        Self {
            task_repo,
            exec_repo,
            settings_repo,
            exec_manager,
            scheduler_tx,
            start_time,
            shutdown_notify,
            active_executions,
            event_tx,
            exec_cancel_token,
            sched_queue,
        }
    }
```

- [ ] **Step 4: 新增 overview DTO 与 IPC 分支**

在 `apps/agent/src/service.rs` 的 `AgentRpcHandler` 结构体之前新增 DTO：

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct TriggerOverview {
    pub trigger_id: TriggerId,
    pub next_fire_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LastRunOverview {
    pub status: ExecutionStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskOverviewEntry {
    pub task_id: TaskId,
    pub triggers: Vec<TriggerOverview>,
    pub last_run: Option<LastRunOverview>,
}
```

在 `handle_request` 的 `match req.method.as_str()` 中、`"task.trigger_now"` 分支之后、`"execution.list"` 之前新增：

```rust
            "task.overview" => {
                let tasks = match self.task_repo.find_all().await {
                    Ok(tasks) => tasks,
                    Err(e) => return IpcResponse::error(req.id, e.to_string()),
                };
                let task_ids: Vec<TaskId> = tasks.iter().map(|task| task.id).collect();
                let latest_runs = match self.exec_repo.find_latest_run_per_task(&task_ids).await {
                    Ok(runs) => runs,
                    Err(e) => return IpcResponse::error(req.id, e.to_string()),
                };
                let next_by_trigger = {
                    let queue = self.sched_queue.lock().await;
                    queue.next_fire_by_trigger()
                };

                let overview: Vec<TaskOverviewEntry> = tasks
                    .iter()
                    .map(|task| TaskOverviewEntry {
                        task_id: task.id,
                        triggers: task
                            .triggers
                            .iter()
                            .map(|trigger| TriggerOverview {
                                trigger_id: trigger.id,
                                next_fire_at: next_by_trigger
                                    .get(&(task.id, trigger.id))
                                    .copied(),
                            })
                            .collect(),
                        last_run: latest_runs.get(&task.id).map(|run| LastRunOverview {
                            status: run.status,
                            started_at: run.started_at,
                            finished_at: run.finished_at,
                            duration_ms: run.duration_ms,
                            exit_code: run.exit_code,
                            error_message: run.error_message.clone(),
                        }),
                    })
                    .collect();

                match serde_json::to_value(overview) {
                    Ok(value) => IpcResponse::success(req.id, value),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
```

- [ ] **Step 5: 运行测试确认通过**

运行：`cargo test -p easyjob-agent --test service_tests`
预期：`test result: ok`，包含新测试在内的全部测试通过。

- [ ] **Step 6: 提交**

```bash
git add apps/agent/src/service.rs apps/agent/tests/service_tests.rs
git commit -m "$(cat <<'EOF'
feat(agent): 新增 task.overview 汇总下次触发时间与上次执行

下次触发时间只存在于调度队列里（Fuzzy 每次求值随机、Cron 需 croner），
前端无法自行计算，因此由 agent 读取队列快照并结合一次批量查询返回。
EOF
)"
```

---

### Task 5: `trigger.reroll` IPC

**Files:**

- Modify: `apps/agent/src/service.rs:926-932`（`_ =>` 兜底分支之前）
- Test: `apps/agent/tests/service_tests.rs`（文末新增）

**Interfaces:**

- Consumes: `SchedulerCommand::RerollTrigger`、`AgentRpcHandler::scheduler_tx`
- Produces: IPC 方法 `trigger.reroll`，返回 `{ "next_fire_at": string | null }`，失败时 `IpcResponse::error`

- [ ] **Step 1: 编写失败测试**

在 `apps/agent/tests/service_tests.rs` 文末追加：

```rust
#[tokio::test]
async fn test_trigger_reroll_ipc_success_and_errors() {
    let (handler, sched_queue, _dir) = setup_test_agent_handler().await;

    let task_id = TaskId::new();
    let fuzzy_trigger_id = TriggerId::new();
    let disabled_trigger_id = TriggerId::new();
    let task = Task {
        id: task_id,
        name: "Reroll IPC Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![
            Trigger {
                id: fuzzy_trigger_id,
                task_id,
                enabled: true,
                kind: TriggerKind::Fuzzy {
                    period: easyjob_domain::trigger::FuzzyPeriod::Daily,
                    window_start: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                    window_end: chrono::NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
                    timezone: "UTC".into(),
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Trigger {
                id: disabled_trigger_id,
                task_id,
                enabled: false,
                kind: TriggerKind::Interval {
                    interval_secs: 60,
                    start_at: None,
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        ],
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let saved = handler
        .handle_request(IpcRequest::new(
            "task.save",
            serde_json::json!({ "task": task }),
        ))
        .await;
    assert!(saved.ok, "task.save failed: {:?}", saved.error);

    let res = handler
        .handle_request(IpcRequest::new(
            "trigger.reroll",
            serde_json::json!({ "task_id": task_id, "trigger_id": fuzzy_trigger_id }),
        ))
        .await;
    assert!(res.ok, "trigger.reroll failed: {:?}", res.error);
    let next_fire = res.data.unwrap()["next_fire_at"].clone();
    assert!(
        !next_fire.is_null(),
        "daily fuzzy trigger must report a next fire time"
    );
    {
        let queue = sched_queue.lock().await;
        assert!(queue
            .next_fire_by_trigger()
            .contains_key(&(task_id, fuzzy_trigger_id)));
    }

    // 停用触发器 → 错误
    let disabled = handler
        .handle_request(IpcRequest::new(
            "trigger.reroll",
            serde_json::json!({ "task_id": task_id, "trigger_id": disabled_trigger_id }),
        ))
        .await;
    assert!(!disabled.ok, "disabled trigger must be rejected");

    // 未注册任务 → 错误
    let unregistered = handler
        .handle_request(IpcRequest::new(
            "trigger.reroll",
            serde_json::json!({ "task_id": TaskId::new(), "trigger_id": TriggerId::new() }),
        ))
        .await;
    assert!(!unregistered.ok, "unregistered task must be rejected");
}
```

- [ ] **Step 2: 运行测试确认失败**

运行：`cargo test -p easyjob-agent --test service_tests test_trigger_reroll_ipc_success_and_errors`
预期：FAIL，`Method 'trigger.reroll' not found`。

- [ ] **Step 3: 实现 `trigger.reroll`**

在 `apps/agent/src/service.rs` 的 `handle_request` 中、`"execution.cancel"` 分支之后、`_ =>` 兜底之前新增：

```rust
            "trigger.reroll" => {
                let task_id: TaskId =
                    match serde_json::from_value(req.params.get("task_id").cloned().unwrap_or_default())
                    {
                        Ok(id) => id,
                        Err(e) => {
                            return IpcResponse::error(
                                req.id,
                                format!("Invalid task_id parameter: {}", e),
                            )
                        }
                    };
                let trigger_id: TriggerId = match serde_json::from_value(
                    req.params.get("trigger_id").cloned().unwrap_or_default(),
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        return IpcResponse::error(
                            req.id,
                            format!("Invalid trigger_id parameter: {}", e),
                        )
                    }
                };

                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                if self
                    .scheduler_tx
                    .send(SchedulerCommand::RerollTrigger {
                        task_id,
                        trigger_id,
                        reply: reply_tx,
                    })
                    .await
                    .is_err()
                {
                    return IpcResponse::error(req.id, "Scheduler is not running".to_string());
                }

                match reply_rx.await {
                    Ok(Ok(next_fire_at)) => IpcResponse::success(
                        req.id,
                        serde_json::json!({ "next_fire_at": next_fire_at }),
                    ),
                    Ok(Err(message)) => IpcResponse::error(req.id, message),
                    Err(_) => IpcResponse::error(
                        req.id,
                        "Scheduler dropped the reroll request".to_string(),
                    ),
                }
            }
```

- [ ] **Step 4: 运行测试确认通过**

运行：`cargo test -p easyjob-agent --test service_tests`
预期：`test result: ok`，全部测试通过。

- [ ] **Step 5: 提交**

```bash
git add apps/agent/src/service.rs apps/agent/tests/service_tests.rs
git commit -m "$(cat <<'EOF'
feat(agent): 新增 trigger.reroll IPC

列表上的重摇按钮需要一个返回新时间的入口；转发到调度循环执行，
让「未注册 / 已停用」这类只有调度器知道的状态能如实回传为 IPC 错误。
EOF
)"
```

---

### Task 6: Tauri 命令 `task_overview` / `reroll_trigger`

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs:1-8`、文末
- Modify: `apps/desktop/src-tauri/src/lib.rs:94-108`
- Test: 无独立测试文件；以 `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --check` 作为验证

**Interfaces:**

- Consumes: `AgentManager::call`
- Produces: Tauri 命令 `task_overview` → IPC `task.overview`；`reroll_trigger` → IPC `trigger.reroll`

- [ ] **Step 1: 新增两个命令**

编辑 `apps/desktop/src-tauri/src/commands.rs`：把 `use easyjob_common::{ExecutionId, TaskId};` 改为 `use easyjob_common::{ExecutionId, TaskId, TriggerId};`，并在文件末尾追加：

```rust
#[tauri::command]
pub async fn task_overview(
    manager: State<'_, Arc<AgentManager>>,
) -> Result<serde_json::Value, String> {
    manager.call("task.overview", serde_json::json!({})).await
}

#[tauri::command]
pub async fn reroll_trigger(
    task_id: TaskId,
    trigger_id: TriggerId,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<serde_json::Value, String> {
    manager
        .call(
            "trigger.reroll",
            serde_json::json!({ "task_id": task_id, "trigger_id": trigger_id }),
        )
        .await
}
```

- [ ] **Step 2: 在 `generate_handler!` 注册**

编辑 `apps/desktop/src-tauri/src/lib.rs:94-108`，在 `save_system_settings,` 之后新增两行：

```rust
        .invoke_handler(tauri::generate_handler![
            get_agent_status,
            list_tasks,
            get_task,
            save_task,
            delete_task,
            trigger_task,
            list_executions,
            get_execution,
            cancel_execution,
            get_execution_output,
            restart_agent,
            get_system_settings,
            save_system_settings,
            task_overview,
            reroll_trigger,
        ])
```

- [ ] **Step 3: 验证编译与 lint**

运行：

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

预期：均无输出/退出码 0（两个新命令已注册、无 warning）。

- [ ] **Step 4: 提交**

```bash
git add apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(desktop): 透传 task_overview 与 reroll_trigger 两个命令

桌面端必须经由 Tauri 命令层才能访问 agent IPC；沿用既有 manager.call
风格，返回值直接透传 JSON 避免在 Tauri 侧重复定义 DTO。
EOF
)"
```

---

### Task 7: 前端 service 与 overview 类型

**Files:**

- Modify: `apps/desktop/src/types/task.ts:1-3`、`286-305` 之后
- Modify: `apps/desktop/src/services/tauri.ts:1-4`、文末

**Interfaces:**

- Produces: `TaskOverviewTrigger`、`TaskOverviewLastRun`、`TaskOverview`、`getTaskOverview()`、`rerollTrigger(taskId, triggerId)`

- [ ] **Step 1: 新增 TS 类型**

在 `apps/desktop/src/types/task.ts` 的 `parseDate` 之后（`export interface Task` 之前）新增：

```ts
export interface TaskOverviewTrigger {
  trigger_id: TriggerId;
  /** null 表示当前队列中没有该触发器的有效条目（Network 事件触发、已过期的 Once、任务或触发器被停用） */
  next_fire_at: string | null;
}

export interface TaskOverviewLastRun {
  status: string;
  started_at: string;
  finished_at: string | null;
  duration_ms: number | null;
  exit_code: number | null;
  error_message: string | null;
}

export interface TaskOverview {
  task_id: TaskId;
  triggers: TaskOverviewTrigger[];
  last_run: TaskOverviewLastRun | null;
}
```

- [ ] **Step 2: 新增 service 方法**

编辑 `apps/desktop/src/services/tauri.ts`：把类型 import 改为 `import type { Task, TaskId, TriggerId, SystemSettings, TaskOverview } from '../types/task';`，并在文件末尾追加：

```ts
export async function getTaskOverview(): Promise<TaskOverview[]> {
  return await invoke<TaskOverview[]>('task_overview');
}

export async function rerollTrigger(
  taskId: TaskId,
  triggerId: TriggerId
): Promise<{ next_fire_at: string | null }> {
  return await invoke<{ next_fire_at: string | null }>('reroll_trigger', {
    taskId,
    triggerId,
  });
}
```

- [ ] **Step 3: 验证类型检查**

运行：`pnpm -C apps/desktop run build`
预期：`vue-tsc --noEmit` 与 `vite build` 均成功（无类型错误）。

- [ ] **Step 4: 提交**

```bash
git add apps/desktop/src/services/tauri.ts apps/desktop/src/types/task.ts
git commit -m "$(cat <<'EOF'
feat(desktop): 新增 task.overview / trigger.reroll 的前端调用与类型

把 overview 的类型定义放在任务类型文件里，便于 store、视图与测试共用
同一份线格式约定。
EOF
)"
```

---

### Task 8: `describeTriggerShort` 与时间格式化工具

**Files:**

- Modify: `apps/desktop/src/types/task.ts`（在 `describeTrigger`（:179-249）之后新增）
- Test: `apps/desktop/tests/triggerShort.test.ts`（新建）

**Interfaces:**

- Consumes: `TriggerKind`、`Weekday`、`FuzzyPeriod`、`parseDate`
- Produces: `describeTriggerShort(kind: TriggerKind): string`、`formatDateTimeShort`、`formatDateTimeFull`、`formatRelativeTime`、`formatDateTimeTitle`

- [ ] **Step 1: 编写失败测试**

创建 `apps/desktop/tests/triggerShort.test.ts`：

```ts
import { describe, it, expect } from 'vitest';
import {
  describeTriggerShort,
  formatDateTimeShort,
  formatDateTimeFull,
  formatRelativeTime,
  formatDateTimeTitle,
  type TriggerKind,
} from '../src/types/task';

function localIso(y: number, mo: number, d: number, h: number, mi: number): string {
  return new Date(y, mo - 1, d, h, mi, 0).toISOString();
}

describe('describeTriggerShort', () => {
  it('covers every row of the design table', () => {
    expect(describeTriggerShort({ Daily: { time: '09:00:00', timezone: 'UTC' } })).toBe('每天 09:00');
    expect(
      describeTriggerShort({
        Weekly: { days_of_week: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'], time: '09:00:00', timezone: 'UTC' },
      })
    ).toBe('每周一至周五 09:00');
    expect(describeTriggerShort({ Interval: { interval_secs: 300 } })).toBe('每 5 分钟');
    expect(describeTriggerShort({ Cron: { expression: '0 9 * * *', timezone: 'UTC' } })).toBe('每天 09:00');
    expect(describeTriggerShort({ Cron: { expression: '*/5 * * * *', timezone: 'UTC' } })).toBe('每 5 分钟');
    expect(describeTriggerShort({ Cron: { expression: '0 9 * * 1-5', timezone: 'UTC' } })).toBe('工作日 09:00');
    expect(describeTriggerShort({ Cron: { expression: '0 9 1 * *', timezone: 'UTC' } })).toBe('0 9 1 * *');
    expect(describeTriggerShort({ Cron: { expression: 'L * * * *', timezone: 'UTC' } })).toBe('L * * * *');
    expect(
      describeTriggerShort({ Fuzzy: { period: 'Daily', window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } })
    ).toBe('每天 09:00-10:00 之间随机');
    expect(
      describeTriggerShort({
        Fuzzy: {
          period: { Weekly: { days_of_week: ['Mon', 'Fri'] } },
          window_start: '09:00:00',
          window_end: '10:00:00',
          timezone: 'UTC',
        },
      })
    ).toBe('每周一、周五 09:00-10:00 之间随机');
    expect(describeTriggerShort({ Network: { events: ['Connect'], network_name: null } })).toBe('连接网络时');
    expect(describeTriggerShort('AgentStarted')).toBe('启动时');

    // Once 用本机本地时区渲染；用当前自然年构造以命中 MM-DD HH:mm 分支
    const thisYear = new Date().getFullYear();
    const onceIso = new Date(thisYear, 8, 19, 14, 30, 0).toISOString();
    expect(describeTriggerShort({ Once: { fire_at: onceIso } })).toBe('单次 09-19 14:30');
  });

  it('covers interval unit fallbacks and weekday separators', () => {
    expect(describeTriggerShort({ Interval: { interval_secs: 86400 } })).toBe('每 1 天');
    expect(describeTriggerShort({ Interval: { interval_secs: 7200 } })).toBe('每 2 小时');
    expect(describeTriggerShort({ Interval: { interval_secs: 45 } })).toBe('每 45 秒');
    // Weekly 非连续日用 、连接（规则明细）
    expect(
      describeTriggerShort({
        Weekly: { days_of_week: ['Mon', 'Wed'], time: '18:00:00', timezone: 'UTC' },
      })
    ).toBe('每周一、周三 18:00');
    // 数字 cron 周几同样可识别
    expect(describeTriggerShort({ Cron: { expression: '0 9 * * 1,3', timezone: 'UTC' } })).toBe('每周一、周三 09:00');
  });

  it('does not throw and never leaks undefined for malformed input', () => {
    const malformed: unknown[] = [
      null,
      undefined,
      {},
      { Cron: {} },
      { Cron: { expression: 123 } },
      { Once: {} },
      { Daily: {} },
      { Weekly: {} },
      { Fuzzy: {} },
      { Fuzzy: { period: 'Bogus', window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } },
      { Network: { events: ['Bogus'], network_name: null } },
      { Network: {} },
      { Interval: { interval_secs: 0 } },
      { Interval: {} },
      'Whatever',
    ];
    for (const kind of malformed) {
      expect(() => describeTriggerShort(kind as TriggerKind)).not.toThrow();
      const text = describeTriggerShort(kind as TriggerKind);
      expect(typeof text).toBe('string');
      expect(text.length).toBeGreaterThan(0);
      expect(text).not.toContain('undefined');
      expect(text).not.toContain('[object');
    }
  });
});

describe('time formatting helpers', () => {
  const now = new Date(2026, 8, 19, 12, 0, 0); // 本地 2026-09-19 12:00

  it('uses MM-DD HH:mm within the same calendar year', () => {
    expect(formatDateTimeShort(localIso(2026, 9, 19, 14, 30), now)).toBe('09-19 14:30');
  });

  it('uses YYYY-MM-DD HH:mm across calendar years', () => {
    expect(formatDateTimeShort(localIso(2027, 1, 2, 3, 4), now)).toBe('2027-01-02 03:04');
  });

  it('falls back to an em dash for missing or invalid timestamps', () => {
    expect(formatDateTimeShort(null, now)).toBe('—');
    expect(formatDateTimeShort('not-a-date', now)).toBe('—');
  });

  it('formats full local time and relative time for the title attribute', () => {
    const future = localIso(2026, 9, 19, 15, 0);
    expect(formatDateTimeFull(future)).toBe('2026-09-19 15:00:00');
    expect(formatRelativeTime(future, now)).toBe('3 小时后');
    expect(formatRelativeTime(localIso(2026, 9, 19, 11, 0), now)).toBe('1 小时前');
    expect(formatRelativeTime(localIso(2026, 9, 19, 12, 0), now)).toBe('刚刚');
    expect(formatDateTimeTitle(future, now)).toBe('2026-09-19 15:00:00（3 小时后）');
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

运行：`pnpm -C apps/desktop test -- triggerShort`
预期：FAIL，报 `describeTriggerShort is not a function` / 未导出。

- [ ] **Step 3: 实现辅助函数与 `describeTriggerShort`**

在 `apps/desktop/src/types/task.ts` 的 `describeTrigger` 之后插入：

```ts
const WEEKDAY_ORDER: Weekday[] = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const WEEKDAY_LABEL: Record<Weekday, string> = {
  Mon: '一', Tue: '二', Wed: '三', Thu: '四', Fri: '五', Sat: '六', Sun: '日',
};

const TRIGGER_SHORT_FALLBACK = '未知触发器';

const NETWORK_SHORT_LABEL: Record<NetworkEventKind, string> = {
  Connect: '连接网络时',
  Disconnect: '断开网络时',
  Online: '可上网时',
};

function pad2(value: number): string {
  return value < 10 ? `0${value}` : String(value);
}

// 把 HH:mm:ss / HH:mm 统一截断为 HH:mm；无法识别时原样返回
function trimSeconds(time: unknown): string {
  if (typeof time !== 'string') return '';
  const matched = /^(\d{1,2}:\d{2})(?::\d{2})?/.exec(time);
  return matched ? matched[1].padStart(5, '0') : time;
}

// 相邻连续的日子合并成区间（周一至周五），组间用 separator 连接
function formatWeekdaysShort(days: Weekday[], separator: string): string {
  const indices = new Set<number>();
  for (const day of days) {
    const idx = WEEKDAY_ORDER.indexOf(day);
    if (idx >= 0) indices.add(idx);
  }
  const sorted = Array.from(indices).sort((a, b) => a - b);
  if (sorted.length === 0) return '';

  const groups: number[][] = [];
  for (const idx of sorted) {
    const last = groups[groups.length - 1];
    if (last && idx === last[last.length - 1] + 1) {
      last.push(idx);
    } else {
      groups.push([idx]);
    }
  }
  return groups
    .map((group) => {
      const head = `周${WEEKDAY_LABEL[WEEKDAY_ORDER[group[0]]]}`;
      if (group.length === 1) return head;
      const tail = `周${WEEKDAY_LABEL[WEEKDAY_ORDER[group[group.length - 1]]]}`;
      return `${head}至${tail}`;
    })
    .join(separator);
}

// cron 周几：0 或 7 = 周日，1 = 周一 ... 6 = 周六
function isoWeekdayToShort(day: number): Weekday | null {
  const normalized = day === 7 ? 0 : day;
  const table: Weekday[] = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  return table[normalized] ?? null;
}

function parseCronWeekdays(dow: string): Weekday[] | null {
  const result = new Set<Weekday>();
  for (const token of dow.split(',')) {
    const range = /^(\d{1,2})-(\d{1,2})$/.exec(token);
    if (range) {
      const start = parseInt(range[1], 10);
      const end = parseInt(range[2], 10);
      if (start > end) return null;
      for (let value = start; value <= end; value++) {
        const day = isoWeekdayToShort(value);
        if (!day) return null;
        result.add(day);
      }
      continue;
    }
    if (!/^\d{1,2}$/.test(token)) return null;
    const day = isoWeekdayToShort(parseInt(token, 10));
    if (!day) return null;
    result.add(day);
  }
  if (result.size === 0) return null;
  return Array.from(result);
}

// 只翻译常见 cron 模式；返回 null 表示调用方应回退为原表达式
function describeCronShort(expression: string): string | null {
  const fields = expression.trim().split(/\s+/);
  if (fields.length !== 5) return null;
  const [minute, hour, dayOfMonth, month, dow] = fields;
  if (dayOfMonth !== '*' || month !== '*') return null;

  const everyN = /^\*\/(\d+)$/.exec(minute);
  if (hour === '*' && everyN) {
    const n = parseInt(everyN[1], 10);
    return n > 0 ? `每 ${n} 分钟` : null;
  }

  if (!/^\d{1,2}$/.test(minute) || !/^\d{1,2}$/.test(hour)) return null;
  const mm = parseInt(minute, 10);
  const hh = parseInt(hour, 10);
  if (mm > 59 || hh > 23) return null;
  const time = `${pad2(hh)}:${pad2(mm)}`;

  if (dow === '*') return `每天 ${time}`;

  const days = parseCronWeekdays(dow);
  if (!days) return null;
  const isWorkdays =
    days.length === 5 && days.every((day, idx) => day === WEEKDAY_ORDER[idx]);
  if (isWorkdays) return `工作日 ${time}`;
  return `每${formatWeekdaysShort(days, '、')} ${time}`;
}

export function describeTriggerShort(kind: TriggerKind): string {
  if (typeof kind === 'string') {
    return kind === 'AgentStarted' ? '启动时' : TRIGGER_SHORT_FALLBACK;
  }
  const k = kind as unknown as Record<string, any> | null | undefined;
  if (!k || typeof k !== 'object') return TRIGGER_SHORT_FALLBACK;

  if ('Once' in k) {
    const fireAt = k.Once?.fire_at;
    return typeof fireAt === 'string' ? `单次 ${formatDateTimeShort(fireAt)}` : '单次 —';
  }
  if ('Interval' in k) {
    const raw = k.Interval?.interval_secs ?? k.Interval?.seconds;
    const secs = typeof raw === 'number' && Number.isFinite(raw) && raw > 0 ? raw : null;
    if (secs === null) return TRIGGER_SHORT_FALLBACK;
    if (secs % 86400 === 0) return `每 ${secs / 86400} 天`;
    if (secs % 3600 === 0) return `每 ${secs / 3600} 小时`;
    if (secs % 60 === 0) return `每 ${secs / 60} 分钟`;
    return `每 ${secs} 秒`;
  }
  if ('Daily' in k) {
    const time = trimSeconds(k.Daily?.time);
    return time ? `每天 ${time}` : TRIGGER_SHORT_FALLBACK;
  }
  if ('Weekly' in k) {
    const days = Array.isArray(k.Weekly?.days_of_week) ? (k.Weekly.days_of_week as Weekday[]) : [];
    const daysText = formatWeekdaysShort(days, '、');
    const time = trimSeconds(k.Weekly?.time);
    if (!daysText || !time) return TRIGGER_SHORT_FALLBACK;
    return `每${daysText} ${time}`;
  }
  if ('Cron' in k) {
    const expression = k.Cron?.expression;
    if (typeof expression !== 'string') return TRIGGER_SHORT_FALLBACK;
    return describeCronShort(expression) ?? expression;
  }
  if ('Fuzzy' in k) {
    const f = k.Fuzzy ?? {};
    const start = trimSeconds(f.window_start);
    const end = trimSeconds(f.window_end);
    if (!start || !end) return TRIGGER_SHORT_FALLBACK;
    const period = f.period;
    let prefix: string;
    if (typeof period === 'string') {
      if (period === 'Daily') prefix = '每天';
      else if (period === 'Weekdays') prefix = '工作日';
      else if (period === 'Weekends') prefix = '周末';
      else return TRIGGER_SHORT_FALLBACK;
    } else if (period && Array.isArray(period.Weekly?.days_of_week)) {
      const daysText = formatWeekdaysShort(period.Weekly.days_of_week as Weekday[], '、');
      if (!daysText) return TRIGGER_SHORT_FALLBACK;
      prefix = `每${daysText}`;
    } else {
      return TRIGGER_SHORT_FALLBACK;
    }
    return `${prefix} ${start}-${end} 之间随机`;
  }
  if ('Network' in k) {
    const events = Array.isArray(k.Network?.events) ? (k.Network.events as unknown[]) : [];
    const labels = events
      .map((event) => NETWORK_SHORT_LABEL[String(event) as NetworkEventKind])
      .filter((label): label is string => typeof label === 'string');
    return labels.length > 0 ? labels.join('、') : TRIGGER_SHORT_FALLBACK;
  }
  return TRIGGER_SHORT_FALLBACK;
}
```

- [ ] **Step 4: 实现时间格式化工具**

在 `apps/desktop/src/types/task.ts` 的 `parseDate` 之后（`TaskOverview` 类型之前）插入：

```ts
/** 同自然年显示 MM-DD HH:mm，否则 YYYY-MM-DD HH:mm；一律按本机本地时区 */
export function formatDateTimeShort(
  iso?: string | null,
  now: Date = new Date()
): string {
  const ms = parseDate(iso);
  if (ms === null) return '—';
  const date = new Date(ms);
  const base = `${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
  return date.getFullYear() === now.getFullYear() ? base : `${date.getFullYear()}-${base}`;
}

/** 完整本地时间 YYYY-MM-DD HH:mm:ss（用于 title） */
export function formatDateTimeFull(iso?: string | null): string {
  const ms = parseDate(iso);
  if (ms === null) return '—';
  const date = new Date(ms);
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}`;
}

/** 相对本机当前时间的中文描述（用于 title） */
export function formatRelativeTime(
  iso?: string | null,
  now: Date = new Date()
): string {
  const ms = parseDate(iso);
  if (ms === null) return '未知时间';
  const diffMs = ms - now.getTime();
  const future = diffMs > 0;
  const absSecs = Math.round(Math.abs(diffMs) / 1000);
  if (absSecs < 60) return future ? '即将' : '刚刚';
  const label = (value: number, unit: string) =>
    future ? `${value} ${unit}后` : `${value} ${unit}前`;
  const mins = Math.round(absSecs / 60);
  if (mins < 60) return label(mins, '分钟');
  const hours = Math.round(mins / 60);
  if (hours < 24) return label(hours, '小时');
  return label(Math.round(hours / 24), '天');
}

/** title 属性：完整时间 + 相对时间，例如 2026-09-19 14:30:00（3 小时后） */
export function formatDateTimeTitle(
  iso?: string | null,
  now: Date = new Date()
): string {
  if (parseDate(iso) === null) return '—';
  return `${formatDateTimeFull(iso)}（${formatRelativeTime(iso, now)}）`;
}
```

- [ ] **Step 5: 运行测试确认通过**

运行：`pnpm -C apps/desktop test -- triggerShort`
预期：`Test Files 1 passed`，全部断言通过。

- [ ] **Step 6: 提交**

```bash
git add apps/desktop/src/types/task.ts apps/desktop/tests/triggerShort.test.ts
git commit -m "$(cat <<'EOF'
feat(desktop): 新增精简触发器描述与本地时间格式化

任务列表需要一眼可读的中文摘要，但不做完整 cron 渲染器：只识别常见模式，
复杂表达式回退原样显示。时间统一按本机时区渲染，避免同一屏出现多个时区。
EOF
)"
```

---

### Task 9: 任务克隆纯函数

**Files:**

- Modify: `apps/desktop/src/types/task.ts`（在 `getEmptyTask`（:259-284）之后新增）
- Test: `apps/desktop/tests/taskClone.test.ts`（新建）

**Interfaces:**

- Consumes: `Task`、`Trigger`、`Action`
- Produces: `cloneTaskForDuplicate(task: Task, now?: Date): Task`

**放在 `types/task.ts` 的理由：**它是与 `getEmptyTask` / `parseDate` 同层的纯任务领域工具，既有的 store、视图与测试都从这一个模块导入任务类型与助手；为单个函数新开模块只会增加导入面，不带来边界收益。

- [ ] **Step 1: 编写失败测试**

创建 `apps/desktop/tests/taskClone.test.ts`：

```ts
import { describe, it, expect } from 'vitest';
import { cloneTaskForDuplicate, getEmptyTask, type Task } from '../src/types/task';

function sampleTask(): Task {
  return {
    ...getEmptyTask(),
    id: 'task-original',
    name: '每日备份',
    description: '备份数据库',
    enabled: true,
    triggers: [
      {
        id: 'trig-1',
        task_id: 'task-original',
        enabled: true,
        kind: { Daily: { time: '09:00:00', timezone: 'UTC' } },
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
      },
      {
        id: 'trig-2',
        task_id: 'task-original',
        enabled: false,
        kind: 'AgentStarted',
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
      },
    ],
    actions: [
      {
        id: 'act-1',
        task_id: 'task-original',
        sequence: 1,
        enabled: true,
        kind: { ExecuteShell: { command: 'echo hi' } },
      },
    ],
    version: 4,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-02-01T00:00:00Z',
  };
}

describe('cloneTaskForDuplicate', () => {
  it('rebuilds ids, renames, disables and resets version and timestamps', () => {
    const original = sampleTask();
    const snapshot = JSON.stringify(original);
    const now = new Date(2026, 8, 19, 10, 0, 0);

    const copy = cloneTaskForDuplicate(original, now);

    expect(copy.id).not.toBe(original.id);
    expect(copy.name).toBe('每日备份 - 副本');
    expect(copy.enabled).toBe(false);
    expect(copy.version).toBe(1);
    expect(copy.created_at).toBe(now.toISOString());
    expect(copy.updated_at).toBe(now.toISOString());

    expect(copy.triggers).toHaveLength(2);
    expect(copy.triggers.every((trigger) => trigger.task_id === copy.id)).toBe(true);
    expect(copy.triggers.map((trigger) => trigger.id)).not.toContain('trig-1');
    expect(copy.triggers.map((trigger) => trigger.id)).not.toContain('trig-2');
    // 触发器自身的启用状态沿用原任务
    expect(copy.triggers[0].enabled).toBe(true);
    expect(copy.triggers[1].enabled).toBe(false);

    expect(copy.actions).toHaveLength(1);
    expect(copy.actions[0].task_id).toBe(copy.id);
    expect(copy.actions[0].id).not.toBe('act-1');
    expect(copy.actions[0].kind).toEqual({ ExecuteShell: { command: 'echo hi' } });

    // 其余字段与原任务一致
    expect(copy.description).toBe(original.description);
    expect(copy.working_directory).toBe(original.working_directory);
    expect(copy.environment).toEqual(original.environment);
    expect(copy.execution_policy).toEqual(original.execution_policy);

    // 原任务对象未被修改
    expect(JSON.stringify(original)).toBe(snapshot);
  });

  it('always disables the copy even when the original task is enabled', () => {
    const original = { ...sampleTask(), enabled: true };
    expect(cloneTaskForDuplicate(original).enabled).toBe(false);
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

运行：`pnpm -C apps/desktop test -- taskClone`
预期：FAIL，报 `cloneTaskForDuplicate is not a function`。

- [ ] **Step 3: 实现克隆函数**

在 `apps/desktop/src/types/task.ts` 的 `getEmptyTask` 之后插入：

```ts
function newLocalId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return 'id-' + Math.random().toString(36).substring(2, 11) + Date.now().toString(36);
}

/**
 * 生成用于「任务复制」的深拷贝副本：
 * 新 id、名称加 ` - 副本`、enabled 固定为 false（副本默认停用）、version 重置为 1、
 * created_at / updated_at 为 now；重建 triggers / actions 的 id 并让 task_id 指向新任务；
 * 其余字段与原任务一致。绝不在原任务对象上做任何写入。
 */
export function cloneTaskForDuplicate(task: Task, now: Date = new Date()): Task {
  const cloned = JSON.parse(JSON.stringify(task)) as Task;
  const id = newLocalId();
  const timestamp = now.toISOString();

  cloned.id = id;
  cloned.name = `${task.name} - 副本`;
  cloned.enabled = false;
  cloned.version = 1;
  cloned.created_at = timestamp;
  cloned.updated_at = timestamp;
  cloned.triggers = (cloned.triggers ?? []).map((trigger) => ({
    ...trigger,
    id: newLocalId(),
    task_id: id,
  }));
  cloned.actions = (cloned.actions ?? []).map((action) => ({
    ...action,
    id: newLocalId(),
    task_id: id,
  }));

  return cloned;
}
```

- [ ] **Step 4: 运行测试确认通过**

运行：`pnpm -C apps/desktop test -- taskClone`
预期：`Test Files 1 passed`。

- [ ] **Step 5: 提交**

```bash
git add apps/desktop/src/types/task.ts apps/desktop/tests/taskClone.test.ts
git commit -m "$(cat <<'EOF'
feat(desktop): 新增任务克隆纯函数

复制出的任务必须是与原任务完全解耦的新实体（新的任务/触发器/动作 id），
并且默认停用，避免副本保存后立刻按原计划开始触发。
EOF
)"
```

---

### Task 10: 列表「复制」入口 + 抽屉「复制任务」标题 + store 快照

**Files:**

- Modify: `apps/desktop/src/components/task/TaskDrawer.vue:5-35`、`:174-175`
- Modify: `apps/desktop/src/stores/taskStore.ts:1-65`
- Modify: `apps/desktop/src/views/TasksView.vue:1-11`、`:20-58`、`:226-230`
- Test: `apps/desktop/tests/taskDrawer.test.ts:13-19`、`:57-67`；`apps/desktop/tests/stores.test.ts:13-30`、文末；`apps/desktop/tests/views.test.ts:32-48`

**Interfaces:**

- Consumes: `cloneTaskForDuplicate`、`getTaskOverview`、`rerollTrigger`、`onExecutionFinished`
- Produces: `TaskDrawer` 的 `mode?: 'create' | 'edit' | 'copy'` prop；`taskStore.scheduleOverview` / `loadOverview()` / `rerollTrigger(taskId, triggerId)` / `initOverviewListener()`

- [ ] **Step 1: 编写失败测试**

编辑 `apps/desktop/tests/taskDrawer.test.ts`：把 tauri mock（:13-19）补全为

```ts
vi.mock('../src/services/tauri', () => ({
  listTasks: vi.fn(),
  getTask: vi.fn(),
  saveTask: vi.fn(),
  deleteTask: vi.fn(),
  triggerTask: vi.fn(),
  getTaskOverview: vi.fn().mockResolvedValue([]),
  rerollTrigger: vi.fn(),
}));
```

并在 `TaskDrawer defines expected props and emits` 测试中追加断言：

```ts
      expect((TaskDrawer as any).props.mode).toBeDefined();
```

编辑 `apps/desktop/tests/views.test.ts`：把 tauri mock（:32-42）补上 `getTaskOverview: vi.fn().mockResolvedValue([]),` 与 `rerollTrigger: vi.fn(),`。

编辑 `apps/desktop/tests/stores.test.ts`：把 tauri mock（:13-24）补上 `getTaskOverview: vi.fn().mockResolvedValue([]),` 与 `rerollTrigger: vi.fn(),`；在 `describe('useTaskStore', ...)` 内、`triggers task execution via service` 之后新增：

```ts
    it('loads schedule overview and updates a single trigger in place after reroll', async () => {
      const store = useTaskStore();
      vi.mocked(tauriService.getTaskOverview).mockResolvedValueOnce([
        {
          task_id: 'task-1',
          triggers: [{ trigger_id: 'trig-1', next_fire_at: '2026-09-19T09:00:00Z' }],
          last_run: null,
        },
      ]);

      await store.loadOverview();
      expect(store.scheduleOverview['task-1'].triggers).toHaveLength(1);

      vi.mocked(tauriService.rerollTrigger).mockResolvedValueOnce({
        next_fire_at: '2026-09-19T09:37:00Z',
      });
      const updated = await store.rerollTrigger('task-1', 'trig-1');

      expect(updated).toBe('2026-09-19T09:37:00Z');
      expect(store.scheduleOverview['task-1'].triggers[0].next_fire_at).toBe(
        '2026-09-19T09:37:00Z'
      );
      expect(tauriService.rerollTrigger).toHaveBeenCalledWith('task-1', 'trig-1');
    });

    it('silently degrades when the overview IPC fails', async () => {
      const store = useTaskStore();
      vi.mocked(tauriService.getTaskOverview).mockRejectedValueOnce(new Error('agent offline'));

      await expect(store.loadOverview()).resolves.toBeUndefined();

      expect(store.scheduleOverview).toEqual({});
    });

    it('refreshes the overview after an execution finished event', async () => {
      let finishedCallback: ((payload: unknown) => void) | undefined;
      vi.mocked(eventsService.onExecutionFinished).mockImplementation(async (cb) => {
        finishedCallback = cb as (payload: unknown) => void;
        return vi.fn();
      });
      vi.mocked(tauriService.getTaskOverview).mockResolvedValue([]);

      const store = useTaskStore();
      await store.initOverviewListener();
      expect(eventsService.onExecutionFinished).toHaveBeenCalledTimes(1);

      finishedCallback!({ execution_id: 'exec-1', task_id: 'task-1', status: 'Succeeded', exit_code: 0 });
      await Promise.resolve();
      expect(tauriService.getTaskOverview).toHaveBeenCalledTimes(1);
    });
```

- [ ] **Step 2: 运行测试确认失败**

运行：`pnpm -C apps/desktop test -- stores`
预期：FAIL，报 `store.loadOverview is not a function` / `props.mode` 为 `undefined`。

- [ ] **Step 3: `TaskDrawer` 新增 `mode` prop**

编辑 `apps/desktop/src/components/task/TaskDrawer.vue`：`<script setup>` 中的 import 改为 `import { computed, ref, watch } from 'vue';`，props 改为

```ts
const props = defineProps<{
  show: boolean;
  task: Task | null;
  mode?: 'create' | 'edit' | 'copy';
}>();
```

在 `const currentTask = ref<Task>(getEmptyTask());` 之后新增：

```ts
// 缺省时按既有行为从 task 真值推断，保证既有调用方不受影响
const drawerTitle = computed(() => {
  const resolved = props.mode ?? (props.task ? 'edit' : 'create');
  if (resolved === 'copy') return '复制任务';
  return resolved === 'edit' ? '编辑任务' : '新建任务';
});
```

模板中 `:title="task ? '编辑任务' : '新建任务'"` 改为 `:title="drawerTitle"`。

- [ ] **Step 4: `taskStore` 新增快照与订阅**

编辑 `apps/desktop/src/stores/taskStore.ts`：imports 改为

```ts
import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import {
  listTasks,
  saveTask as apiSaveTask,
  deleteTask as apiDeleteTask,
  triggerTask as apiTriggerTask,
  getTaskOverview,
  rerollTrigger as apiRerollTrigger,
} from '../services/tauri';
import { onExecutionFinished } from '../services/events';
import type { Task, TaskId, TaskOverview } from '../types/task';
```

在 `const loading = ref(false);` 之后新增状态；在 `loadTasks` 之后新增函数：

```ts
  const scheduleOverview = ref<Record<TaskId, TaskOverview>>({});
  let unlistenOverview: (() => void) | null = null;
  let overviewListenerStarted = false;

  async function loadOverview() {
    try {
      const list = await getTaskOverview();
      const next: Record<TaskId, TaskOverview> = {};
      for (const entry of list ?? []) {
        next[entry.task_id] = entry;
      }
      scheduleOverview.value = next;
    } catch {
      // 静默降级：agent 未运行 / IPC 失败时列表照常渲染，只是不显示时间信息
    }
  }

  async function rerollTrigger(taskId: TaskId, triggerId: string) {
    const result = await apiRerollTrigger(taskId, triggerId);
    const entry = scheduleOverview.value[taskId];
    if (entry) {
      scheduleOverview.value = {
        ...scheduleOverview.value,
        [taskId]: {
          ...entry,
          triggers: entry.triggers.map((trigger) =>
            trigger.trigger_id === triggerId
              ? { ...trigger, next_fire_at: result.next_fire_at }
              : trigger
          ),
        },
      };
    }
    return result.next_fire_at;
  }

  async function initOverviewListener() {
    if (overviewListenerStarted) return;
    overviewListenerStarted = true;
    try {
      unlistenOverview = await onExecutionFinished(() => {
        void loadOverview();
      });
    } catch {
      overviewListenerStarted = false;
      unlistenOverview = null;
    }
  }

  function cleanupOverviewListener() {
    if (typeof unlistenOverview === 'function') unlistenOverview();
    unlistenOverview = null;
    overviewListenerStarted = false;
  }
```

`loadTasks` 改为在拉取任务后刷新概览：

```ts
  async function loadTasks() {
    loading.value = true;
    try {
      tasks.value = await listTasks();
    } finally {
      loading.value = false;
    }
    await loadOverview();
  }
```

`return` 对象补充：`scheduleOverview, loadOverview, rerollTrigger, initOverviewListener, cleanupOverviewListener,`。

- [ ] **Step 5: `TasksView` 新增复制入口**

编辑 `apps/desktop/src/views/TasksView.vue`：

1. import 行（:4）新增 `Copy` 图标：

```ts
import { Plus, Search, Play, Edit2, Trash2, Download, Upload, Copy } from 'lucide-vue-next';
```

2. 类型 import（:11）改为：

```ts
import type { Task } from '../types/task';
import { cloneTaskForDuplicate } from '../types/task';
```

3. 在 `const editingTask = ref<Task | null>(null);`（:25）之后新增 `const drawerMode = ref<'create' | 'edit' | 'copy'>('create');`，并把 `openCreateDrawer` / `openEditDrawer`（:48-58）替换为下面三个函数（原有两个函数的其余内容不变）：

```ts
function openCreateDrawer() {
  drawerMode.value = 'create';
  editingTask.value = null;
  showDrawer.value = true;
  emit('create-task');
}

function openEditDrawer(task: Task) {
  drawerMode.value = 'edit';
  editingTask.value = JSON.parse(JSON.stringify(task));
  showDrawer.value = true;
  emit('edit-task', task);
}

// 深拷贝副本，点「保存任务」后才通过 task.save 落库；取消不产生任何数据
function openCopyDrawer(task: Task) {
  drawerMode.value = 'copy';
  editingTask.value = cloneTaskForDuplicate(task);
  showDrawer.value = true;
}
```

4. `onMounted` 改为：

```ts
onMounted(() => {
  taskStore.loadTasks();
  taskStore.initOverviewListener();
});
```

5. 行内按钮组（`:209-220` 的「编辑」按钮之后、删除按钮之前）新增：

```html
          <NButton size="small" secondary @click="openCopyDrawer(task)">
            <template #icon>
              <Copy class="w-3.5 h-3.5 text-slate-500" />
            </template>
            复制
          </NButton>
```

6. `TaskDrawer` 用法（:226-230）新增 `mode`：

```html
    <TaskDrawer
      v-model:show="showDrawer"
      :task="editingTask"
      :mode="drawerMode"
      @saved="taskStore.loadTasks()"
    />
```

- [ ] **Step 6: 运行测试确认通过**

运行：`pnpm -C apps/desktop test`
预期：`Test Files 10 passed`（含修改后的 stores / taskDrawer / views 测试）。

- [ ] **Step 7: 提交**

```bash
git add apps/desktop/src/components/task/TaskDrawer.vue apps/desktop/src/stores/taskStore.ts apps/desktop/src/views/TasksView.vue apps/desktop/tests/stores.test.ts apps/desktop/tests/taskDrawer.test.ts apps/desktop/tests/views.test.ts
git commit -m "$(cat <<'EOF'
feat(desktop): 任务列表支持复制并预取调度概览

复制沿用既有编辑抽屉，先在内存里克隆出停用副本、点保存才落库，
避免「点了复制就等于创建了一个马上会触发的任务」；
概览快照在任务加载与执行结束后刷新，读不到时静默降级。
EOF
)"
```

---

### Task 11: 列表展示触发器摘要、下次时间与上次执行

**Files:**

- Modify: `apps/desktop/src/views/TasksView.vue:1-11`、`:90-116`（script 末尾）、`:174-223`
- Test: 无新增测试文件；由 `pnpm -C apps/desktop test` 与 `pnpm -C apps/desktop run build` 保证不回归

**Interfaces:**

- Consumes: `taskStore.scheduleOverview`、`taskStore.rerollTrigger`、`describeTriggerShort`、`formatDateTimeShort`、`formatDateTimeTitle`、`getStatusLabel`、`getTriggerType`
- Produces: 每触发器一行摘要 + 下次时间（`—` 表示算不出）+ Fuzzy 刷新按钮；任务级上次执行行

- [ ] **Step 1: 补充 script 辅助函数**

编辑 `apps/desktop/src/views/TasksView.vue`：

1. `:4` 的图标 import 改为 `import { Plus, Search, Play, Edit2, Trash2, Download, Upload, Copy, RefreshCw } from 'lucide-vue-next';`
2. `:11` 的类型 import 改为下面三行（`export interface Task` 已删除，因此 `Task` 用 `import type` 引入）：

```ts
import type { Task, Trigger } from '../types/task';
import { cloneTaskForDuplicate, describeTriggerShort, formatDateTimeShort, formatDateTimeTitle, getTriggerType } from '../types/task';
import { getStatusLabel } from '../types/execution';
```

3. 在 `handleDelete` 之后新增：

```ts
const rerollingKey = ref<string | null>(null);

function triggerNextFire(taskId: string, triggerId: string): string | null {
  const entry = taskStore.scheduleOverview[taskId];
  return entry?.triggers.find((trigger) => trigger.trigger_id === triggerId)?.next_fire_at ?? null;
}

function nextFireText(taskId: string, triggerId: string): string {
  const iso = triggerNextFire(taskId, triggerId);
  return iso ? formatDateTimeShort(iso) : '—';
}

function nextFireTitle(taskId: string, triggerId: string): string {
  const iso = triggerNextFire(taskId, triggerId);
  return iso ? formatDateTimeTitle(iso) : '当前没有排定的下次触发时间';
}

function isFuzzyTrigger(trigger: Trigger): boolean {
  return getTriggerType(trigger.kind) === 'Fuzzy';
}

async function handleReroll(task: Task, trigger: Trigger) {
  const key = `${task.id}:${trigger.id}`;
  rerollingKey.value = key;
  try {
    await taskStore.rerollTrigger(task.id, trigger.id);
  } catch (e: any) {
    message.error('重摇失败: ' + (e?.message || e));
  } finally {
    rerollingKey.value = null;
  }
}

function formatDuration(durationMs: number | null): string {
  if (durationMs === null || durationMs === undefined) return '';
  const secs = durationMs / 1000;
  return secs < 60 ? `${secs.toFixed(1)}s` : `${Math.round(secs)}s`;
}

function errSummary(err: string): string {
  const oneLine = err.replace(/\s+/g, ' ').trim();
  return oneLine.length > 60 ? `${oneLine.slice(0, 60)}…` : oneLine;
}

function lastRunText(taskId: string): string {
  const lastRun = taskStore.scheduleOverview[taskId]?.last_run;
  if (!lastRun) return '上次 —';
  const base = `上次 ${formatDateTimeShort(lastRun.started_at)} ${getStatusLabel(lastRun.status)}`;
  if (lastRun.status === 'Succeeded') {
    const duration = formatDuration(lastRun.duration_ms);
    return duration ? `${base} · ${duration}` : base;
  }
  const summary = lastRun.error_message ? errSummary(lastRun.error_message) : '';
  return summary ? `${base} · ${summary}` : base;
}

function lastRunTitle(taskId: string): string {
  const lastRun = taskStore.scheduleOverview[taskId]?.last_run;
  return lastRun ? formatDateTimeTitle(lastRun.started_at) : '从未执行';
}

function lastRunClass(taskId: string): string {
  const status = taskStore.scheduleOverview[taskId]?.last_run?.status;
  return status === 'Failed' || status === 'TimedOut' || status === 'Interrupted'
    ? 'text-red-500'
    : 'text-slate-500 dark:text-zinc-400';
}
```

- [ ] **Step 2: 渲染触发器摘要与上次执行**

编辑 `apps/desktop/src/views/TasksView.vue` 的行内信息块（:185-198）：删掉「N 个触发器」的 `NTag`（保留动作数量），并在描述之后追加触发器行与上次执行行。改造后的块为：

```html
          <div>
            <div class="flex items-center gap-2">
              <span class="font-semibold text-sm">{{ task.name }}</span>
              <NTag size="small" :bordered="false" type="default">
                {{ task.actions.length }} 个动作
              </NTag>
            </div>
            <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">
              {{ task.description || '暂无描述' }}
            </p>
            <div
              v-for="trigger in task.triggers"
              :key="trigger.id"
              class="flex items-center gap-1.5 text-xs text-slate-500 dark:text-zinc-400 mt-1"
            >
              <span>{{ describeTriggerShort(trigger.kind) }}</span>
              <span>→ 下次</span>
              <span :title="nextFireTitle(task.id, trigger.id)">
                {{ nextFireText(task.id, trigger.id) }}
              </span>
              <NButton
                v-if="isFuzzyTrigger(trigger)"
                size="tiny"
                secondary
                :loading="rerollingKey === `${task.id}:${trigger.id}`"
                @click="handleReroll(task, trigger)"
              >
                <template #icon>
                  <RefreshCw class="w-3 h-3" />
                </template>
              </NButton>
            </div>
            <div class="text-xs mt-1" :class="lastRunClass(task.id)" :title="lastRunTitle(task.id)">
              {{ lastRunText(task.id) }}
            </div>
          </div>
```

- [ ] **Step 3: 验证前端测试与构建**

运行：

```bash
pnpm -C apps/desktop test
pnpm -C apps/desktop run build
```

预期：测试全绿；`vue-tsc --noEmit` 无类型错误，`vite build` 成功。

- [ ] **Step 4: 提交**

```bash
git add apps/desktop/src/views/TasksView.vue
git commit -m "$(cat <<'EOF'
feat(desktop): 任务列表展示触发器摘要、下次时间与上次执行

让「这个任务什么时候会跑、上次跑成什么样」在列表里一目了然；
Fuzzy 的随机点不可预测，因此单独给一个就地重摇按钮。
EOF
)"
```

---

### Task 12: 全栈质量门禁收尾与人工验收

**Files:**

- Modify: 无（只做验证；如门禁失败则回到对应任务修复）

**Interfaces:**

- Consumes: 前 11 个任务的全部改动
- Produces: 全绿的质量门禁与人工验收结论

- [ ] **Step 1: 后端全量测试**

运行：`cargo test --all`
预期：所有 crate 测试通过，无 `FAILED`。

- [ ] **Step 2: clippy 与格式检查**

运行：

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

预期：均无输出、退出码 0。

- [ ] **Step 3: 前端测试与构建**

运行：

```bash
pnpm -C apps/desktop test
pnpm -C apps/desktop run build
```

预期：vitest 全部通过；`vue-tsc --noEmit` 无错误，`vite build` 成功。

- [ ] **Step 4: 人工验收（设计 §8）**

在 macOS 上启动 `pnpm -C apps/desktop run dev` 并逐条确认：

1. 任务列表里点「复制」→ 抽屉标题显示「复制任务」且名称带 ` - 副本`；点「保存任务」后列表出现新任务，**启用开关为关闭状态**；打开原任务确认各字段未变。
2. 列表里每个触发器显示中文摘要与「下次 <时间>」；停用该任务后该任务的下次时间消失（显示 `—`）；任务级显示「上次 <时间> <状态> · <耗时>」，失败任务的时间行标红并带错误摘要。
3. 对 Fuzzy 触发器点刷新按钮 → 该行时间变化，同一任务其它触发器的下次时间保持不变。
4. 断开 agent（或让 agent 未运行）后刷新任务列表 → 列表照常渲染、不弹窗。

- [ ] **Step 5: 提交收尾（仅当 Step 1-3 有修复时）**

```bash
git add -A
git commit -m "$(cat <<'EOF'
chore: 修复质量门禁问题

保持 cargo test/clippy/fmt 与前端 test/build 全绿。
EOF
)"
```

---

## 设计 §2 / §7 / §9 覆盖对照

- §2「下次触发时间读队列快照、不持久化」→ Task 1、4；「启动/时钟跳变总是重算」→ 沿用既有 `run`，本计划不改；「Fuzzy 可手动重摇」→ Task 2、5、11。
- §2「复制后 `enabled` 固定 `false`」→ Task 9、10。
- §7「`task.overview` 失败静默降级」→ Task 10（`loadOverview` 吞异常）、Task 11（`—` 兜底）；「`trigger.reroll` 失败弹 `NMessage`」→ Task 11（`handleReroll` catch）；「未注册 / 停用返回错误」→ Task 2、5；「null 显示 `—`，刷新按钮只看触发器类型」→ Task 11。
- §9 YAGNI：未持久化 `next_fire_at`、未做完整 cron 渲染器、未动 `describeTrigger`、未把 `next_fire_at` 放进 `Task`/`Trigger` 领域模型或线格式；未新增除索引外的任何 migration。
