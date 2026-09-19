# 任务复制 + 任务列表调度概览 设计

日期：2026-09-19
状态：待评审

## 1. 背景与目标

本次新增两个功能：

1. **任务复制**：从任务列表复制一个任务，弹出编辑界面（预填副本），点「确定」后新增成一个独立的新任务。
2. **任务列表调度概览**：任务列表里展示每个触发器的简要描述（如「每天 9 点」）、各触发器的下次触发时间、以及该任务最近一次执行的时间与结果。

## 2. 已冻结的决策

| 决策点 | 结论 | 理由 |
|---|---|---|
| 下次触发时间的数据源 | **读调度队列快照**，不持久化 | 队列（`ScheduleQueue`）已是唯一真源；避免新增 migration 与「队列↔库」一致性同步 |
| 持久化 next_fire_at | **不做** | 「启动总是重算」已保证该值不跨重启有效，持久化只带来冗余与 drift 风险 |
| 启动/时钟跳变时 | **总是重算并覆盖队列中的值**（沿用现有行为） | 行为不变，风险最小；Fuzzy 重启即重摇（与现状一致） |
| Fuzzy 重摇 | 列表提供刷新按钮，可随时重摇 | 随机值需要可见 + 可手动重摇 |
| Cron 人性化描述 | **只翻译常见模式**，其它回退原表达式 | 不写完整 cron 中文渲染器 |
| 复制后的 `enabled` | **固定为 `false`（默认停用）** | 避免副本保存后立刻按原计划开始触发；由用户确认后再手动启用 |

## 3. 为什么前端不能自行计算下次触发时间

- `TriggerKind::Fuzzy` 的下次时间每次求值随机（`evaluator.rs` 用 `rand::thread_rng`），前端重算必然与后端不一致。
- `Cron` 需要 croner 解析，前端没有等价实现。

因此下次时间**必须**由 agent 提供。

## 4. 架构与数据流

```
Scheduler::run ──(Arc<Mutex<ScheduleQueue>>, 每个 (task,trigger) 的 next_fire_at)──┐
                                                                                    │
AgentService ── 持有该 Arc 的克隆，回答 task.overview ─────────────────────────────┤
                                                                                    │
ExecutionRepository ── task_runs 按 task 取最近一条 ──────────────────────────────┘
                                          │
                        Tauri 命令 task_overview
                                          │
                        services/tauri.ts: getTaskOverview()
                                          │
                        taskStore.scheduleOverview（渲染任务列表）
```

`next_fire_at` 不进入 `Task` 领域模型，也不进入 IPC 的 `Task` 线格式；它只出现在 `task.overview` 的响应里。这样 `task.save` 的往返不会污染调度运行时状态。

## 5. 后端改动

### 5.1 `crates/scheduler/src/queue.rs`

新增只读查询（遍历堆、忽略失效 generation、同一触发器取最早）：

```rust
/// 返回每个触发器的下次触发时间（仅统计当前 generation 有效的条目）
pub fn next_fire_by_trigger(&self) -> HashMap<(TaskId, TriggerId), DateTime<Utc>>;

/// 取出某任务的全部条目（其余任务保留），供重摇后按新 generation 重新入队
pub fn take_task_items(&mut self, task_id: &TaskId) -> Vec<ScheduledItem>;
```

### 5.2 `crates/scheduler/src/scheduler.rs`

新增命令，用于「只重摇一个触发器」：

```rust
SchedulerCommand::RerollTrigger { task_id: TaskId, trigger_id: TriggerId }
```

处理语义（**必须保证**）：
1. 目标触发器用 `now` 重新求值（Fuzzy 因此得到新的随机点）。
2. **同一任务其它触发器的下次时间保持不变**，不被推迟。
   - 反例（不可接受）：直接复用 `add_task` 会让同任务的 `Interval` 触发器以 `now` 为基准重算，把它的下次时间推后。
3. 重新入队的条目使用新的 generation（先 `bump_generation`，旧条目自然失效）。

### 5.3 `crates/persistence/src/execution_repo.rs`

新增批量查询，避免任务列表 N 次查询：

```rust
/// 每个任务最近一次执行（无记录的 task 不出现在结果中）
async fn find_latest_run_per_task(&self, task_ids: &[TaskId]) -> Result<HashMap<TaskId, Execution>>;
```

SQL 用窗口函数取每个 `task_id` 的 `started_at` 最新一行（SQLite ≥ 3.25 支持 `ROW_NUMBER()`），并列时以 `id DESC` 作为稳定的次级排序键（保证结果确定、测试不 flaky）；并新增复合索引 `(task_id, started_at)` 支撑该查询。

### 5.4 `apps/agent/src/service.rs`：新增两个 IPC 方法

**`task.overview`**（params `{}`）

```json
[
  {
    "task_id": "...",
    "triggers": [ { "trigger_id": "...", "next_fire_at": "2026-09-19T09:00:00Z" } ],
    "last_run": {
      "status": "Succeeded",
      "started_at": "2026-09-19T01:00:00Z",
      "finished_at": "2026-09-19T01:00:01Z",
      "duration_ms": 1234,
      "exit_code": 0,
      "error_message": null
    }
  }
]
```

- `next_fire_at` 为 `null` 表示当前队列中没有该触发器的有效条目（Network 事件触发、已过期的 Once、任务或触发器被停用）。
- `last_run` 为 `null` 表示从未执行过。
- 只返回任务已存在的触发器；`task.list` 里没有的任务不会出现。
- 读不到队列锁或查询失败时返回错误（见 §7）。

**`trigger.reroll`**（params `{ "task_id": "...", "trigger_id": "..." }`）

- 语义：对该触发器重新求值并重排（Fuzzy → 新随机点）。
- 返回：`{ "next_fire_at": "..." | null }`。
- 通用实现，不按触发器类型分支；UI 只在 Fuzzy 上暴露入口。

### 5.5 `apps/desktop/src-tauri/src/commands.rs`

新增两个命令 `task_overview`→`task.overview`、`reroll_trigger`→`trigger.reroll`，并在 `lib.rs` 注册。

## 6. 前端改动

### 6.1 任务复制

- 入口：`TasksView.vue` 行内按钮组新增「复制」（`lucide-vue-next` 的 `Copy` 图标，`NButton size="small" secondary`，与现有 `立即执行/编辑/删除` 同款）。
- 克隆（复用 `TaskImportModal.vue` 既有写法）：`JSON.parse(JSON.stringify(task))`，然后
  - `id` = `crypto.randomUUID()`；`name` = 原名 + ` - 副本`
  - 重建 `triggers[].id` / `actions[].id` 为新的 UUID，并把它们的 `task_id` 指向新 id
  - `created_at` / `updated_at` = now；`version` = 1
  - **`enabled` = `false`（固定停用，不沿用原任务的值）**
  - 其余字段（执行策略、动作、环境变量、工作目录等）与原任务一致
- 打开既有 `TaskDrawer`（`show=true`，`task` = 该副本）。抽屉标题在复制场景显示「复制任务」以区别于编辑。
- 点「确定」→ 既有 `handleSave` → `task.save`（upsert，新 id 即插入）→ `saved` → `taskStore.loadTasks()`。
- **约束**：编辑副本期间不得修改原任务对象（深拷贝保证）；取消（关闭抽屉）不产生任何数据。

### 6.2 列表展示区

任务行内在现有名称/描述下方新增：

1. **每个触发器一行**：`摘要 → 下次 09-19 14:30`；算不出时显示 `→ 下次 —`。
2. **Fuzzy 触发器**：在下次时间后紧跟一个刷新图标按钮，点击调 `trigger.reroll` 并就地更新该行时间。
3. **任务级一行上次执行**：`上次 09-19 09:00 成功 · 1.2s`；失败标红并显示 `error_message` 摘要；从未执行显示 `上次 —`。

原有的「N 个触发器 / N 个动作」文字被触发器摘要行取代（动作数量保留显示）。

### 6.3 新的精简触发器中描述函数

在 `apps/desktop/src/types/task.ts` 新增：

```ts
export function describeTriggerShort(kind: TriggerKind): string;
```

与既有 `describeTrigger` 并存（后者保留给任务对比视图与既有测试，不改动）。输出示例：

| 触发器 | 输出 |
|---|---|
| Daily 09:00 | `每天 09:00` |
| Weekly Mon-Fri 09:00 | `每周一至周五 09:00` |
| Interval 300s | `每 5 分钟` |
| Cron `0 9 * * *` | `每天 09:00` |
| Cron `*/5 * * * *` | `每 5 分钟` |
| Cron `0 9 * * 1-5` | `工作日 09:00` |
| Cron 其它/复杂表达式 | 回退为原表达式 |
| Fuzzy Daily 09:00-10:00 | `每天 09:00-10:00 之间随机` |
| Fuzzy Weekly | `每周一/周五 09:00-10:00 之间随机` |
| Network Connect | `连接网络时` |
| Once | `单次 09-19 14:30` |
| AgentStarted | `启动时` |

规则明细：
- **周几**:相邻连续的日子合并成区间（`周一至周五`），否则用 `、` 连接（`周一、周三`）。
- **Interval**:能被 86400 / 3600 / 60 整除时分别显示 `每 N 天 / N 小时 / N 分钟`，否则显示 `每 N 秒`。
- **Cron 常见模式**:「分 时 日 月 周」中，日/月为 `*` 时按「每天/每周几/工作日」识别；`*/N` 识别为「每 N 分钟」（但仅当小时字段为 `*`）；其余（含 `L`、范围组合、月字段非 `*` 等）回退为原表达式。
- **畸形输入**:与 `describeTrigger` 一致，返回兜底文案，不抛异常。

### 6.4 状态与刷新时机

- `taskStore` 新增 `scheduleOverview: Record<TaskId, { triggers: [...], lastRun: ... }>` 与 `loadOverview()`。
- 刷新时机：
  1. `loadTasks()` 之后（包含首次挂载、保存、删除之后）；
  2. 收到 `execution.finished` 事件后（下次时间会随触发重排、同时上次执行结果变化）；
  3. 重摇成功后就地更新该触发器的时间（不整体重载）。
- `taskStore` 目前没有事件订阅，本次在 store 初始化时订阅 `execution.finished`（沿用 `services/events.ts` 既有封装）。

### 6.5 时间显示格式

- 一律按**本机本地时区**显示。
- 若该时刻与本机当前时间处于**同一自然年**，显示 `MM-DD HH:mm`；否则显示 `YYYY-MM-DD HH:mm`。
- `title` 属性提供完整时间与相对时间（如 `2026-09-19 14:30:00（3 小时后）`）。
- 触发器自身的 `timezone` 只影响调度计算，不影响展示时区（避免同一屏幕出现多个时区）。

## 7. 错误处理

- `task.overview` 失败（agent 未运行、IPC 错误）：任务列表照常渲染，只是不显示时间信息；不弹窗、不阻断。
- `trigger.reroll` 失败：弹出 `NMessage` 错误提示，界面保持原值。
- `trigger.reroll` 对**任务未注册到调度器、或目标任务/触发器已停用**的情况返回错误（这些触发器不在队列中，不应被重摇），不做任何改动。
- `next_fire_at` 为 null：显示 `—`。刷新按钮只对 Fuzzy 触发器显示，与 `next_fire_at` 是否为 null 无关（null 时更需要重摇）。

## 8. 测试策略

**Rust**
- `ScheduleQueue::next_fire_by_trigger`：多任务多触发器、失效 generation 被忽略、同一触发器多条取最早。
- `ScheduleQueue::take_task_items`：只取目标任务、其余任务条目保留。
- `find_latest_run_per_task`：真实 SQLite 内存库，多任务多运行、无运行记录、`started_at` 并列。
- `SchedulerCommand::RerollTrigger` 集成测试：Fuzzy 重摇后值改变；**同任务其它触发器的下次时间不变**（回归防护）。
- `task.overview` IPC 测试：无触发器任务、从未执行的任务、Network 触发器（`next_fire_at` 为 null）。

**前端**
- `describeTriggerShort` 单测：上表每行 + 复杂 cron 回退 + 畸形输入不抛异常。
- 克隆函数单测：新 id、子 id 重建、名称后缀、**`enabled` 恒为 `false`（即使原任务启用）**、原任务对象未被修改。
- 时间格式化单测：跨年分支、相对时间。

**手工验收**
- 复制任务 → 新增成功、**副本为停用状态**、原任务各字段未变。
- 列表显示摘要/下次/上次；停用任务后下次时间消失。
- 点 Fuzzy 刷新 → 时间变化且同一任务其它触发器时间不变。

## 9. 明确不做（YAGNI）

- 不持久化 `next_fire_at`，不加 migration。
- 不实现完整 cron 中文渲染器（只做常见模式 + 回退）。
- 不做「错过的执行补跑」（`MissedRunPolicy` 仍未被 scheduler 消费，属既有遗留）。
- 列表里不做跳到执行日志的入口（只显示结果文本）。
- 不改 `describeTrigger` 的既有输出（避免影响任务对比视图与既有测试）。
- 不把 `next_fire_at` 加入 `Task`/`Trigger` 领域模型或 IPC 的 `Task` 线格式。
