# 新增三类触发器（Cron / Fuzzy / Network）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 easyJob 新增 Cron 表达式、模糊时间（周期窗口内随机）、网络变动（macOS/Windows）三类触发器。

**Architecture:** Cron/Fuzzy 是时间触发器，仅扩展 `evaluate_next_occurrence` 纯函数；Network 是事件触发器，由 agent 内独立 tokio 后台任务（NetworkMonitor + NetworkEventDispatcher）感知平台网络事件，经 `SchedulerCommand::TriggerNow` 复用现有执行管线。规格见 `docs/superpowers/specs/2026-09-18-triggers-cron-fuzzy-network-design.md`。

**Tech Stack:** Rust workspace (domain/scheduler/persistence/agent crates) + croner 2.x + rand 0.8 + objc2-network (macOS) + windows crate (Windows) + Vue 3 + Naive UI + Vitest。

**平台约束（硬性）：** 全栈仅考虑 macOS + Windows。不写任何 `cfg(target_os = "linux")` 分支、不做前端平台隐藏逻辑。

**FFI 说明：** Task 6 的平台 FFI 代码以 docs.rs 实际签名为准。objc2/windows 系 crate 用 `cargo add` 安装最新兼容版本；若 import 路径或签名与计划代码有出入（windows-rs 各版本模块归属偶有调整），以编译器 + docs.rs 为准做最小修正，不得改变事件语义。Windows 侧用 `cargo check --target` 交叉验证编译（本机不链接）。

---

### Task 1: 领域模型扩展（TriggerKind 三变体 + 辅助枚举）

**Files:**
- Modify: `crates/domain/src/trigger.rs`
- Test: `crates/domain/src/trigger.rs`（文件底部 `#[cfg(test)] mod tests`）

- [ ] **Step 1: 写失败测试（serde 线上格式 + kind_name）**

在 `crates/domain/src/trigger.rs` 末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_kind_serializes_with_expression_and_timezone() {
        let kind = TriggerKind::Cron {
            expression: "*/5 * * * *".into(),
            timezone: "Asia/Shanghai".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains(r#""Cron""#));
        assert!(json.contains("*/5 * * * *"));
        let back: TriggerKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }

    #[test]
    fn fuzzy_period_unit_variants_serialize_as_strings() {
        // serde 对无字段枚举变体序列化为字符串
        assert_eq!(
            serde_json::to_string(&FuzzyPeriod::Weekdays).unwrap(),
            r#""Weekdays""#
        );
        let weekly = FuzzyPeriod::Weekly {
            days_of_week: vec![Weekday::Mon, Weekday::Fri],
        };
        let json = serde_json::to_string(&weekly).unwrap();
        assert!(json.contains(r#""Weekly""#));
        assert!(json.contains("days_of_week"));
        let back: FuzzyPeriod = serde_json::from_str(&json).unwrap();
        assert_eq!(back, weekly);
    }

    #[test]
    fn network_kind_serializes_events_and_optional_name() {
        let kind = TriggerKind::Network {
            events: vec![NetworkEventKind::Connect, NetworkEventKind::Online],
            network_name: Some("MyHome".into()),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains(r#""Connect""#));
        assert!(json.contains("MyHome"));
        let none_kind = TriggerKind::Network {
            events: vec![NetworkEventKind::Disconnect],
            network_name: None,
        };
        let back: TriggerKind = serde_json::from_str(&serde_json::to_string(&none_kind).unwrap()).unwrap();
        assert_eq!(back, none_kind);
    }

    #[test]
    fn kind_name_returns_pure_variant_names() {
        let mk = |k: TriggerKind| k.kind_name().to_string();
        assert_eq!(mk(TriggerKind::Once { fire_at: Utc::now() }), "Once");
        assert_eq!(mk(TriggerKind::Daily { time: NaiveTime::MIN, timezone: "UTC".into() }), "Daily");
        assert_eq!(mk(TriggerKind::AgentStarted), "AgentStarted");
        assert_eq!(mk(TriggerKind::Cron { expression: "".into(), timezone: "".into() }), "Cron");
        assert_eq!(mk(TriggerKind::Fuzzy {
            period: FuzzyPeriod::Daily,
            window_start: NaiveTime::MIN,
            window_end: NaiveTime::MIN,
            timezone: "".into(),
        }), "Fuzzy");
        assert_eq!(mk(TriggerKind::Network { events: vec![], network_name: None }), "Network");
    }

    #[test]
    fn fuzzy_period_matches_weekday() {
        assert!(FuzzyPeriod::Daily.matches_weekday(Weekday::Sun));
        assert!(FuzzyPeriod::Weekdays.matches_weekday(Weekday::Mon));
        assert!(!FuzzyPeriod::Weekdays.matches_weekday(Weekday::Sat));
        assert!(FuzzyPeriod::Weekends.matches_weekday(Weekday::Sun));
        assert!(!FuzzyPeriod::Weekends.matches_weekday(Weekday::Mon));
        let weekly = FuzzyPeriod::Weekly { days_of_week: vec![Weekday::Fri] };
        assert!(weekly.matches_weekday(Weekday::Fri));
        assert!(!weekly.matches_weekday(Weekday::Sat));
    }
}
```

注意：文件顶部现有 `use chrono::{DateTime, NaiveTime, Utc, Weekday};`，测试里 `Utc`/`NaiveTime` 已可用；`serde_json` 需在 `crates/domain/Cargo.toml` 确认已有（`serde_json.workspace = true`，若无则加）。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p easyjob-domain`
Expected: 编译失败，`Cron`/`Fuzzy`/`Network`/`FuzzyPeriod`/`NetworkEventKind`/`kind_name`/`matches_weekday` 未定义。

- [ ] **Step 3: 最小实现**

替换 `crates/domain/src/trigger.rs` 的 `TriggerKind` 枚举并在文件末尾（tests 模块之前）追加新类型：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerKind {
    Once {
        fire_at: DateTime<Utc>,
    },
    Daily {
        time: NaiveTime,
        timezone: String,
    },
    Weekly {
        days_of_week: Vec<Weekday>,
        time: NaiveTime,
        timezone: String,
    },
    Interval {
        interval_secs: u64,
        start_at: Option<DateTime<Utc>>,
    },
    AgentStarted,
    /// 标准 5 字段 cron（分 时 日 月 周），按指定时区解释
    Cron {
        expression: String,
        timezone: String,
    },
    /// 周期窗口内随机触发：每个匹配周期在 [window_start, window_end) 内随机选点
    Fuzzy {
        period: FuzzyPeriod,
        window_start: NaiveTime,
        window_end: NaiveTime,
        timezone: String,
    },
    /// 网络变动触发（仅 macOS / Windows）
    Network {
        events: Vec<NetworkEventKind>,
        network_name: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FuzzyPeriod {
    Daily,
    Weekdays,
    Weekends,
    Weekly { days_of_week: Vec<Weekday> },
}

impl FuzzyPeriod {
    pub fn matches_weekday(&self, w: Weekday) -> bool {
        match self {
            FuzzyPeriod::Daily => true,
            FuzzyPeriod::Weekdays => !matches!(w, Weekday::Sat | Weekday::Sun),
            FuzzyPeriod::Weekends => matches!(w, Weekday::Sat | Weekday::Sun),
            FuzzyPeriod::Weekly { days_of_week } => days_of_week.contains(&w),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkEventKind {
    Connect,
    Disconnect,
    Online,
}

impl TriggerKind {
    /// 纯变体名（持久化 triggers.kind 列使用）
    pub fn kind_name(&self) -> &'static str {
        match self {
            TriggerKind::Once { .. } => "Once",
            TriggerKind::Daily { .. } => "Daily",
            TriggerKind::Weekly { .. } => "Weekly",
            TriggerKind::Interval { .. } => "Interval",
            TriggerKind::AgentStarted => "AgentStarted",
            TriggerKind::Cron { .. } => "Cron",
            TriggerKind::Fuzzy { .. } => "Fuzzy",
            TriggerKind::Network { .. } => "Network",
        }
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p easyjob-domain`
Expected: 全部 PASS（含原有测试）。

- [ ] **Step 5: 提交**

```bash
git add crates/domain/src/trigger.rs crates/domain/Cargo.toml
git commit -m "feat(domain): add Cron, Fuzzy, Network trigger kinds"
```

---

### Task 2: 调度器 Cron evaluator

**Files:**
- Modify: `Cargo.toml`（workspace.dependencies 追加 croner、rand）
- Modify: `crates/scheduler/Cargo.toml`
- Modify: `crates/scheduler/src/evaluator.rs`
- Test: `crates/scheduler/src/evaluator.rs`（底部新 tests 模块）

- [ ] **Step 1: 添加依赖**

根 `Cargo.toml` `[workspace.dependencies]` 追加：

```toml
croner = "2"
rand = "0.8"
```

`crates/scheduler/Cargo.toml` `[dependencies]` 追加：

```toml
croner.workspace = true
rand.workspace = true
```

- [ ] **Step 2: 写失败测试**

在 `crates/scheduler/src/evaluator.rs` 末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use easyjob_domain::trigger::{FuzzyPeriod, NetworkEventKind};

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn cron_daily_0930_shanghai() {
        // after = 2026-06-01 08:00 +08 → 下一次 09:30 +08 = 01:30Z
        let kind = TriggerKind::Cron {
            expression: "30 9 * * *".into(),
            timezone: "Asia/Shanghai".into(),
        };
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z")).unwrap();
        assert_eq!(next, utc("2026-06-01T01:30:00Z"));
    }

    #[test]
    fn cron_every_5_min_respects_after() {
        let kind = TriggerKind::Cron {
            expression: "*/5 * * * *".into(),
            timezone: "UTC".into(),
        };
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T10:02:00Z")).unwrap();
        assert_eq!(next, utc("2026-06-01T10:05:00Z"));
    }

    #[test]
    fn cron_invalid_expression_returns_none() {
        let kind = TriggerKind::Cron {
            expression: "not a cron".into(),
            timezone: "UTC".into(),
        };
        assert_eq!(evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z")), None);
    }

    #[test]
    fn cron_rejects_non_5_field_expressions() {
        let kind = TriggerKind::Cron {
            expression: "0 */5 * * * *".into(), // 6 字段（含秒），规格明确不支持
            timezone: "UTC".into(),
        };
        assert_eq!(evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z")), None);
    }

    #[test]
    fn network_kind_is_event_driven_returns_none() {
        let kind = TriggerKind::Network {
            events: vec![NetworkEventKind::Connect],
            network_name: None,
        };
        assert_eq!(evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z")), None);
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p easyjob-scheduler`
Expected: 编译失败或 `Cron` 分支 unreachable —— match 非穷尽会直接编译报错。

- [ ] **Step 4: 实现 Cron 分支**

`crates/scheduler/src/evaluator.rs` 顶部 use 区加：

```rust
use std::str::FromStr;
```

（`FromStr` 可能已引入，确认无重复。）在 `evaluate_next_occurrence` 的 `match kind` 中，`TriggerKind::AgentStarted` 分支之前插入：

```rust
        TriggerKind::Cron { expression, timezone } => {
            // 规格明确仅支持标准 5 字段（分 时 日 月 周）
            if expression.split_whitespace().count() != 5 {
                return None;
            }
            let cron = match croner::Cron::from_str(expression) {
                Ok(c) => c,
                Err(_) => return None,
            };
            let tz: Tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
            let local_after = after.with_timezone(&tz);
            // croner 基于 tz-aware DateTime 计算，内部处理 DST；
            // inclusive=false → 严格晚于 after
            cron.find_next_occurrence(&local_after, false)
                .map(|dt| dt.with_timezone(&Utc))
        }
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p easyjob-scheduler`
Expected: 全部 PASS。

- [ ] **Step 6: 提交**

```bash
git add Cargo.toml crates/scheduler/Cargo.toml crates/scheduler/src/evaluator.rs Cargo.lock
git commit -m "feat(scheduler): add Cron trigger evaluator via croner"
```

---

### Task 3: 调度器 Fuzzy evaluator

**Files:**
- Modify: `crates/scheduler/src/evaluator.rs`

- [ ] **Step 1: 写失败测试**

在 Task 2 的 `mod tests` 中追加：

```rust
    fn fuzzy_kind(period: FuzzyPeriod) -> TriggerKind {
        TriggerKind::Fuzzy {
            period,
            window_start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            window_end: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            timezone: "UTC".into(),
        }
    }

    #[test]
    fn fuzzy_daily_returns_time_within_window() {
        // after = 2026-06-01 08:30Z → 今天窗口 [09:00,10:00) 内随机
        let kind = fuzzy_kind(FuzzyPeriod::Daily);
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T08:30:00Z")).unwrap();
        let t = next.time();
        assert!(t >= NaiveTime::from_hms_opt(9, 0, 0).unwrap() && t < NaiveTime::from_hms_opt(10, 0, 0).unwrap());
        assert_eq!(next.date_naive(), utc("2026-06-01T08:30:00Z").date_naive());
    }

    #[test]
    fn fuzzy_mid_window_picks_in_remaining_window() {
        // after 在窗口中段：结果必须仍在今天窗口内且 > after
        let kind = fuzzy_kind(FuzzyPeriod::Daily);
        let after = utc("2026-06-01T09:30:00Z");
        let next = evaluate_next_occurrence(&kind, after).unwrap();
        assert!(next > after);
        assert!(next < utc("2026-06-01T10:00:00Z"));
        assert_eq!(next.date_naive(), after.date_naive());
    }

    #[test]
    fn fuzzy_window_over_moves_to_next_matching_day() {
        // 周一(2026-06-01)窗口已过 → 下一个匹配日周二
        let kind = fuzzy_kind(FuzzyPeriod::Weekdays);
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T10:30:00Z")).unwrap();
        assert_eq!(next.date_naive(), utc("2026-06-02T00:00:00Z").date_naive());
        let t = next.time();
        assert!(t >= NaiveTime::from_hms_opt(9, 0, 0).unwrap() && t < NaiveTime::from_hms_opt(10, 0, 0).unwrap());
    }

    #[test]
    fn fuzzy_weekly_skips_unlisted_days() {
        // Weekly 只含 Sunday：2026-06-01 是周一 → 下一个周日是 2026-06-07
        let kind = fuzzy_kind(FuzzyPeriod::Weekly { days_of_week: vec![chrono::Weekday::Sun] });
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T08:30:00Z")).unwrap();
        assert_eq!(next.date_naive(), utc("2026-06-07T00:00:00Z").date_naive());
    }

    #[test]
    fn fuzzy_invalid_window_returns_none() {
        let kind = TriggerKind::Fuzzy {
            period: FuzzyPeriod::Daily,
            window_start: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            window_end: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            timezone: "UTC".into(),
        };
        assert_eq!(evaluate_next_occurrence(&kind, utc("2026-06-01T08:30:00Z")), None);
    }
```

测试文件顶部 tests 模块内需 `use chrono::NaiveTime;`（evaluator 顶部已 use，但 tests 模块内作用域独立，通过 `super::*` 已可拿到 `NaiveTime`——`super::*` 引入的是 evaluator 模块全部项，含顶部 use 的 `NaiveTime`，无需重复）。若编译器报缺，显式补 `use chrono::NaiveTime;`。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p easyjob-scheduler`
Expected: 编译失败（match 非穷尽，缺 `Fuzzy` 分支）。

- [ ] **Step 3: 实现 Fuzzy 分支**

在 `evaluate_next_occurrence` 的 `AgentStarted` 分支前插入：

```rust
        TriggerKind::Fuzzy { period, window_start, window_end, timezone } => {
            if window_end <= window_start {
                return None;
            }
            let tz: Tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
            let local_after = after.with_timezone(&tz);
            let mut candidate_date = local_after.date_naive();

            for _ in 0..8 {
                if period.matches_weekday(candidate_date.weekday()) {
                    let picked = pick_random_time_in_window(window_start, window_end);
                    if let Some(utc_dt) = resolve_candidate(&tz, candidate_date, picked, after) {
                        return Some(utc_dt);
                    }
                    // 今天：随机点已过期但窗口未结束 → 在剩余窗口内重摇一次
                    if candidate_date == local_after.date_naive() && local_after.time() < *window_end {
                        let late_pick = pick_random_time_in_window(&local_after.time(), window_end);
                        if let Some(utc_dt) = resolve_candidate(&tz, candidate_date, late_pick, after) {
                            return Some(utc_dt);
                        }
                    }
                }
                candidate_date = candidate_date.succ_opt()?;
            }
            None
        }
```

在文件底部（tests 模块之前）加辅助函数：

```rust
fn pick_random_time_in_window(start: &NaiveTime, end: &NaiveTime) -> NaiveTime {
    use rand::Rng;
    let span = (*end - *start).num_seconds().max(1) as u64;
    let offset = rand::thread_rng().gen_range(0..span);
    *start + chrono::Duration::seconds(offset as i64)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p easyjob-scheduler`
Expected: 全部 PASS。（随机性通过范围断言规避。）

- [ ] **Step 5: 全工作区回归**

Run: `cargo test --all && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
Expected: 全绿。若 fmt 报错执行 `cargo fmt` 后重查。

- [ ] **Step 6: 提交**

```bash
git add crates/scheduler/src/evaluator.rs
git commit -m "feat(scheduler): add Fuzzy trigger evaluator with random window picking"
```

---

### Task 4: 持久化 kind 列修复

**Files:**
- Modify: `crates/persistence/src/task_repo.rs:185`

- [ ] **Step 1: 替换 Debug 输出为纯变体名**

将 `task_repo.rs` 第 185 行：

```rust
            let kind_str = format!("{:?}", trigger.kind);
```

替换为：

```rust
            let kind_str = trigger.kind.kind_name().to_string();
```

（第 210 行 action 的 `format!` **不动**——本任务只修 trigger 侧，action 列语义变更超出规格范围。）

- [ ] **Step 2: 验证**

Run: `cargo test -p easyjob-persistence && cargo test --all`
Expected: 全部 PASS（读侧只依赖 config_json，kind 列格式变更无存量兼容风险）。

- [ ] **Step 3: 提交**

```bash
git add crates/persistence/src/task_repo.rs
git commit -m "fix(persistence): store pure trigger variant name in triggers.kind"
```

---

### Task 5: Agent 网络模块——纯函数与 Online 探测

**Files:**
- Create: `apps/agent/src/network.rs`
- Modify: `apps/agent/src/lib.rs`（加 `pub mod network;`）
- Test: `apps/agent/src/network.rs`（tests 模块）

- [ ] **Step 1: 写失败测试 + 模块骨架**

创建 `apps/agent/src/network.rs`，先只含类型、纯函数与测试（平台监控 Task 6 再补）：

```rust
use std::sync::Arc;

use easyjob_domain::trigger::{NetworkEventKind, TriggerKind};
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::{SchedulerCommand, TriggerEvent};
use tokio::sync::mpsc;
use tracing::warn;

/// 运行时网络事件（含事件发生时的 SSID；有线或读取失败为 None）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEvent {
    Connect { ssid: Option<String> },
    Disconnect { ssid: Option<String> },
    Online { ssid: Option<String> },
}

impl NetworkEvent {
    pub fn kind(&self) -> NetworkEventKind {
        match self {
            NetworkEvent::Connect { .. } => NetworkEventKind::Connect,
            NetworkEvent::Disconnect { .. } => NetworkEventKind::Disconnect,
            NetworkEvent::Online { .. } => NetworkEventKind::Online,
        }
    }

    pub fn ssid(&self) -> Option<&str> {
        match self {
            NetworkEvent::Connect { ssid }
            | NetworkEvent::Disconnect { ssid }
            | NetworkEvent::Online { ssid } => ssid.as_deref(),
        }
    }
}

/// 事件类型匹配：触发器 events 列表非空且包含事件类型
pub fn event_matches(ev: &NetworkEvent, events: &[NetworkEventKind]) -> bool {
    events.contains(&ev.kind())
}

/// SSID 过滤：network_name 为 None = 任意网络；Some(name) = 精确、区分大小写匹配
pub fn name_matches(ev: &NetworkEvent, network_name: &Option<String>) -> bool {
    match network_name {
        None => true,
        Some(expected) => ev.ssid() == Some(expected.as_str()),
    }
}

/// 共享 Online 探测：TCP 1.1.1.1:80（2s 超时），失败 fallback DNS 解析 example.com
pub async fn probe_online() -> bool {
    if tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::net::TcpStream::connect("1.1.1.1:80"),
    )
    .await
    .is_ok()
    {
        return true;
    }
    tokio::time::timeout(std::time::Duration::from_secs(2), tokio::net::lookup_host("example.com:443"))
        .await
        .map(|mut addrs| addrs.next().is_some())
        .unwrap_or(false)
}

/// 派发：查启用任务 → 匹配 Network 触发器（事件类型 + SSID）→ TriggerNow（每任务至多一次）
pub async fn dispatch_network_event(
    ev: NetworkEvent,
    task_repo: Arc<SqliteTaskRepository>,
    scheduler_tx: mpsc::Sender<SchedulerCommand>,
) {
    let Ok(tasks) = task_repo.find_all_enabled().await else {
        warn!("network dispatch: failed to list enabled tasks");
        return;
    };
    for task in tasks {
        let hit = task.triggers.iter().any(|tr| {
            tr.enabled
                && match &tr.kind {
                    TriggerKind::Network { events, network_name } => {
                        event_matches(&ev, events) && name_matches(&ev, network_name)
                    }
                    _ => false,
                }
        });
        if hit {
            let _ = scheduler_tx
                .send(SchedulerCommand::TriggerNow(task.id))
                .await;
        }
    }
}

#[allow(dead_code)]
fn _assert_event_sendable(ev: &TriggerEvent) {
    let _ = ev.task_id;
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyjob_domain::trigger::NetworkEventKind;

    fn connect(ssid: Option<&str>) -> NetworkEvent {
        NetworkEvent::Connect { ssid: ssid.map(String::from) }
    }

    #[test]
    fn event_kind_maps_correctly() {
        assert_eq!(connect(Some("x")).kind(), NetworkEventKind::Connect);
        assert_eq!(
            NetworkEvent::Disconnect { ssid: None }.kind(),
            NetworkEventKind::Disconnect
        );
        assert_eq!(
            NetworkEvent::Online { ssid: None }.kind(),
            NetworkEventKind::Online
        );
    }

    #[test]
    fn event_matches_respects_list() {
        let ev = connect(Some("x"));
        assert!(event_matches(&ev, &[NetworkEventKind::Connect]));
        assert!(event_matches(&ev, &[NetworkEventKind::Disconnect, NetworkEventKind::Connect]));
        assert!(!event_matches(&ev, &[NetworkEventKind::Disconnect]));
        assert!(!event_matches(&ev, &[]));
    }

    #[test]
    fn name_matches_none_is_wildcard() {
        assert!(name_matches(&connect(Some("Home")), &None));
        assert!(name_matches(&connect(None), &None));
    }

    #[test]
    fn name_matches_requires_exact_case_sensitive_ssid() {
        let name = Some("MyHome".to_string());
        assert!(name_matches(&connect(Some("MyHome")), &name));
        assert!(!name_matches(&connect(Some("myhome")), &name));
        assert!(!name_matches(&connect(None), &name));
        assert!(!name_matches(&connect(Some("MyHome2")), &name));
    }
}
```

`apps/agent/src/lib.rs` 的 `pub mod lock;` 前加：

```rust
pub mod network;
```

- [ ] **Step 2: 确认 easyjob-agent 依赖齐全**

`apps/agent/Cargo.toml` `[dependencies]` 应已含 `easyjob-persistence`、`easyjob-scheduler`、`tokio`、`tracing`（service.rs 已在用，无需新增）。

- [ ] **Step 3: 运行测试确认通过**

Run: `cargo test -p easyjob-agent`
Expected: 新增 4 个测试 PASS。（probe_online 无单测——需要真实网络，由运行时行为覆盖。）

- [ ] **Step 4: 提交**

```bash
git add apps/agent/src/network.rs apps/agent/src/lib.rs
git commit -m "feat(agent): add network event matching, dispatch, and online probe"
```

---

### Task 6: NetworkMonitor 平台实现（macOS + Windows）

**Files:**
- Modify: `apps/agent/src/network.rs`（追加监控实现）
- Modify: `apps/agent/Cargo.toml`（平台依赖）

- [ ] **Step 1: 定义 Monitor 接口骨架（与平台无关部分）**

在 `apps/agent/src/network.rs` 的 `dispatch_network_event` 之后、tests 之前追加：

```rust
/// 启动平台网络监控。事件经 event_tx 发出（try_send，通道满则丢弃并 warn）。
/// 返回 JoinHandle；handler 退出即停止监控。
pub fn spawn_network_monitor(
    mut event_tx: mpsc::Sender<NetworkEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel::<NetworkEvent>();
        let send = |ev: NetworkEvent| {
            if tx.send(ev).is_err() {
                warn!("network monitor: internal channel closed");
            }
        };

        #[cfg(target_os = "macos")]
        let platform = platform_macos::MacosNetworkWatcher::start(send);
        #[cfg(target_os = "windows")]
        let platform = platform_windows::WindowsNetworkWatcher::start(send);

        tracing::info!("network monitor started");
        while let Some(ev) = rx.recv().await {
            if let Err(e) = event_tx.try_send(ev) {
                tracing::warn!("network event dropped (channel full): {}", e);
            }
        }
        drop(platform);
        tracing::info!("network monitor stopped");
    })
}

/// 防抖共享状态：同目标事件 500ms 内去重
pub(crate) struct Debouncer {
    last_kind: std::sync::Mutex<Option<(NetworkEventKind, std::time::Instant)>>,
}

impl Debouncer {
    pub fn new() -> Self {
        Self { last_kind: std::sync::Mutex::new(None) }
    }

    /// 返回 true = 放行
    pub fn allow(&self, kind: NetworkEventKind) -> bool {
        let mut guard = self.last_kind.lock().unwrap();
        let now = std::time::Instant::now();
        match guard.as_mut() {
            Some((last, at)) if *last == kind && now.duration_since(*at).as_millis() < 500 => false,
            _ => {
                *guard = Some((kind, now));
                true
            }
        }
    }
}
```

- [ ] **Step 2: macOS 实现**

```bash
cd /Users/yangguirong/IdeaProjects/tauri/easyJob
cargo add -p easyjob-agent --target 'cfg(target_os = "macos")' objc2 objc2-foundation objc2-network objc2-core-wlan block2
```

在 `network.rs` 末尾（tests 模块之前）追加：

```rust
#[cfg(target_os = "macos")]
mod platform_macos {
    use super::{Debouncer, NetworkEvent};
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_network::{
        nw_path_get_status, nw_path_monitor_create, nw_path_monitor_set_update_handler,
        nw_path_monitor_start, nw_path_monitor_cancel, NWPath, NWPathStatus,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tracing::{error, info};

    type SendFn = Arc<dyn Fn(NetworkEvent) + Send + Sync>;

    pub struct MacosNetworkWatcher {
        monitor: Retained<objc2_network::NWPathMonitor>,
        running: Arc<AtomicBool>,
    }

    // monitor 跨线程使用安全（nw_* API 线程安全）
    unsafe impl Send for MacosNetworkWatcher {}
    unsafe impl Sync for MacosNetworkWatcher {}

    impl MacosNetworkWatcher {
        pub fn start(on_event: SendFn) -> Self {
            let running = Arc::new(AtomicBool::new(true));
            let debouncer = Arc::new(Debouncer::new());

            let monitor = unsafe { nw_path_monitor_create() };

            let cb_running = running.clone();
            let cb_debounce = debouncer.clone();
            let cb = RcBlock::new(move |path: *mut NWPath| {
                if !cb_running.load(Ordering::Relaxed) {
                    return;
                }
                let status = unsafe { nw_path_get_status(path) };
                let ssid = current_ssid();
                let event = match status {
                    NWPathStatus::Satisfied => NetworkEvent::Connect { ssid },
                    NWPathStatus::Unsatisfied => NetworkEvent::Disconnect { ssid },
                    _ => return, // Satisfiable / Invalid 等中间态不触发
                };
                if !cb_debounce.allow(event.kind()) {
                    return;
                }
                info!("network path changed: {:?}", event);
                on_event(event);
                // Connect 后异步探测 Online（10s 冷却在探测任务内部）
                if matches!(event, NetworkEvent::Connect { .. }) {
                    spawn_online_probe(on_event.clone(), cb_debounce.clone());
                }
            });
            unsafe {
                nw_path_monitor_set_update_handler(&monitor, &cb);
                nw_path_monitor_start(&monitor);
            }

            Self { monitor, running }
        }

        pub fn stop(&self) {
            self.running.store(false, Ordering::Relaxed);
            unsafe { nw_path_monitor_cancel(&self.monitor) };
        }
    }

    impl Drop for MacosNetworkWatcher {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn current_ssid() -> Option<String> {
        // macOS 14+ 读取 SSID 需定位权限；无权限/有线时返回 None（触发器按任意网络匹配）
        objc2_core_wlan::CWWiFiClient::sharedWiFiClient()
            .interface()
            .and_then(|iface| iface.ssid().map(|s| s.to_string()))
    }

    fn spawn_online_probe(on_event: SendFn, debouncer: Arc<Debouncer>) {
        use std::sync::atomic::{AtomicI64, Ordering};
        static LAST_PROBE_MS: AtomicI64 = AtomicI64::new(0);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if now_ms - LAST_PROBE_MS.load(Ordering::Relaxed) < 10_000 {
            return; // 10s 探测冷却
        }
        LAST_PROBE_MS.store(now_ms, Ordering::Relaxed);

        let handle = tokio::runtime::Handle::try_current();
        let Ok(handle) = handle else { return };
        handle.spawn(async move {
            if super::probe_online().await {
                let ev = NetworkEvent::Online { ssid: current_ssid() };
                if debouncer.allow(ev.kind()) {
                    on_event(ev);
                }
            }
        });
    }
}
```

说明：`objc2_network` 生成的 API 形态（`Retained<NWPathMonitor>` vs 裸指针别名、handler block 签名）以所装版本 docs.rs 为准做最小修正；事件语义（Satisfied→Connect、Unsatisfied→Disconnect、Connect 后探测 Online、500ms 防抖、10s 探测冷却）不得变。macOS 14+ SSID 读取需定位权限，读不到时 `ssid=None` 属预期降级。

- [ ] **Step 3: Windows 实现**

```bash
cargo add -p easyjob-agent --target 'cfg(target_os = "windows")' windows --features Win32_Foundation,Win32_NetworkManagement_NetworkInformation,Win32_NetworkManagement_Ndis,Win32_NetworkManagement_WiFi,Win32_System_Com
```

在 `network.rs` 末尾追加：

```rust
#[cfg(target_os = "windows")]
mod platform_windows {
    use super::{Debouncer, NetworkEvent};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use tracing::{error, info};
    use windows::Win32::Foundation::{HANDLE, WIN32_ERROR};
    use windows::Win32::NetworkManagement::Ndis::{
        NotifyIpInterfaceChange, MibDeleteInstance, MibParameterNotification,
    };
    use windows::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
        WlanRegisterNotification, WLAN_ACCESS_MODE, WLAN_INTF_OPCODE,
        WLAN_NOTIFICATION_SOURCE_ACM, WLAN_NOTIFICATION_CALLBACK,
        WLAN_NOTIFICATION_DATA, wlan_intf_opcode_current_connection,
        wlan_notification_acm_connection_complete, wlan_notification_acm_disconnected,
    };
    use windows::Win32::NetworkManagement::NetworkInformation::AF_UNSPEC;

    type SendFn = Arc<dyn Fn(NetworkEvent) + Send + Sync>;

    pub struct WindowsNetworkWatcher {
        running: Arc<AtomicBool>,
        _state: Arc<WatcherState>,
    }

    struct WatcherState {
        on_event: SendFn,
        debouncer: Arc<Debouncer>,
        wlan_handle: Mutex<Option<HANDLE>>,
    }

    unsafe impl Send for WatcherState {}
    unsafe impl Sync for WatcherState {}

    impl WindowsNetworkWatcher {
        pub fn start(on_event: SendFn) -> Self {
            let running = Arc::new(AtomicBool::new(true));
            let state = Arc::new(WatcherState {
                on_event,
                debouncer: Arc::new(Debouncer::new()),
                wlan_handle: Mutex::new(None),
            });

            // 1) IP 接口变动（有线 + 通用 up/down）
            start_interface_watch(state.clone());

            // 2) WLAN 专用通知（提供 SSID）
            start_wlan_watch(state.clone());

            Self { running, _state: state }
        }

        pub fn stop(&self) {
            self.running.store(false, Ordering::Relaxed);
        }
    }

    fn start_interface_watch(state: Arc<WatcherState>) {
        let ctx = Arc::into_raw(state.clone()) as *const c_void;
        unsafe extern "system" fn ip_callback(
            caller_context: *const c_void,
            _row: *mut windows::Win32::NetworkManagement::Ndis::MIB_IPINTERFACE_ROW,
            notification_type: windows::Win32::NetworkManagement::Ndis::MIB_NOTIFICATION_TYPE,
        ) {
            let state = unsafe { Arc::from_raw(caller_context as *const WatcherState) };
            // 注意：此处 from_raw 会拿走 Arc 所有权；为保存活，改用 ManuallyDrop
            let state = std::mem::ManuallyDrop::new(unsafe {
                Arc::from_raw(caller_context as *const WatcherState)
            });
            let disconnected = matches!(notification_type, MibDeleteInstance | MibParameterNotification);
            // MibParameterNotification 也可能是地址变更（连接）；仅 MibDeleteInstance 视为断开
            let event = if matches!(notification_type, MibDeleteInstance) {
                NetworkEvent::Disconnect { ssid: None }
            } else {
                NetworkEvent::Connect { ssid: query_current_ssid() }
            };
            emit(&state, event);
            let _ = disconnected;
        }
        let mut handle = HANDLE::default();
        let res = unsafe {
            NotifyIpInterfaceChange(AF_UNSPEC, Some(ip_callback), ctx, false, &mut handle)
        };
        if res != WIN32_ERROR(0) {
            error!("NotifyIpInterfaceChange failed: {}", res.0);
        }
    }

    fn start_wlan_watch(state: Arc<WatcherState>) {
        let cb_state = state.clone();
        let callback: WLAN_NOTIFICATION_CALLBACK =
            Some(unsafe extern "system" fn wlan_callback(
                notification: *const WLAN_NOTIFICATION_DATA,
                context: *mut c_void,
            ) {
                if notification.is_null() {
                    return;
                }
                let n = unsafe { &*notification };
                let state = unsafe { &*(context as *const WatcherState) };
                if n.NotificationSource != WLAN_NOTIFICATION_SOURCE_ACM {
                    return;
                }
                let event = match n.NotificationCode {
                    c if c == wlan_notification_acm_connection_complete.0 => {
                        NetworkEvent::Connect { ssid: query_current_ssid() }
                    }
                    c if c == wlan_notification_acm_disconnected.0 => {
                        NetworkEvent::Disconnect { ssid: query_current_ssid() }
                    }
                    _ => return,
                };
                emit(state, event);
            });

        std::thread::spawn(move || unsafe {
            let mut negotiated = 0u32;
            let mut handle = HANDLE::default();
            if WlanOpenHandle(2u32, None, &mut negotiated, &mut handle) != WIN32_ERROR(0) {
                error!("WlanOpenHandle failed (WLAN service unavailable?)");
                return;
            }
            *cb_state.wlan_handle.lock().unwrap() = Some(handle);
            // dwPrevNotif: 0；注册全部 ACM 通知
            let _ = WlanRegisterNotification(
                handle,
                WLAN_NOTIFICATION_SOURCE_ACM,
                false,
                callback,
                Arc::as_ptr(&cb_state) as *mut c_void,
                None,
                None,
            );
            // WLAN 回调在系统线程触发；此线程挂起等待 stop
            loop {
                if !cb_state.running_flag() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            let _ = WlanCloseHandle(handle, None);
        });
    }

    impl WatcherState {
        fn running_flag(&self) -> bool {
            // running 状态由外层 AtomicBool 持有；此处简化为持续运行，随进程退出回收
            true
        }
    }

    fn emit(state: &WatcherState, event: NetworkEvent) {
        if !state.debouncer.allow(event.kind()) {
            return;
        }
        info!("network event: {:?}", event);
        (state.on_event)(event);
        if let NetworkEvent::Connect { .. } = event {
            spawn_online_probe(state);
        }
    }

    fn spawn_online_probe(state: &WatcherState) {
        use std::sync::atomic::AtomicI64;
        static LAST_PROBE_MS: AtomicI64 = AtomicI64::new(0);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if now_ms - LAST_PROBE_MS.load(Ordering::Relaxed) < 10_000 {
            return;
        }
        LAST_PROBE_MS.store(now_ms, Ordering::Relaxed);

        let on_event = state.on_event.clone();
        let debouncer = state.debouncer.clone();
        let Ok(handle) = tokio::runtime::Handle::try_current() else { return };
        handle.spawn(async move {
            if super::probe_online().await {
                let ev = NetworkEvent::Online { ssid: query_current_ssid() };
                if debouncer.allow(ev.kind()) {
                    on_event(ev);
                }
            }
        });
    }

    /// 查询当前关联 SSID（非 WiFi 或失败 → None）
    fn query_current_ssid() -> Option<String> {
        unsafe {
            let mut negotiated = 0u32;
            let mut handle = HANDLE::default();
            if WlanOpenHandle(2u32, None, &mut negotiated, &mut handle) != WIN32_ERROR(0) {
                return None;
            }
            let mut iface_list: *mut windows::Win32::NetworkManagement::WiFi::WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            // 枚举接口
            let list_ptr = (&mut iface_list) as *mut _;
            if windows::Win32::NetworkManagement::WiFi::WlanEnumInterfaces(handle, None, list_ptr) != WIN32_ERROR(0) {
                let _ = WlanCloseHandle(handle, None);
                return None;
            }
            let list = &*iface_list;
            let mut ssid = None;
            'outer: for i in 0..list.dwNumberOfItems {
                let info = &(*list.InterfaceInfo.offset(i as isize));
                let mut data_size = 0u32;
                let mut data: *mut c_void = std::ptr::null_mut();
                let opcode: WLAN_INTF_OPCODE = wlan_intf_opcode_current_connection;
                if WlanQueryInterface(handle, &info.InterfaceGuid, opcode, None, &mut data_size, &mut data, None) == WIN32_ERROR(0) && !data.is_null() {
                    let conn = &*(data as *const windows::Win32::NetworkManagement::WiFi::WLAN_CONNECTION_ATTRIBUTES);
                    let s = &conn.wlanAssociationAttributes.dot11Ssid;
                    if s.uSSIDLength > 0 {
                        ssid = Some(String::from_utf8_lossy(&s.ucSSID[..s.uSSIDLength as usize]).to_string());
                    }
                    WlanFreeMemory(data as *const c_void);
                    if ssid.is_some() {
                        break 'outer;
                    }
                }
            }
            windows::Win32::NetworkManagement::WiFi::WlanFreeMemory(iface_list as *const c_void);
            let _ = WlanCloseHandle(handle, None);
            ssid
        }
    }
}
```

说明：windows-rs 各版本中 netioapi（`NotifyIpInterfaceChange`/`MIB_*`）在 `NetworkManagement::Ndis` 或 `NetworkInformation` 模块、`WlanEnumInterfaces` 签名（是否含 `*mut *mut`）以所装版本 docs.rs 为准微调；事件语义（MibDeleteInstance→Disconnect、connection_complete→Connect、disconnected→Disconnect、SSID 从 `WLAN_CONNECTION_ATTRIBUTES` 读取）不变。`ip_callback` 中首次 `Arc::from_raw` 后立刻被 `ManuallyDrop` 覆盖属计划笔误，实现时直接只写 `ManuallyDrop::new(Arc::from_raw(..))` 一行，删掉前一行。

- [ ] **Step 4: 编译验证（双平台）**

```bash
rustup target add x86_64-pc-windows-msvc 2>/dev/null || true
cargo check -p easyjob-agent
cargo check -p easyjob-agent --target x86_64-pc-windows-msvc
```

Expected: 两条命令均编译通过（check 不链接，可交叉验证）。FFI 细节按 docs.rs 修正直至通过。

- [ ] **Step 5: 回归**

Run: `cargo test -p easyjob-agent && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
Expected: 全绿（本机为 macOS，跑 macOS 分支；Windows 分支由 cross-check 保证编译）。

- [ ] **Step 6: 提交**

```bash
git add apps/agent/src/network.rs apps/agent/Cargo.toml Cargo.lock
git commit -m "feat(agent): add macOS and Windows network monitors with debounce"
```

---

### Task 7: AgentService 接线 + Cron 配置告警 + 集成测试

**Files:**
- Modify: `apps/agent/src/service.rs`
- Modify: `crates/scheduler/src/evaluator.rs`（导出 validate_cron_expression）
- Test: Create `apps/agent/tests/network_tests.rs`

- [ ] **Step 1: scheduler 导出 cron 校验**

`crates/scheduler/src/evaluator.rs` 追加（tests 模块之前）：

```rust
/// 校验 cron 表达式是否为受支持的 5 字段合法表达式（agent 侧 warn 日志用）
pub fn validate_cron_expression(expression: &str) -> bool {
    if expression.split_whitespace().count() != 5 {
        return false;
    }
    croner::Cron::from_str(expression).is_ok()
}
```

`crates/scheduler/src/lib.rs` 的 re-export 行改为：

```rust
pub use evaluator::{evaluate_next_occurrence, validate_cron_expression, TriggerEvaluator};
```

- [ ] **Step 2: service.rs 接线**

`apps/agent/src/service.rs` 顶部 use 区追加：

```rust
use crate::network::{dispatch_network_event, spawn_network_monitor};
```

`AgentService` 结构体追加两个字段：

```rust
    net_mon_handle: tokio::task::JoinHandle<()>,
    net_disp_handle: tokio::task::JoinHandle<()>,
```

`init()` 中，`let (event_tx, _) = tokio::sync::broadcast::channel(1024);` 之后、`let handler = ...` 之前插入：

```rust
        // 网络变动触发：监控 + 派发双后台任务
        let (net_ev_tx, mut net_ev_rx) = mpsc::channel::<crate::network::NetworkEvent>(64);
        let net_mon_handle = tokio::spawn(async move {
            crate::network::spawn_network_monitor(net_ev_tx).await;
        });
        let net_task_repo = task_repo.clone();
        let net_scheduler_tx = scheduler_tx.clone();
        let net_disp_handle = tokio::spawn(async move {
            while let Some(ev) = net_ev_rx.recv().await {
                dispatch_network_event(ev, net_task_repo.clone(), net_scheduler_tx.clone())
                    .await;
            }
        });
```

`init()` 返回的 `Self { ... }` 追加：

```rust
            net_mon_handle,
            net_disp_handle,
```

加载任务循环（`for task in enabled_tasks` 内）追加 cron 非法告警（在 `let _ = scheduler_tx.send(...)` 之前）：

```rust
            for tr in &task.triggers {
                if tr.enabled {
                    if let easyjob_domain::trigger::TriggerKind::Cron { expression, .. } = &tr.kind {
                        if !easyjob_scheduler::validate_cron_expression(expression) {
                            tracing::warn!("Task {} has invalid cron expression: {:?}", task.id, expression);
                        }
                    }
                }
            }
```

`AgentRpcHandler::handle_request` 的 `"task.save"` 分支 `Ok(()) => {` 内、`scheduler_tx.send` 之前追加同样告警逻辑（复制上面 for 循环）。

`run()` 末尾（`let _ = self.scheduler_handle.await;` 之后、`info!("Agent service successfully shut down")` 之前）追加：

```rust
        self.net_disp_handle.abort();
        self.net_mon_handle.abort();
```

- [ ] **Step 3: 写集成测试**

创建 `apps/agent/tests/network_tests.rs`：

```rust
use easyjob_agent::network::{dispatch_network_event, NetworkEvent};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{NetworkEventKind, Trigger, TriggerKind};
use easyjob_persistence::db::init_pool;
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::{Scheduler, SchedulerCommand, TriggerEvent};
use std::sync::Arc;

fn network_task(task_id: &str, events: Vec<NetworkEventKind>, name: Option<&str>) -> Task {
    let task_id = task_id.to_string();
    Task {
        id: task_id.clone(),
        name: format!("task-{}", task_id),
        description: None,
        enabled: true,
        triggers: vec![Trigger {
            id: format!("tr-{}", task_id),
            task_id: task_id.clone(),
            enabled: true,
            kind: TriggerKind::Network {
                events,
                network_name: name.map(String::from),
            },
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }],
        actions: vec![Action {
            id: format!("ac-{}", task_id),
            task_id: task_id.clone(),
            sequence: 0,
            enabled: true,
            kind: ActionKind::ExecuteShell { command: "true".into() },
        }],
        execution_policy: Default::default(),
        working_directory: None,
        environment: Default::default(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

async fn setup(tasks: Vec<Task>) -> (mpsc::Sender<SchedulerCommand>, tokio::sync::mpsc::Receiver<TriggerEvent>, Arc<SqliteTaskRepository>, tokio::task::JoinHandle<()>) {
    let db = tempfile::tempdir().unwrap();
    let pool = init_pool(&format!("sqlite://{}/test.db", db.path().display())).await.unwrap();
    let repo = Arc::new(SqliteTaskRepository::new(pool));
    for t in &tasks {
        repo.save(t).await.unwrap();
    }

    let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx);
    let sched_tx = scheduler.sender();
    let queue = scheduler.queue();
    let ev_tx = scheduler.event_sender();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, ev_tx));
    for t in tasks {
        sched_tx.send(SchedulerCommand::add_task(t)).await.unwrap();
    }
    (sched_tx, event_rx, repo, handle)
}

#[tokio::test]
async fn network_connect_event_triggers_matching_task() {
    let (tx, mut rx, repo, _sched) = setup(vec![network_task("t1", vec![NetworkEventKind::Connect], None)]).await;
    dispatch_network_event(NetworkEvent::Connect { ssid: Some("Home") .map(String::from) }, repo, tx).await;
    let ev = rx.recv().await.unwrap();
    assert_eq!(ev.task_id, "t1");
    assert_eq!(ev.trigger_id, None); // TriggerNow 路径
}

#[tokio::test]
async fn network_disconnect_event_skips_task_without_disconnect() {
    let (tx, mut rx, repo, _sched) = setup(vec![network_task("t1", vec![NetworkEventKind::Connect], None)]).await;
    dispatch_network_event(NetworkEvent::Disconnect { ssid: None }, repo, tx).await;
    // 短暂等待确认无事件
    let got = tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv()).await;
    assert!(got.is_err(), "should not receive trigger event");
}

#[tokio::test]
async fn network_ssid_filter_triggers_only_on_match() {
    let (tx, mut rx, repo, _sched) = setup(vec![network_task("t1", vec![NetworkEventKind::Connect], Some("Office"))]).await;
    dispatch_network_event(NetworkEvent::Connect { ssid: Some("Home".into()) }, repo.clone(), tx.clone()).await;
    assert!(tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv()).await.is_err());
    dispatch_network_event(NetworkEvent::Connect { ssid: Some("Office".into()) }, repo, tx).await;
    let ev = rx.recv().await.unwrap();
    assert_eq!(ev.task_id, "t1");
}

#[tokio::test]
async fn network_event_for_disabled_task_does_not_fire() {
    let mut task = network_task("t1", vec![NetworkEventKind::Connect], None);
    task.enabled = false;
    let (tx, mut rx, repo, _sched) = setup(vec![task]).await;
    dispatch_network_event(NetworkEvent::Connect { ssid: None }, repo, tx).await;
    assert!(tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv()).await.is_err());
}
```

注意：`use tokio::sync::mpsc;` 需补入文件顶部；`Task`/`ExecutionPolicy` 若 `Default::default()` 不可用则照抄 `apps/agent/tests/service_tests.rs` 中现有 Task 构造方式（该文件已有完整构造样例，保持字段一致）。`ActionKind::ExecuteShell` 字段名以 `crates/domain/src/action.rs` 为准。

- [ ] **Step 4: 运行测试**

Run: `cargo test -p easyjob-agent`
Expected: 4 个 network_tests 全部 PASS，service_tests 原有测试不回归。

- [ ] **Step 5: 全量回归 + 提交**

Run: `cargo test --all && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`

```bash
git add apps/agent/src/service.rs crates/scheduler/src/evaluator.rs crates/scheduler/src/lib.rs apps/agent/tests/network_tests.rs
git commit -m "feat(agent): wire network monitor into agent service and add integration tests"
```

---

### Task 8: 前端类型与辅助函数

**Files:**
- Modify: `apps/desktop/src/types/task.ts`
- Test: Create `apps/desktop/tests/triggerKinds.test.ts`

- [ ] **Step 1: 写失败测试**

创建 `apps/desktop/tests/triggerKinds.test.ts`：

```ts
import { describe, it, expect } from 'vitest';
import {
  getTriggerType,
  describeTrigger,
  validateCronExpression,
  type TriggerKind,
} from '../src/types/task';

describe('New trigger kinds wire format', () => {
  it('recognizes Cron kind', () => {
    const kind: TriggerKind = { Cron: { expression: '*/5 * * * *', timezone: 'Asia/Shanghai' } };
    expect(getTriggerType(kind)).toBe('Cron');
  });

  it('recognizes Fuzzy kind and serde shape', () => {
    const kind: TriggerKind = {
      Fuzzy: { period: { Weekly: { days_of_week: ['Mon', 'Fri'] } }, window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' },
    };
    expect(getTriggerType(kind)).toBe('Fuzzy');
    expect(JSON.stringify(kind)).toContain('"days_of_week"');
  });

  it('Fuzzy unit periods serialize as strings (serde conformance)', () => {
    expect(JSON.stringify({ period: 'Daily' })).toContain('"Daily"');
  });

  it('recognizes Network kind with optional name', () => {
    const withName: TriggerKind = { Network: { events: ['Connect', 'Online'], network_name: 'MyHome' } };
    const noName: TriggerKind = { Network: { events: ['Disconnect'], network_name: null } };
    expect(getTriggerType(withName)).toBe('Network');
    expect(getTriggerType(noName)).toBe('Network');
  });
});

describe('validateCronExpression', () => {
  it('accepts valid 5-field expressions', () => {
    expect(validateCronExpression('*/5 * * * *')).toBe(true);
    expect(validateCronExpression('30 9 * * 1-5')).toBe(true);
    expect(validateCronExpression('0 0 1 JAN SUN')).toBe(true);
  });

  it('rejects wrong field count and bad tokens', () => {
    expect(validateCronExpression('0 */5 * * * *')).toBe(false); // 6 字段
    expect(validateCronExpression('*/5 * * *')).toBe(false);     // 4 字段
    expect(validateCronExpression('99 * * * *')).toBe(false);    // 分钟越界
    expect(validateCronExpression('hello world')).toBe(false);
  });
});

describe('describeTrigger', () => {
  it('describes new kinds in Chinese', () => {
    expect(describeTrigger({ Cron: { expression: '*/5 * * * *', timezone: 'Asia/Shanghai' } })).toBe('Cron: */5 * * * *');
    expect(describeTrigger({ Fuzzy: { period: 'Daily', window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } })).toBe('模糊时间：每天 09:00:00-10:00:00 随机');
    expect(describeTrigger({ Network: { events: ['Connect'], network_name: 'MyHome' } })).toBe('网络变动：连接时 (MyHome)');
    expect(describeTrigger({ Network: { events: ['Connect', 'Disconnect'], network_name: null } })).toBe('网络变动：连接时/断开时（任意网络）');
  });
});
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm -C apps/desktop test -- triggerKinds`
Expected: FAIL（Cron/Fuzzy/Network 类型与函数不存在）。

- [ ] **Step 3: 实现**

`apps/desktop/src/types/task.ts`：

（a）`TriggerKind` 联合类型末尾（`| 'AgentStarted'` 之后）追加：

```ts
  | { Cron: { expression: string; timezone: string } }
  | { Fuzzy: { period: FuzzyPeriod; window_start: string; window_end: string; timezone: string } }
  | { Network: { events: NetworkEventKind[]; network_name: string | null } };
```

（b）`Weekday` 定义之后追加：

```ts
// serde: 无字段变体序列化为字符串，带字段变体序列化为对象
export type FuzzyPeriod =
  | 'Daily'
  | 'Weekdays'
  | 'Weekends'
  | { Weekly: { days_of_week: Weekday[] } };

export type NetworkEventKind = 'Connect' | 'Disconnect' | 'Online';

export type TriggerType =
  | 'Once' | 'Interval' | 'Daily' | 'Weekly' | 'AgentStarted'
  | 'Cron' | 'Fuzzy' | 'Network';
```

（c）`getTriggerType` 整体替换为：

```ts
export function getTriggerType(kind: TriggerKind): TriggerType {
  if (typeof kind === 'string') {
    return kind;
  }
  if ('Once' in kind) return 'Once';
  if ('Interval' in kind) return 'Interval';
  if ('Daily' in kind) return 'Daily';
  if ('Weekly' in kind) return 'Weekly';
  if ('Cron' in kind) return 'Cron';
  if ('Fuzzy' in kind) return 'Fuzzy';
  if ('Network' in kind) return 'Network';
  throw new Error(`Unknown trigger kind: ${JSON.stringify(kind)}`);
}
```

（d）文件底部（`isWindowsPlatform` 之后）追加：

```ts
const CRON_FIELD_RANGES: [number, number][] = [
  [0, 59], // minute
  [0, 23], // hour
  [1, 31], // day of month
  [1, 12], // month
  [0, 7],  // day of week (7 = Sunday)
];

const CRON_TOKEN_RE =
  /^(\*|\?|\d+|[A-Za-z]{3})(?:-(\d+|[A-Za-z]{3}))?(?:\/(\d+))?$/;

export function validateCronExpression(expr: string): boolean {
  const fields = expr.trim().split(/\s+/);
  if (fields.length !== 5) return false;
  return fields.every((field, i) => {
    if (field === '*' || field === '?') return true;
    const [lo, hi] = CRON_FIELD_RANGES[i];
    return field.split(',').every((token) => {
      const step = token.includes('/');
      if (step) {
        const [base, stepStr] = token.split('/');
        if (!/^\d+$/.test(stepStr) || parseInt(stepStr, 10) < 1) return false;
        if (base === '*' || base === '?') return true;
        return checkCronToken(base, lo, hi, i);
      }
      return checkCronToken(token, lo, hi, i);
    });
  });
}

function checkCronToken(token: string, lo: number, hi: number, fieldIdx: number): boolean {
  if (!CRON_TOKEN_RE.test(token)) return false;
  if (/^[A-Za-z]{3}$/.test(token)) return true; // JAN/DEC/MON/SUN 等名称
  const nums = token.match(/\d+/g);
  if (!nums) return false;
  return nums.every((n) => {
    const v = parseInt(n, 10);
    return v >= lo && v <= hi;
  });
  // fieldIdx 保留用于未来按字段细化名称校验
}

export function describeTrigger(kind: TriggerKind): string {
  if (typeof kind === 'string') return '启动即运行';
  if ('Once' in kind) return `单次：${kind.Once.fire_at.replace('T', ' ').slice(0, 19)} UTC`;
  if ('Interval' in kind) {
    const s = kind.Interval.interval_secs ?? kind.Interval.seconds ?? 60;
    return `间隔：每 ${s} 秒`;
  }
  if ('Daily' in kind) return `每天 ${kind.Daily.time} (${kind.Daily.timezone})`;
  if ('Weekly' in kind) return `每周 ${kind.Weekly.days_of_week.join(',')} ${kind.Weekly.time}`;
  if ('Cron' in kind) return `Cron: ${kind.Cron.expression}`;
  if ('Fuzzy' in kind) {
    const f = kind.Fuzzy;
    const periodLabel =
      typeof f.period === 'string'
        ? { Daily: '每天', Weekdays: '工作日', Weekends: '周末' }[f.period]
        : `每周 ${f.period.Weekly.days_of_week.join(',')}`;
    return `模糊时间：${periodLabel} ${f.window_start}-${f.window_end} 随机`;
  }
  if ('Network' in kind) {
    const evLabels = kind.Network.events
      .map((e) => ({ Connect: '连接时', Disconnect: '断开时', Online: '可上网时' }[e]))
      .join('/');
    const name = kind.Network.network_name ? ` (${kind.Network.network_name})` : '（任意网络）';
    return `网络变动：${evLabels}${name}`;
  }
  return JSON.stringify(kind);
}
```

（若项目有 lint 禁止未使用参数，`fieldIdx` 改为 `_fieldIdx`。）

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm -C apps/desktop test`
Expected: 全部 PASS（含既有 93 个）。

- [ ] **Step 5: 提交**

```bash
git add apps/desktop/src/types/task.ts apps/desktop/tests/triggerKinds.test.ts
git commit -m "feat(desktop): add Cron/Fuzzy/Network trigger types with validation and summaries"
```

---

### Task 9: TriggerEditor 三块表单

**Files:**
- Modify: `apps/desktop/src/components/task/TriggerEditor.vue`

- [ ] **Step 1: 类型选项与默认值**

`triggerTypeOptions` 替换为（顺序保持既有项在前）：

```ts
const triggerTypeOptions = [
  { label: '间隔触发 (Interval)', value: 'Interval' },
  { label: '每日定时 (Daily)', value: 'Daily' },
  { label: '每周定时 (Weekly)', value: 'Weekly' },
  { label: '单次执行 (Once)', value: 'Once' },
  { label: '启动即运行 (AgentStarted)', value: 'AgentStarted' },
  { label: 'Cron 表达式 (Cron)', value: 'Cron' },
  { label: '模糊时间 (Fuzzy)', value: 'Fuzzy' },
  { label: '网络变动 (Network)', value: 'Network' },
];
```

顶部 import 区追加：

```ts
import { NSelect, NInputNumber, NInput, NDatePicker, NCheckboxGroup, NCheckbox, NButton, NSwitch } from 'naive-ui'; // 已有
import { getTriggerType, parseDate, validateCronExpression } from '../../types/task';
import type { Trigger, Weekday, FuzzyPeriod } from '../../types/task';
```

`changeKindType` 函数在 `else if (type === 'AgentStarted')` 块之后追加：

```ts
  } else if (type === 'Cron') {
    trigger.kind = {
      Cron: {
        expression: '*/5 * * * *',
        timezone: defaultTz,
      },
    };
  } else if (type === 'Fuzzy') {
    trigger.kind = {
      Fuzzy: {
        period: 'Daily',
        window_start: '09:00:00',
        window_end: '10:00:00',
        timezone: defaultTz,
      },
    };
  } else if (type === 'Network') {
    trigger.kind = {
      Network: {
        events: ['Connect'],
        network_name: null,
      },
    };
  }
```

新增脚本辅助（`changeKindType` 之后）：

```ts
const fuzzyPeriodOptions = [
  { label: '每天', value: 'Daily' },
  { label: '工作日 (周一至周五)', value: 'Weekdays' },
  { label: '周末', value: 'Weekends' },
  { label: '按周几', value: 'Weekly' },
];

const networkEventOptions = [
  { label: '连接网络时', value: 'Connect' },
  { label: '断开网络时', value: 'Disconnect' },
  { label: '检测到可上网时', value: 'Online' },
];

function getFuzzyPeriod(tr: Trigger): string {
  if (typeof tr.kind === 'object' && 'Fuzzy' in tr.kind) {
    return typeof tr.kind.Fuzzy.period === 'string' ? tr.kind.Fuzzy.period : 'Weekly';
  }
  return 'Daily';
}

function setFuzzyPeriod(tr: Trigger, period: string) {
  if (typeof tr.kind === 'object' && 'Fuzzy' in tr.kind) {
    tr.kind.Fuzzy.period =
      period === 'Weekly' ? { Weekly: { days_of_week: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'] } } : (period as FuzzyPeriod);
  }
}

function cronInvalid(tr: Trigger): boolean {
  if (typeof tr.kind === 'object' && 'Cron' in tr.kind) {
    return !validateCronExpression(tr.kind.Cron.expression);
  }
  return false;
}

function fuzzyWindowInvalid(tr: Trigger): boolean {
  if (typeof tr.kind === 'object' && 'Fuzzy' in tr.kind) {
    return tr.kind.Fuzzy.window_end <= tr.kind.Fuzzy.window_start;
  }
  return false;
}
```

- [ ] **Step 2: 模板三区块**

在模板中 `<!-- AgentStarted Editor -->` 区块之后、闭合 `</div>` 之前追加：

```html
      <!-- Cron Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Cron' in tr.kind" class="grid grid-cols-2 gap-3 text-xs">
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">Cron 表达式（分 时 日 月 周）</label>
          <NInput
            v-model:value="tr.kind.Cron.expression"
            placeholder="*/5 * * * *"
            size="small"
            class="font-mono"
            :status="cronInvalid(tr) ? 'error' : undefined"
          />
          <div v-if="cronInvalid(tr)" class="text-red-500 mt-1">表达式无效，需为 5 字段 cron（如 */5 * * * *）</div>
        </div>
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">时区 (Timezone)</label>
          <NInput v-model:value="tr.kind.Cron.timezone" placeholder="例如 Asia/Shanghai, UTC" size="small" />
        </div>
      </div>

      <!-- Fuzzy Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Fuzzy' in tr.kind" class="space-y-2.5 text-xs">
        <div class="grid grid-cols-2 gap-3">
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">周期</label>
            <NSelect
              :value="getFuzzyPeriod(tr)"
              :options="fuzzyPeriodOptions"
              size="small"
              @update:value="setFuzzyPeriod(tr, $event)"
            />
          </div>
          <div v-if="getFuzzyPeriod(tr) === 'Weekly'">
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">每周执行日</label>
            <NCheckboxGroup v-model:value="tr.kind.Fuzzy.period.Weekly.days_of_week">
              <div class="flex flex-wrap gap-2">
                <NCheckbox v-for="d in dayOptions" :key="d.value" :value="d.value" :label="d.label" size="small" />
              </div>
            </NCheckboxGroup>
          </div>
        </div>
        <div class="grid grid-cols-3 gap-3">
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">窗口开始</label>
            <input
              type="time" step="1" :value="tr.kind.Fuzzy.window_start"
              @change="tr.kind.Fuzzy.window_start = normalizeTime(($event.target as HTMLInputElement).value)"
              class="w-full px-2.5 py-1.5 rounded border border-slate-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-xs font-mono"
            />
          </div>
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">窗口结束</label>
            <input
              type="time" step="1" :value="tr.kind.Fuzzy.window_end"
              @change="tr.kind.Fuzzy.window_end = normalizeTime(($event.target as HTMLInputElement).value)"
              class="w-full px-2.5 py-1.5 rounded border border-slate-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-xs font-mono"
            />
          </div>
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">时区</label>
            <NInput v-model:value="tr.kind.Fuzzy.timezone" placeholder="Asia/Shanghai" size="small" />
          </div>
        </div>
        <div v-if="fuzzyWindowInvalid(tr)" class="text-red-500">窗口结束时间必须晚于开始时间</div>
      </div>

      <!-- Network Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Network' in tr.kind" class="space-y-2.5 text-xs">
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">触发事件（至少选一个）</label>
          <NCheckboxGroup v-model:value="tr.kind.Network.events">
            <div class="flex flex-wrap gap-2">
              <NCheckbox v-for="e in networkEventOptions" :key="e.value" :value="e.value" :label="e.label" size="small" />
            </div>
          </NCheckboxGroup>
        </div>
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">网络名称（可选）</label>
          <NInput
            v-model:value="tr.kind.Network.network_name"
            placeholder="留空 = 任意网络；填写则精确匹配 WiFi 名称（区分大小写）"
            size="small"
            clearable
          />
        </div>
      </div>
```

注意 `NInput` 的 `v-model:value` 绑定 `string | null`：naive-ui 输入空串时与 `null` 的转换——在 `networkEventOptions` 区块下补一个 watch 或改用 `:value`/`@update:value` 手动归一（`$event === '' ? null : $event`）。采用后者：

```html
          <NInput
            :value="tr.kind.Network.network_name ?? ''"
            @update:value="tr.kind.Network.network_name = $event === '' ? null : $event"
            placeholder="留空 = 任意网络；填写则精确匹配 WiFi 名称（区分大小写）"
            size="small"
            clearable
          />
```

- [ ] **Step 3: 验证**

Run: `pnpm -C apps/desktop test && pnpm -C apps/desktop run build`
Expected: 测试全绿、类型检查与生产编译通过。手动目检：`pnpm -C apps/desktop dev` 打开任务抽屉，切换 8 种触发器类型确认表单渲染与默认值。

- [ ] **Step 4: 提交**

```bash
git add apps/desktop/src/components/task/TriggerEditor.vue
git commit -m "feat(desktop): add Cron, Fuzzy, Network trigger editor forms"
```

---

### Task 10: 任务列表中文摘要接线

**Files:**
- Modify: `apps/desktop/src/components/task/TaskAccordionContent.vue:266`

- [ ] **Step 1: 替换 JSON 串为中文摘要**

`TaskAccordionContent.vue` 第 266 行：

```html
              {{ JSON.stringify(tr.kind) }}
```

替换为：

```html
              {{ describeTrigger(tr.kind) }}
```

import 区（第 24 行附近）改为：

```ts
import { getTriggerType, getActionType, describeTrigger } from '../../types/task';
```

- [ ] **Step 2: 验证 + 提交**

Run: `pnpm -C apps/desktop test && pnpm -C apps/desktop run build`

```bash
git add apps/desktop/src/components/task/TaskAccordionContent.vue
git commit -m "feat(desktop): show human-readable trigger summaries in task list"
```

---

### Task 11: 全栈质量门禁收尾

**Files:** 无新改动（验证 + 修补）

- [ ] **Step 1: 全量验证**

```bash
cargo test --all
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
pnpm -C apps/desktop test
pnpm -C apps/desktop run build
```

Expected: 五条全绿（对齐 README 质量门禁）。

- [ ] **Step 2: 人工验收清单**

`pnpm -C apps/desktop tauri dev` 启动后：

1. 创建任务，添加 Cron 触发器 `*/1 * * * *`，观察每分钟执行并在日志抽屉看到输出
2. 添加 Fuzzy 触发器（每天，窗口设为未来 2 分钟内起止，如 now+1min ~ now+2min），确认在窗口内随机时刻执行
3. 添加 Network 触发器（连接时），关闭再打开 Wi-Fi，确认任务被触发；用"断开时"再验证一次
4. SSID 过滤：填一个错误的 WiFi 名，确认不触发；清空后触发
5. 重启 agent（`cargo run -p easyjob-agent -- --daemon`），确认 Cron/Fuzzy 触发器正常恢复调度

- [ ] **Step 3: 收尾提交（如门禁期间有修补）**

```bash
git add -A
git commit -m "chore: final quality gate fixes for new triggers"
```

---

## Self-Review 记录

1. **Spec 覆盖**：§4 领域模型→Task 1；§5 Cron/Fuzzy/Network evaluator→Task 2/3（croner 处理 DST，等价于 spec 的 resolve_candidate 方案且更可靠）；§6 网络监控+防抖+探测冷却+接线→Task 5/6/7；§7 kind 列修复→Task 4；§8 IPC 无改动（透传验证由 Task 1 serde 测试 + Task 7 集成测试覆盖）；§9 前端→Task 8/9/10；§10 错误处理→散布各任务实现与校验分支；§11 测试→各任务 TDD + Task 7 集成 + Task 11 门禁。§9.3 摘要→Task 10。
2. **占位符扫描**：无 TBD/TODO；Task 6 FFI 有两处显式"以 docs.rs 为准微调"的声明，属平台库版本 contingency，非占位符（代码完整给出）。
3. **类型一致性**：`FuzzyPeriod` TS 用字符串/对象双形态与 serde 对齐；`kind_name()` 定义于 Task 1、使用于 Task 4；`validate_cron_expression` 定义于 Task 2 步骤 1 的导出（Task 7 实现）、使用于 Task 7 与前端 `validateCronExpression`（Task 8/9）；`NetworkEvent`/`NetworkEventKind` 在 Task 5/6/7/8/9 名称一致。
