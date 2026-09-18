# 新增三类触发器设计：Cron 表达式 / 模糊时间 / 网络变动

日期：2026-09-18
状态：已确认（方案 A：事件驱动网络监控器作为独立后台任务）
平台范围：**全栈仅考虑 macOS + Windows**。领域模型、调度器、Agent、持久化、
前端所有层均不编写任何其他平台（Linux 等）的适配分支或条件隐藏逻辑。

## 1. 背景与目标

easyJob 现有 5 类触发器（Once / Interval / Daily / Weekly / AgentStarted），定义于
`crates/domain/src/trigger.rs` 的 `TriggerKind` 枚举。本设计新增 3 类触发器：

1. **Cron 表达式触发（Cron）**：标准 5 字段 cron 表达式调度（分 时 日 月 周）。
2. **模糊时间触发（Fuzzy）**：周期性时间窗口内随机选点触发（如每天 09:00–10:00
   随机一次），用于避免固定时间被预测。
3. **网络变动触发（Network）**：网络连接 / 断开 / 真正可上网时触发，支持按
   WiFi 名称（SSID）过滤。

## 2. 需求决策记录

| 决策点 | 结论 |
|---|---|
| Fuzzy 语义 | 周期窗口内随机（非单次区间随机） |
| Network 事件 | Connect、Disconnect、Online 三种全部支持，可多选 |
| Cron 格式 | 标准 5 字段（分 时 日 月 周），不含秒、不含 @别名 |
| 平台范围 | 全栈仅 macOS + Windows；不编写任何其他平台（Linux 等）的适配分支 |
| 架构方案 | 方案 A：NetworkMonitor 独立后台任务 + TriggerNow 派发，零侵入调度循环 |

## 3. 总体架构

```text
┌───────────────────────────────────────────────────────────────────┐
│ AgentService (apps/agent/src/service.rs)                          │
│                                                                   │
│  ┌────────────────────────┐      SchedulerCommand::AddTask        │
│  │ Scheduler (时间轮)      │ ◄─────────────────────────────┐       │
│  │  - Cron   (evaluator)  │                               │       │
│  │  - Fuzzy  (evaluator)  │  TriggerEvent                 │       │
│  │  - Once/Daily/...      │ ─────────────┐                │       │
│  └────────────────────────┘              ▼                │       │
│  ┌────────────────────────┐   ┌──────────────────────┐    │       │
│  │ NetworkMonitor         │──►│ NetworkEventDispatch │──►─┤       │
│  │ (后台 tokio task)      │   │ er (后台 task)       │    │       │
│  │  macOS: nw_path_monitor│   │  - 查 task_repo      │    │       │
│  │  Windows: NotifyAddr   │   │  - 匹配 Network 触发 │    │       │
│  │    Change + Wlan API   │   │  - 发 TriggerNow     │    │       │
│  └────────────────────────┘   └──────────────────────┘    │       │
│                                                           ▼       │
│  SchedulerCommand::TriggerNow(task_id) ─► Scheduler ─► TriggerEvent│
└───────────────────────────────────────────────────────────────────┘
```

要点：

- **Cron 与 Fuzzy 是时间触发器**：仅扩展 `evaluate_next_occurrence`
  （`crates/scheduler/src/evaluator.rs`），scheduler 主循环、IPC、持久化零修改。
- **Network 是事件触发器**：与 `AgentStarted` 同类，`evaluate_next_occurrence`
  返回 `None`；由 agent 内后台任务感知网络事件，经
  `SchedulerCommand::TriggerNow` 派发，复用现有执行管线（并发策略、信号量、
  日志、通知全部自动生效）。
- **NetworkMonitor 与 NetworkEventDispatcher 为两个独立 tokio task**：
  monitor 只做平台事件采集与防抖，dispatcher 只做任务匹配与派发，职责分离、
  可独立测试。

## 4. 领域模型（crates/domain/src/trigger.rs）

`TriggerKind` 枚举末尾追加 3 个变体：

```rust
pub enum TriggerKind {
    // ... 现有 5 个变体不变 ...

    /// 标准 5 字段 cron 表达式（分 时 日 月 周），按指定时区解释
    Cron {
        expression: String,   // e.g. "*/5 * * * *"
        timezone: String,     // IANA tz name，默认 Asia/Shanghai
    },

    /// 周期窗口内随机触发：每个匹配周期在 [window_start, window_end) 内随机选点
    Fuzzy {
        period: FuzzyPeriod,
        window_start: NaiveTime,
        window_end: NaiveTime,    // 必须 > window_start
        timezone: String,
    },

    /// 网络变动触发（仅 macOS / Windows）
    Network {
        events: Vec<NetworkEventKind>,          // 至少一个
        network_name: Option<String>,           // SSID 过滤；None = 任意网络
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FuzzyPeriod {
    Daily,
    Weekdays,                                   // Mon–Fri
    Weekends,                                   // Sat–Sun
    Weekly { days_of_week: Vec<Weekday> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkEventKind {
    Connect,     // 接口 up 且获得 IP（连上 WiFi / 插上网线 / 切换网络）
    Disconnect,  // 接口 down（断开 WiFi / 拔网线）
    Online,      // 主动探测确认可访问互联网（区分"连上 WiFi 但无外网"）
}
```

命名约定：

| 触发器 | Rust 变体 | UI 显示名 |
|---|---|---|
| Cron 表达式 | `Cron` | Cron 表达式 (Cron) |
| 模糊时间 | `Fuzzy` | 模糊时间 (Fuzzy) |
| 网络变动 | `Network` | 网络变动 (Network) |

## 5. 调度器扩展（crates/scheduler/src/evaluator.rs）

### 5.1 新依赖

`Cargo.toml` workspace 级：

```toml
croner = "2.0"     # 纯 Rust 5 字段 cron 解析
rand = "0.8"       # Fuzzy 随机选点
```

`crates/scheduler/Cargo.toml` 引入以上两个依赖。

### 5.2 Cron 分支

```rust
TriggerKind::Cron { expression, timezone } => {
    let cron = match croner::Cron::new(expression) {
        Ok(c) => c,
        Err(_) => return None,   // 非法表达式静默跳过；前端实时校验
    };
    let tz: Tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
    let local_after = after.with_timezone(&tz);
    match cron.after(&local_after).next() {
        Some(ndt) => {
            // 复用 resolve_candidate 处理 DST 间隙，行为与 Daily/Weekly 一致
            tz.from_local_datetime(&ndt)
                .single()
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|| resolve_candidate(&tz, ndt.date_naive(), ndt.time(), after))
                .filter(|u| *u > after)
        }
        None => None,
    }
}
```

- 表达式非法返回 `None`，不阻断同任务其他触发器；agent 侧以 `warn!` 日志记录一次
  （在 `AddTask` 注册路径），前端表单实时校验兜底。
- 仅支持 5 字段；`croner` 的 `@` 别名与 6 字段秒级模式不启用。

### 5.3 Fuzzy 分支

```rust
TriggerKind::Fuzzy { period, window_start, window_end, timezone } => {
    if window_end <= window_start {
        return None;
    }
    let tz: Tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
    let local_after = after.with_timezone(&tz);
    let mut candidate_date = local_after.date_naive();

    for _ in 0..8 {   // 8 天搜索上限，覆盖 Weekly 最坏情况
        if period.matches_weekday(candidate_date.weekday()) {
            // 该日窗口内随机选点
            let picked = pick_random_time_in_window(window_start, window_end);
            if let Some(utc) = resolve_candidate(&tz, candidate_date, picked, after) {
                return Some(utc);
            }
            // 今天：首次随机点已过期（窗口接近尾声），在剩余窗口内重摇
            if candidate_date == local_after.date_naive() {
                let after_time = local_after.time();
                if after_time < *window_end {
                    let late_pick = pick_random_time_in_window(&after_time, window_end);
                    if let Some(utc) = resolve_candidate(&tz, candidate_date, late_pick, after) {
                        return Some(utc);
                    }
                }
            }
        }
        candidate_date = candidate_date.succ_opt()?;
    }
    None
}
```

```rust
fn pick_random_time_in_window(start: &NaiveTime, end: &NaiveTime) -> NaiveTime {
    use rand::Rng;
    let span = (*end - *start).num_seconds().max(1) as u64;
    let offset = rand::thread_rng().gen_range(0..span);
    *start + chrono::Duration::seconds(offset as i64)
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
```

语义细则：

- 每个周期窗口内**只触发一次**；fire 后 scheduler 重新调用
  `evaluate_next_occurrence(now)`，得到下一匹配周期窗口内的新随机点。
- 任务注册（`AddTask`）当天若为匹配日且窗口未过完：以
  `max(now_local, window_start)` 为下界随机，保证今天仍可触发。
- 每次调用独立随机：clock-jump 队列重建、agent 重启都会重新摇号——这是
  fuzzy 语义的合理行为，不做"已选时间点"持久化。
- `window_end <= window_start` 视为非法配置，返回 `None`（前端校验兜底）。

### 5.4 Network 分支

```rust
TriggerKind::Network { .. } => None,  // 事件驱动，与 AgentStarted 同路径
```

## 6. 网络监控（apps/agent/src/network.rs，新模块）

### 6.1 事件类型与接口

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEvent {
    Connect    { ssid: Option<String> },
    Disconnect { ssid: Option<String> },
    Online     { ssid: Option<String> },
}

pub struct NetworkMonitor;

impl NetworkMonitor {
    /// 启动平台后台监控；事件经 event_tx 发出。返回 JoinHandle。
    pub fn spawn(event_tx: mpsc::Sender<NetworkEvent>) -> tokio::task::JoinHandle<()>;
}

/// 纯函数：事件类型匹配（可单测）
pub fn event_matches(ev: &NetworkEvent, events: &[NetworkEventKind]) -> bool;

/// 纯函数：SSID 过滤（可单测）。network_name 为 None 时匹配任意网络；
///  Some(name) 时要求 ev.ssid == Some(name)，区分大小写，精确匹配。
pub fn name_matches(ev: &NetworkEvent, network_name: &Option<String>) -> bool;

/// 派发：查启用任务 → 匹配 Network 触发器 → TriggerNow
pub async fn dispatch_network_event(
    ev: NetworkEvent,
    task_repo: Arc<SqliteTaskRepository>,
    scheduler_tx: mpsc::Sender<SchedulerCommand>,
);
```

`dispatch_network_event` 匹配规则：任务 `enabled`、触发器 `enabled`、事件类型命中
`events`、SSID 命中 `network_name`；同一任务多个 Network 触发器任一命中即
`TriggerNow` 一次（`break` 防重复派发）。并发策略（SkipIfRunning 等）由现有
执行管线兜底。

### 6.2 平台实现

**macOS**（`#[cfg(target_os = "macos")]`）：

- 依赖：`objc2-network`、`objc2-foundation`（workspace target 依赖）。
- Connect/Disconnect：`nw_path_monitor_create` + `set_update_handler` 订阅
  path 状态变化；`nw_path_status_satisfied` → Connect，`unsatisfied` → Disconnect。
- SSID：CoreWLAN `CWWiFiClient` 获取当前关联 SSID；有线网络返回 `None`。
- Online：path 变为 Satisfied 后，spawn 一次性探测——TCP 连
  `1.1.1.1:80`（2s 超时），失败则 fallback DNS 解析 `example.com`；成功发
  `Online`。探测带 10s 冷却，避免断连风暴时反复探测。

**Windows**（`#[cfg(target_os = "windows")]`）：

- 依赖：`windows` crate（features：
  `Win32_NetworkManagement_NetworkInformation`、
  `Win32_NetworkManagement_WiFi`、`Win32_Foundation`、
  `Win32_Networking_WinSock`）。
- Connect/Disconnect：`NotifyIpInterfaceChange`（`AF_UNSPEC`，异步回调）感知
  接口 up/down 与地址变更；回调内区分 `NotificationType`（接口删除/禁用 →
  Disconnect，新增/启用且有地址 → Connect）。
- SSID 与 WiFi 事件：`WlanOpenHandle` + `WlanRegisterNotification` 订阅
  `wlan_notification_acm_connection_complete` /
  `wlan_notification_acm_disconnected`；`WlanQueryInterface`
  （`wlan_intf_opcode_current_connection`）读取 SSID。
- Online：与 macOS 相同的 TCP 1.1.1.1:80 + DNS fallback 探测。
- 回调线程模型：Win32 回调在原生线程触发，monitor 任务持有
  `tokio::sync::mpsc::UnboundedSender<NetworkEvent>`（`try_send` 在同步上下文
  可用），回调内直接投递事件进通道，无需 block_on。

**Online 探测实现共享**：TCP/DNS 探测逻辑用纯 `tokio` 写一份
（`probe_online() -> bool`），两平台复用，不依赖平台 API。

### 6.3 防抖

`NetworkMonitor` 内部维护 `last_state`，同类事件 500ms 内的重复上报丢弃，
避免 WiFi 摇摆 / 虚拟网卡抖动导致连续触发。状态变迁（Connect→Disconnect）
不受防抖限制。

### 6.4 AgentService 接线（apps/agent/src/service.rs）

`init()` 中，dispatcher 启动后追加：

```rust
let (net_ev_tx, mut net_ev_rx) = mpsc::channel::<NetworkEvent>(64);
let net_mon_handle = tokio::spawn(NetworkMonitor::spawn(net_ev_tx));
let net_disp_handle = tokio::spawn(async move {
    while let Some(ev) = net_ev_rx.recv().await {
        dispatch_network_event(ev, task_repo.clone(), scheduler_tx.clone()).await;
    }
});
```

`AgentService` 新增字段 `net_mon_handle`、`net_disp_handle`；`run()` 退出路径
与 `scheduler_handle` 一起 abort/await，保证优雅停机。

## 7. 持久化（crates/persistence/src/task_repo.rs）

- **无 schema 迁移**：`triggers` 表 `kind TEXT + config_json TEXT` 直接兼容新变体
  （读侧 `serde_json::from_str::<Trigger>(config_json)` 自动反序列化）。
- **顺手修复保存侧 bug**：现写 `format!("{:?}", trigger.kind)` 对带 payload 的
  变体产生 `"Once { fire_at: ... }"` 这类非纯变体名。改为
  `trigger_kind_name(&TriggerKind) -> &'static str`（match 返回 `"Once"` /
  `"Daily"` / ... / `"Cron"` / `"Fuzzy"` / `"Network"`），`kind` 列语义统一为
  纯变体名。读侧不依赖 `kind` 列反序列化，故对存量数据无兼容风险。

## 8. IPC 层

无新增方法。任务保存/读取走现有 `task.save` / `task.list`，`Trigger` 经
`serde_json` 透传，新变体自动兼容。

## 9. 前端（apps/desktop/src/）

### 9.1 类型（types/task.ts）

```ts
export type FuzzyPeriod =
  | { Daily: true }
  | { Weekdays: true }
  | { Weekends: true }
  | { Weekly: { days_of_week: Weekday[] } };

export type NetworkEventKind = 'Connect' | 'Disconnect' | 'Online';

export type TriggerKind =
  | /* 现有 5 项不变 */ ...
  | { Cron: { expression: string; timezone: string } }
  | { Fuzzy: { period: FuzzyPeriod; window_start: string; window_end: string; timezone: string } }
  | { Network: { events: NetworkEventKind[]; network_name: string | null } };
```

`getTriggerType` 增加 `'Cron' | 'Fuzzy' | 'Network'` 识别分支。

### 9.2 表单（components/task/TriggerEditor.vue）

`triggerTypeOptions` 追加 3 项；`changeKindType` 追加默认值：

- Cron：`{ Cron: { expression: "*/5 * * * *", timezone: defaultTz } }`
- Fuzzy：`{ Fuzzy: { period: { Daily: true }, window_start: "09:00:00",
  window_end: "10:00:00", timezone: defaultTz } }`
- Network：`{ Network: { events: ["Connect"], network_name: null } }`

编辑区块：

- **Cron**：表达式 `NInput`（等宽字体）+ 时区 `NInput`；实时校验——用轻量
  5 字段正则 + 每字段范围检查做预校验，非法时红色提示（不引入 croner-js，
  校验失败仅提示不阻断保存，与后端"非法返回 None"策略一致）。
- **Fuzzy**：周期类型 `NSelect`（每天/工作日/周末/按周几）；选"按周几"时显示
  星期 `NCheckboxGroup`（复用 Weekly 的 dayOptions）；窗口起止两个
  `input[type=time]`；时区 `NInput`。前端校验 `window_end > window_start`。
- **Network**：事件类型 `NCheckboxGroup`（连接时/断开时/可上网时，至少选一个）；
  网络名称 `NInput`（placeholder："留空 = 任意网络；填写则精确匹配 WiFi 名称，
  区分大小写"）。

### 9.3 任务列表展示

任务详情/列表中的触发器摘要文案（现有格式化函数处）补充 3 种新类型的中文摘要：
如 `Cron: */5 * * * *`、`Fuzzy: 每天 09:00–10:00 随机`、
`Network: 连接/断开时 (WiFi: MyHome)`。

## 10. 错误处理

| 场景 | 行为 |
|---|---|
| Cron 表达式非法 | evaluator 返回 None；`AddTask` 时 agent `warn!` 日志；前端实时提示 |
| Fuzzy 窗口非法（end ≤ start） | evaluator 返回 None；前端表单校验阻断 |
| Network events 为空 | evaluator 不适用（事件驱动）；前端校验至少选一个 |
| SSID 获取失败（无 WiFi / API 异常） | `ssid = None`，仅匹配 `network_name = None` 的触发器 |
| Online 探测超时 | 不发 Online 事件；下次状态变化再探测；10s 冷却防风暴 |
| 平台 API 初始化失败 | `error!` 日志，monitor 任务退出，不影响其他调度功能 |

## 11. 测试策略

**Rust 单元测试**：

- `crates/scheduler/src/evaluator.rs`（新增 tests）：
  - Cron：每 5 分钟边界、时区偏移、DST 间隙、非法表达式、跨月字段
  - Fuzzy：随机点落窗内断言（不精确断言时刻）、Weekdays/Weekends/Weekly
    周期过滤、今天窗口剩余重摇、非法窗口、fire 后进入下一周期
- `apps/agent/src/network.rs`：`event_matches` / `name_matches` 纯函数组合测试
  （SSID 精确匹配、大小写、None 通配、多事件类型）。

**Rust 集成测试**（`apps/agent/tests/network_tests.rs`，沿用 service_tests 模式）：

- `network_connect_event_triggers_matching_task`
- `network_disconnect_event_skips_task_without_disconnect_event`
- `network_ssid_filter_triggers_only_on_match`
- `network_event_for_disabled_task_does_not_fire`
- `cron_trigger_fires_and_reschedules`（时间触发器经内存 scheduler 端到端）
- `fuzzy_trigger_fires_within_window`

集成测试直接调用 `dispatch_network_event` 注入 mock `NetworkEvent`，不依赖真实
平台 API，CI 可跑。

**前端 Vitest**：`TriggerEditor` 组件测试覆盖 3 种新类型的默认值初始化、
类型切换、字段绑定与校验提示。

**质量门禁**：`cargo test --all`、`cargo clippy --workspace --all-targets --
-D warnings`、`cargo fmt --check`、`pnpm -C apps/desktop test`、
`pnpm -C apps/desktop run build` 全绿。

## 12. 明确不做（YAGNI）

- 其他平台（Linux 等）的任何适配分支（含前端平台隐藏逻辑）
- cron 6/7 字段（秒级）与 `@` 别名
- Fuzzy 触发点持久化 / 种子复现
- Network 的"网络切换"（A→B 直连）细分事件（Connect 已覆盖）
- 按接口类型（有线/WiFi/蜂窝）过滤（暂以 SSID 过滤满足需求）
- Cron 表达式前端"下次 N 次触发时间"预览（后续迭代可加）
