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

依赖为 croner **2.x**（`Cron::from_str(..).parse()` + `find_next_occurrence`，而非 1.x 的
`Cron::new(..)` / `cron.after(..).next()`）。解析走「字段数 + 词法白名单 + croner 解析」
三重校验：

```rust
/// 解析标准 5 字段 cron：字段数 + 字段词法 + croner 解析三重校验。
fn parse_standard_cron(expression: &str) -> Option<croner::Cron> {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() != 5 {
        return None;
    }
    for (idx, field) in fields.iter().enumerate() {
        if !is_standard_cron_field(field, idx == 3, idx == 4) {
            return None;
        }
    }
    // NOTE: croner 的 `FromStr` 不校验表达式（恒返回 `Ok`），真正的解析发生在 `Cron::parse`
    croner::Cron::from_str(expression)
        .and_then(|mut cron| cron.parse())
        .ok()
}

/// 用 croner 求下一次触发，并保证结果**在绝对时刻上**严格晚于 `after`。
fn cron_next_occurrence(cron: &croner::Cron, tz: &Tz, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let mut anchor = after.with_timezone(tz);
    for _ in 0..3 {
        let candidate = cron.find_next_occurrence(&anchor, false).ok()?;
        let candidate_utc = candidate.with_timezone(&Utc);
        if candidate_utc > after {
            return Some(candidate_utc);
        }
        anchor = candidate_utc.with_timezone(tz);
    }
    None
}
```

- **DST 回拨陷阱**：croner 的 `inclusive = false` 只保证「**本地时间**」严格晚于锚点，
  不保证「**绝对时刻**」晚于 `after`。秋季回拨日同一本地时刻会出现两次（例如
  America/New_York 2026-11-01 的本地 01:30 = 05:30Z EDT 与 06:30Z EST），croner 固定返回
  较早那次，可能落在 `after` 之前。若直接返回，scheduler 会判定 `next_fire_at <= now` →
  `sleep(Duration::ZERO)` → 立即重排又得同一过去时刻，形成**约 1 小时的忙循环并反复触发任务**
  （并发策略只能限流、止不住再触发）。因此必须像上面这样以候选的绝对时刻**重锚再搜**，
  跨过重复的那个小时。
- **标准 5 字段词法白名单**：单个字段只允许数字、`*`、`?`、`,`、`-`、`/`，以及月/周字段的
  三字母别名（JAN..DEC / SUN..SAT）。必须**拒绝 Quartz 扩展（`L` / `#` / `W`）**等非标准记号：
  croner 的 parse 会接受它们，但 `find_next_occurrence` 可能一路搜索到
  `YEAR_UPPER_LIMIT = 5000` 才放弃（实测 `L * * * *` 单次求值 35 秒），而该调用在异步
  `Scheduler::run` 内是**同步**的（AddTask 注册、fire 后重排、clock-jump 重建三处），
  一条这样的任务会让**全部**定时任务停摆数十秒。
- 表达式非法返回 `None`，不阻断同任务其他触发器；agent 侧以 `warn!` 日志记录一次
  （在 `AddTask` 注册路径），前端表单实时校验兜底。
- 仅支持 5 字段；`croner` 的 `@` 别名与 6 字段秒级模式不启用。
- 校验与求值共用同一实现（单一事实来源）：`validate_cron_expression` 复用
  `parse_standard_cron` + `cron_next_occurrence`，因此「语法合法但永不发生」的表达式
  （如 `0 0 30 2 *`、`0 0 31 4 *`）也判为非法并触发 `warn!`，避免触发器静默永不触发且零日志。
  代价是最坏约 236ms（`0 0 30 2 *` 需扫到 5000 年），仅在任务加载/保存时按 Cron 触发器
  调用一次。

### 5.3 Fuzzy 分支

```rust
TriggerKind::Fuzzy { period, window_start, window_end, timezone } => {
    if window_end <= window_start {
        return None;
    }
    let tz: Tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
    let local_after = after.with_timezone(&tz);
    let mut candidate_date = local_after.date_naive();

    // 上限 14 天。Weekly 只含单日且当日窗口已开始时，最坏需跨 7 天才命中；
    // 这里留一周余量，避免边界收得过紧导致静默返回 None（触发器会永久停止调度）。
    for _ in 0..14 {
        // 窗口已开始（含已结束）即跳过该日：保证每个匹配窗口至多触发一次。
        // 否则 fire 之后 scheduler 以 now 重新求值，会在同一天剩余窗口内反复重摇。
        let window_not_started =
            candidate_date > local_after.date_naive() || *window_start > local_after.time();
        if period.matches_weekday(candidate_date.weekday()) && window_not_started {
            // 该日窗口内随机选点
            let picked = pick_random_time_in_window(window_start, window_end);
            if let Some(utc) = resolve_candidate(&tz, candidate_date, picked, after) {
                return Some(utc);
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

- 每个周期窗口内**至多触发一次**：判定以"窗口是否已开始"为准——若 `after`
  已不早于该日 `window_start`，则整日跳过、顺延到下一个匹配日。因此 fire 后
  scheduler 以 `now` 重排时必然落到下一匹配窗口；agent 在窗口进行中启动、或任务
  在窗口进行中才创建时，该窗口不再触发，首次触发落在下一个匹配日。
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

> 实现偏差：监控器不是 `NetworkMonitor` 类型上的方法，而是**自由函数**
> `spawn_network_monitor(event_tx) -> tokio::task::JoinHandle<()>`
> （`apps/agent/src/network.rs`），无 `NetworkMonitor` 类型。

`dispatch_network_event` 匹配规则：任务 `enabled`、触发器 `enabled`、事件类型命中
`events`、SSID 命中 `network_name`；同一任务多个 Network 触发器任一命中即
`TriggerNow` 一次（`break` 防重复派发）。并发策略（SkipIfRunning 等）由现有
执行管线兜底。

### 6.2 平台实现

**macOS**（`#[cfg(target_os = "macos")]`）：

- 依赖：`block2` / `objc2` / `objc2-core-wlan`（workspace target 依赖）。
  Network.framework 是纯 C API，`objc2-network` 在 crates.io 上只有 `0.0.0` 的
  「Reserved crate name」占位版本、无任何绑定，因此**手写最小 FFI 声明**
  （`nw_path_monitor_create` / `nw_path_monitor_set_queue` /
  `nw_path_monitor_set_update_handler` / `nw_path_monitor_start` /
  `nw_path_monitor_cancel` / `nw_path_get_status` + `dispatch_queue_create`），
  用 `RcBlock` 承载 update handler。
- Connect/Disconnect：`nw_path_get_status` 为 `satisfied` → Connect，
  `unsatisfied` → Disconnect，其余中间态（Satisfiable / Invalid）不触发。
  `start` 时 update handler 会立即以当前 path 触发一次快照回调（非状态变迁），
  整帧跳过第一次回调，与 Windows 侧 `initialnotification = false` 对齐。
- SSID：CoreWLAN `CWWiFiClient` 获取当前关联 SSID；有线网络返回 `None`。
- Online：path 变为 Satisfied 后，spawn 一次性探测——TCP 连
  `1.1.1.1:80`（2s 超时），失败则 fallback DNS 解析 `example.com`；成功发
  `Online`。探测带 10s 冷却，避免断连风暴时反复探测。

**Windows**（`#[cfg(target_os = "windows")]`）：

- 依赖：`windows` crate（features：
  `Win32_NetworkManagement_IpHelper`、
  `Win32_NetworkManagement_Ndis`、
  `Win32_NetworkManagement_WiFi`、
  `Win32_Foundation`、
  `Win32_Networking_WinSock`）。
- Connect/Disconnect：`NotifyIpInterfaceChange`（`AF_UNSPEC`，
  `initialnotification = false`）感知接口变动。回调 `MIB_IPINTERFACE_ROW` **没有
  `OperStatus` 字段**（`OperStatus` 属于 `MIB_IF_ROW2`，本 API 拿不到），只有
  `Connected: bool`（是否已连到网络接入点）。因此事件分类为：
  `MibDeleteInstance`（接口行被移除）或 `!Connected` → Disconnect，否则 Connect。
  若只判 `MibDeleteInstance`，拔网线 / 禁用网卡产生的 `MibParameterNotification`
  （接口行仍在、仅参数变化且 `Connected=false`）会被反向误报成 Connect，而
  「断开网络时」不触发。
  - **已知残留误报**：接口仍处于连接状态时的纯参数变更（MTU / metric / DNS）
    `Connected` 仍为 `true`，会多报一次 Connect。彻底消除需按 `InterfaceLuid`
    维护状态迁移表，属后续迭代。
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

`spawn_network_monitor` 内部为每平台维护一个 `Debouncer`（字段名 `last_kind`，
`Mutex<Option<(NetworkEventKind, Instant)>>`），同类事件 500ms 内的重复上报丢弃，
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
// serde：无字段变体序列化为字符串，带字段变体序列化为对象
export type FuzzyPeriod =
  | 'Daily'
  | 'Weekdays'
  | 'Weekends'
  | { Weekly: { days_of_week: Weekday[] } };

export type NetworkEventKind = 'Connect' | 'Disconnect' | 'Online';

export type TriggerKind =
  | /* 现有 5 项不变 */ ...
  | { Cron: { expression: string; timezone: string } }
  | { Fuzzy: { period: FuzzyPeriod; window_start: string; window_end: string; timezone: string } }
  | { Network: { events: NetworkEventKind[]; network_name: string | null } };
```

> 注意 `FuzzyPeriod` 的 serde 线格式：`Daily` / `Weekdays` / `Weekends` 是**字符串**
> （如 `"Daily"`），只有 `Weekly` 是对象 `{ Weekly: { days_of_week: [...] } }`。

`getTriggerType` 增加 `'Cron' | 'Fuzzy' | 'Network'` 识别分支，并在 `TriggerType`
追加 `'Unknown'`：该函数在模板渲染路径被直接调用，**不得抛异常**（否则畸形数据会让
任务列表白屏）；非对象 / 未知变体一律返回 `'Unknown'`，未知字符串同样返回 `'Unknown'`。

### 9.2 表单（components/task/TriggerEditor.vue）

`triggerTypeOptions` 追加 3 项；`changeKindType` 追加默认值：

- Cron：`{ Cron: { expression: "*/5 * * * *", timezone: defaultTz } }`
- Fuzzy：`{ Fuzzy: { period: "Daily", window_start: "09:00:00",
  window_end: "10:00:00", timezone: defaultTz } }`（`period` 为字符串，非 `{ Daily: true }`）
- Network：`{ Network: { events: ["Connect"], network_name: null } }`

编辑区块：

- **Cron**：表达式 `NInput`（等宽字体）+ 时区 `NInput`；实时校验——轻量 5 字段
  词法/范围检查（`validateCronExpression`，拒绝多斜杠步长如 `1/2/3` 等 croner 会报
  `Invalid stepped range syntax` 的写法），非法时红色提示（不引入 croner-js，
  校验失败仅提示不阻断保存，与后端"非法返回 None"策略一致）。
- **Fuzzy**：周期类型 `NSelect`（每天/工作日/周末/按周几）；选"按周几"时显示
  星期 `NCheckboxGroup`（复用 Weekly 的 dayOptions）；窗口起止两个
  `input[type=time]`；时区 `NInput`。前端校验 `window_end > window_start`，
  非法时**红色提示、不阻断保存**（与 Cron 一致）。
- **Network**：事件类型 `NCheckboxGroup`（连接时/断开时/可上网时，至少选一个）；
  网络名称 `NInput`（placeholder："留空 = 任意网络；填写则精确匹配 WiFi 名称，
  区分大小写"）。

### 9.3 任务列表展示

任务详情/列表中的触发器摘要文案（`describeTrigger`）补充 3 种新类型的中文摘要，
实际输出格式为 `Cron: */5 * * * *`、`模糊时间：每天 09:00:00-10:00:00 随机`、
`网络变动：连接时 (MyHome)` / `网络变动：连接时/断开时（任意网络）`。
`describeTrigger` 同样在渲染路径被调用，对缺失字段 / 未知事件必须返回兜底文案，
不得抛异常、不得输出字面量 `undefined`（未知 Network 事件跳过，不留尾斜杠）。

## 10. 错误处理

| 场景 | 行为 |
|---|---|
| Cron 表达式非法 | evaluator 返回 None；`AddTask` 时 agent `warn!` 日志；前端实时提示 |
| Cron 表达式永不发生（如 `0 0 30 2 *`） | `validate_cron_expression` 判为非法 → agent `warn!` 日志（不再静默） |
| Cron 含 Quartz 扩展（`L`/`#`/`W`） | 词法白名单直接拒绝，返回 None 且不进入 croner 搜索（避免同步阻塞调度循环） |
| Fuzzy 窗口非法（end ≤ start） | evaluator 返回 None；前端表单校验**红色提示、不阻断保存** |
| Network events 为空 | evaluator 不适用（事件驱动）；前端校验至少选一个 |
| SSID 获取失败（无 WiFi / API 异常） | `ssid = None`，仅匹配 `network_name = None` 的触发器 |
| Online 探测超时 | 不发 Online 事件；下次状态变化再探测；10s 冷却防风暴 |
| 平台 API 初始化失败 | `error!` 日志，monitor 任务退出，不影响其他调度功能 |

## 11. 测试策略

**Rust 单元测试**：

- `crates/scheduler/src/evaluator.rs`（tests 模块）：
  - Cron：每 5 分钟边界与严格晚于 `after`、时区偏移、非法表达式、非 5 字段、
    **DST 回拨日必须返回未来时刻**（`cron_fall_back_dst_returns_future`，
    America/New_York + Europe/London 采样断言 `next > after`）、
    **Quartz 扩展 `L` 被快速拒绝**（`cron_quartz_extensions_are_rejected_fast`，
    断言返回 None 且耗时 < 1s）、**永不发生的日期返回 None**
    （`cron_impossible_date_returns_none`）
  - `validate_cron_expression`：合法 5 字段（含 `0 0 1 1 0`、`0 0 * * MON-FRI`）、
    非法取值/字段数/`@` 别名、`L * * * *`、`0 0 30 2 *`、`1/2/3 * * * *` 均判非法
  - Fuzzy：随机点落窗内断言（不精确断言时刻）、Weekdays/Weekends/Weekly
    周期过滤、今天窗口剩余重摇、非法窗口、fire 后进入下一周期
- `apps/agent/src/network.rs`：`event_matches` / `name_matches` 纯函数组合测试
  （SSID 精确匹配、大小写、None 通配、多事件类型）、`dispatch_network_event` 匹配
  启用任务测试、`tcp_reachable` 可达/不可达测试。

**Rust 集成测试**（`apps/agent/tests/network_tests.rs`）：

- `network_connect_event_triggers_matching_task`
- `network_disconnect_event_skips_task_without_disconnect`
- `network_ssid_filter_triggers_only_on_match`
- `network_event_for_disabled_task_does_not_fire`

集成测试直接调用 `dispatch_network_event` 注入 mock `NetworkEvent`，不依赖真实
平台 API，CI 可跑。

> 实现现状：Cron / Fuzzy **没有**端到端集成测试，覆盖仅来自上表的 evaluator 单测
> （含本轮新增的 DST / 词法白名单回归）。Fuzzy 的随机性与 scheduler 重排语义通过
> `fuzzy_fires_at_most_once_per_window` 等单测模拟，不跑真实调度循环。

**前端 Vitest**：`apps/desktop/tests/triggerKinds.test.ts` 覆盖 3 种新类型的线格式
识别、`validateCronExpression`（含多斜杠步长拒绝）、`getTriggerType` 与
`describeTrigger` 的畸形输入兜底；另有 `views.test.ts` 覆盖表单交互。

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
