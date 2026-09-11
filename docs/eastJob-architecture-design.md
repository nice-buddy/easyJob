# 跨平台定时任务管理器设计方案

**技术栈：Rust + Tauri 2 + Vue 3 + TypeScript + SQLite**

**目标平台：macOS / Windows**

**文档状态：Architecture Design / V1**

---

## 1. 项目概述

本项目是一款跨平台桌面定时任务管理程序，用于创建、管理和执行本地自动化任务。

核心能力包括：

- 按时间触发任务：一次性、每天、每周、每月、固定间隔、Cron、随机时间区间（如上午8点到8点半之间随机）等。
- 按程序生命周期触发：Agent 启动、用户登录后的应用启动等。
- 执行外部程序。
- 执行 CMD、PowerShell、sh/zsh 或其他脚本解释器。
- 捕获标准输出、标准错误和退出码。
- 支持任务超时、取消、重试、并发策略。
- 保存任务执行历史与日志。
- 支持任务启用、禁用、手动执行和立即跳过。
- 支持系统睡眠、唤醒、时区变化、夏令时和系统时间修改。
- UI 关闭后任务仍然可以继续运行。
- 不使用 Windows Task Scheduler 或 macOS launchd 承载业务任务调度。

### 核心架构原则

> **Tauri/Vue 负责控制台和用户交互，Rust Agent 负责调度、执行和运行状态。**

任务触发完全由自己的 Scheduler/Trigger Engine 完成。

Windows Task Scheduler、macOS launchd 等系统调度器不参与业务任务的计算、排程或触发。

---

# 2. 设计目标

## 2.1 必须达到

1. Windows 与 macOS 共享核心业务模型。
2. 任务调度逻辑跨平台一致。
3. 平台差异封装在 Platform Adapter。
4. GUI 关闭后调度继续运行。
5. Agent 与 UI 生命周期解耦。
6. 一个任务不会因为 UI 卡顿而延迟执行。
7. 调度器不能因为一个任务执行时间很长而阻塞其他任务。
8. 系统时间发生跳变时不会出现大量重复执行。
9. 计算时间规则使用墙上时间（wall clock），等待时间使用单调时钟（monotonic clock）。
10. 所有任务执行都有明确的 Execution Record。
11. 所有执行状态均可恢复、追踪和审计。
12. 外部命令执行默认不通过字符串拼接形成 shell 命令，减少注入风险。
13. 所有 UI 界面除了输入框外，禁止拖动鼠标选中。
14. 程序必须支持 Windows 的系统托盘和 macOS 的菜单栏。
15. 执行 powershell、cmd、sh 等脚本时，需要静默执行，不能弹出命令行窗口。

## 2.2 非目标

V1 不考虑：

- 远程任务调度。
- 多机器任务同步。
- 云端控制台。
- 服务端 API。
- Windows Task Scheduler 任务导入/导出。
- macOS launchd plist 任务导入/导出。
- 系统级 root/Administrator 守护进程。
- 跨用户账号的中央任务服务。

---

# 3. 总体架构

```text
                           ┌─────────────────────┐
                           │      Vue 3 UI       │
                           │                     │
                           │ Dashboard           │
                           │ Task Editor         │
                           │ History             │
                           │ Settings            │
                           └──────────┬──────────┘
                                      │
                                  Tauri IPC
                                      │
                           ┌──────────▼──────────┐
                           │    Tauri Desktop    │
                           │                     │
                           │ Window              │
                           │ Tray                │
                           │ Menu                │
                           │ Frontend Bridge     │
                           └──────────┬──────────┘
                                      │
                              Local IPC / Socket
                                      │
                     ┌────────────────▼────────────────┐
                     │          Rust Agent             │
                     │                                 │
                     │ Scheduler                       │
                     │ Trigger Engine                  │
                     │ Task Repository                 │
                     │ Execution Manager               │
                     │ Process Manager                 │
                     │ Event Bus                       │
                     │ History / Logging               │
                     │ Recovery                         │
                     └───────────────┬─────────────────┘
                                     │
                 ┌───────────────────┴───────────────────┐
                 │                                       │
        ┌────────▼────────┐                    ┌────────▼────────┐
        │ Windows Adapter │                    │ macOS Adapter   │
        │                 │                    │                 │
        │ cmd.exe         │                    │ sh/zsh          │
        │ PowerShell      │                    │ .app / binaries │
        │ CreateProcess   │                    │ Process APIs    │
        │ Process Groups  │                    │ Process Groups  │
        └─────────────────┘                    └─────────────────┘

                              │
                              ▼
                         ┌──────────┐
                         │ SQLite   │
                         │ WAL      │
                         └──────────┘
```

---

# 4. 为什么采用独立 Agent

Tauri 应用本身不应该承担所有调度职责。

如果把 Scheduler 放在前端或 Tauri 窗口生命周期里，会出现：

- 窗口关闭以后任务停止。
- WebView 卡顿影响调度。
- 前端页面刷新导致运行状态丢失。
- UI 代码异常可能影响调度器。
- 长时间后台运行不够稳定。

因此采用：

```text
Tauri Desktop
    │
    └── Rust Agent
          ├── Scheduler
          ├── Executor
          ├── Database
          └── Recovery
```

Agent 是真正的运行时核心。

Tauri 只负责：

- 启动/停止 Agent。
- 查询状态。
- 修改任务。
- 手动执行。
- 查看日志。
- UI/Tray。
- 设置和配置。

---

# 5. Agent 生命周期

## 5.1 默认运行模式

建议应用默认采用“后台运行”模式：

```text
启动 App
    ↓
启动 Agent
    ↓
Agent 加载数据库
    ↓
恢复任务运行状态
    ↓
Scheduler 开始工作
    ↓
用户关闭主窗口
    ↓
窗口隐藏
    ↓
Tray + Agent 继续运行
```

也就是说：

> **关闭主窗口 ≠ 退出程序 ≠ 停止任务。**

## 5.2 两种退出行为

### 关闭窗口

进程依旧在 Windows 系统托盘或 macOS 菜单栏继续运行

### 退出应用

关闭窗口，停止所有调度任务，退出所有进程

---

# 6. 自动启动

这里必须区分：

### 业务调度

由：

```text
Rust Scheduler
```

实现。

### Agent 自启动

只负责让应用/Agent 在用户登录后运行。

可使用平台原生的“登录启动”机制：

- Windows：用户登录启动项（使用注册表`计算机\HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`）。
- macOS：Login Item / 应用自启动机制。

这些机制只负责：

```text
用户登录
    ↓
启动应用
    ↓
应用启动 Agent
    ↓
Scheduler 自己计算所有任务
```

不允许：

```text
Windows Task Scheduler → 执行任务
macOS launchd → 执行任务
```

因此系统调度器不是本产品的任务引擎。

---

# 7. Rust Workspace 设计

推荐采用 Cargo Workspace。

```text
workspace/
│
├── apps/
│   ├── desktop/
│   │   └── Tauri application
│   │
│   └── agent/
│       └── Agent executable
│
├── crates/
│   ├── domain/
│   ├── scheduler/
│   ├── executor/
│   ├── persistence/
│   ├── ipc/
│   ├── platform/
│   ├── logging/
│   └── common/
│
├── src-ui/
│   ├── views/
│   ├── components/
│   ├── stores/
│   ├── services/
│   └── types/
│
└── migrations/
```

推荐职责：

### domain

纯业务模型：

```text
Task
Trigger
Action
Execution
RetryPolicy
ConcurrencyPolicy
```

不能依赖：

- Tauri
- Windows API
- macOS API
- SQLite

这样可以做纯单元测试。

### scheduler

负责：

- Trigger 注册。
- next fire time 计算。
- 调度队列。
- 时间跳变处理。
- sleep/wake。
- missed trigger。

### executor

负责：

- 进程创建。
- stdout/stderr。
- exit code。
- timeout。
- cancellation。
- retry。
- concurrency。

### persistence

负责：

- SQLite。
- migration。
- repository。
- transaction。
- history。

### platform

提供：

```text
Platform
├── Windows
└── macOS
```

封装：

- 操作系统信息。
- shell 路径。
- 外部程序启动。
- 进程树控制。
- 登录启动。
- 系统睡眠/唤醒相关事件。

### ipc

定义 Agent 与 Tauri 的协议。

### logging

统一：

```text
tracing
structured logging
log rotation
```

---

# 8. Vue 3 前端架构

推荐：

```text
Vue 3
TypeScript
Pinia
Vue Router
Vite
```

目录：

```text
src-ui/
│
├── app/
│   ├── router.ts
│   └── app.ts
│
├── views/
│   ├── DashboardView.vue
│   ├── TasksView.vue
│   ├── TaskDetailView.vue
│   ├── HistoryView.vue
│   ├── ExecutionView.vue
│   └── SettingsView.vue
│
├── components/
│   ├── task/
│   ├── trigger/
│   ├── action/
│   ├── execution/
│   └── common/
│
├── stores/
│   ├── taskStore.ts
│   ├── agentStore.ts
│   ├── settingsStore.ts
│   └── executionStore.ts
│
├── services/
│   ├── tauri.ts
│   ├── agent.ts
│   └── events.ts
│
└── types/
```

---

# 9. IPC 设计

Tauri Command 适合：

```text
request → response
```

例如：

```text
listTasks
getTask
createTask
updateTask
deleteTask
runTask
stopExecution
getExecution
getAgentStatus
```

Tauri Events 适合：

```text
one-way state change
```

例如：

```text
task.started
task.output
task.finished
task.failed
agent.status_changed
scheduler.next_run_changed
```

Tauri 官方 IPC 机制区分 Commands 与 Events；Commands 适合请求/响应，Events 更适合生命周期和状态变更通知。 citeturn410358search7

原则：

> 前端不能直接操作 SQLite，也不能直接运行 shell。

所有敏感操作必须经过 Rust Core / Agent。

---

# 10. Agent IPC

Tauri Core 与 Agent 之间不建议直接共享 Rust 内存。

推荐：

```text
Tauri
  │
  ▼
Local IPC
  │
  ▼
Agent
```

协议建议采用：

```text
JSON messages over local IPC
```

消息模型：

```text
{
  "id": "request-id",
  "method": "task.list",
  "params": {}
}
```

响应：

```text
{
  "id": "request-id",
  "ok": true,
  "data": {}
}
```

事件：

```text
{
  "event": "execution.finished",
  "data": {}
}
```

Agent 端 IPC 服务只监听本机。

建议：

- Windows：Named Pipe。
- macOS：Unix Domain Socket。

如果实现成本较高，也可以使用：

```text
127.0.0.1 + random port + authentication token
```

但默认优先本机 IPC。

---

# 11. Domain Model

## 11.1 Task

```text
Task
├── id
├── name
├── description
├── enabled
├── triggers[]
├── actions[]
├── execution_policy
├── environment
├── working_directory
├── created_at
├── updated_at
└── metadata
```

---

# 12. Trigger 模型

Trigger 分为：

```text
Time Trigger
Lifecycle Trigger
Event Trigger
```

---

# 13. Time Trigger

## 13.1 Once

```text
2026-10-01 20:00:00
```

字段：

```text
type = once
datetime
timezone
```

---

# 14. Daily

```text
每天 08:30
```

字段：

```text
type = daily
time = 08:30
timezone
```

---

# 15. Weekly

例如：

```text
每周一、三、五
18:00
```

字段：

```text
days_of_week[]
time
timezone
```

---

# 16. Monthly

例如：

```text
每月 1 日 09:00
```

也应支持：

```text
每月最后一天
```

以及：

```text
每月第一个 Monday
```

V1 可先支持：

```text
day-of-month
```

复杂 Calendar Rule 留到 V2。

---

# 17. Interval

例如：

```text
每隔 30 分钟
```

但是必须明确两种语义。

### Fixed Interval

```text
第一次触发
↓ 30 min
↓ 30 min
↓ 30 min
```

### Calendar Interval

例如：

```text
每天 08:00
```

不应把 Daily 当作 24 小时 interval。

---

# 18. Cron

V2 支持标准 Cron：

```text
*/15 * * * *
```

建议内部统一转成：

```text
CronTrigger
```

但不要让 Cron 成为底层 Scheduler 的唯一抽象。

最终调度接口应该是：

```text
Trigger
   ↓
next_occurrence(after)
```

这样：

```text
DailyTrigger
WeeklyTrigger
IntervalTrigger
CronTrigger
```

都实现统一接口。

---

# 19. Lifecycle Trigger

建议定义：

```text
AgentStarted
ApplicationStarted
UserSessionStarted
```

由于本项目不通过系统 Scheduler 实现任务，因此“开机”在无系统级 daemon/service 的情况下应明确解释为：

> Agent/Application 启动后的第一次生命周期事件。

如果未来需要真正意义上的：

```text
系统启动、用户尚未登录
```

则必须引入系统服务/daemon；这属于新的架构层，不应偷偷混进当前版本。

---

# 20. Event Trigger

未来支持：

```text
FileChanged
ProcessStarted
ProcessExited
NetworkChanged
SystemWake
BatteryStateChanged
```

V1 不必全部实现。

为了避免未来破坏模型，Trigger 接口从第一天就应该允许：

```text
event trigger
```

---

# 21. Trigger Engine

Trigger Engine 负责：

```text
Trigger Definition
       ↓
Next Occurrence
       ↓
Scheduler
```

核心接口语义：

```text
next_occurrence(trigger, after)
```

必须满足：

1. 不修改数据库状态。
2. 输入相同得到相同结果。
3. 具有明确 timezone。
4. 能表示不存在下一次触发。
5. 能处理 DST。

---

# 22. Scheduler

Scheduler 使用：

```text
Priority Queue / Min Heap
```

队列元素：

```text
ScheduledItem
├── task_id
├── trigger_id
├── next_fire_at
└── generation
```

按：

```text
next_fire_at
```

升序排列。

算法：

```text
load enabled tasks
        ↓
calculate next fire
        ↓
insert queue
        ↓
take earliest item
        ↓
sleep until deadline
        ↓
re-check current wall clock
        ↓
fire
        ↓
calculate next occurrence
        ↓
reinsert
```

不要：

```text
while true {
    scan all tasks
    sleep(1 sec)
}
```

否则任务规模增加后效率会迅速下降。

---

# 23. 调度等待

Tokio 的 `sleep_until` / `sleep` 适合长时间异步等待，并且取消 sleep 不要求额外清理；Tokio 的 Interval 还提供 missed-tick 策略控制。 citeturn410358search1turn410358search3turn410358search2

但本系统不建议简单地：

```text
for task:
    interval.tick()
```

因为我们需要：

- Calendar Rule。
- Timezone。
- Cron。
- DST。
- 系统时间修改。
- Dynamic Task 修改。

所以 Scheduler 应由：

```text
业务 wall-clock 计算
+
Tokio monotonic timer
```

组合实现。

---

# 24. Wall Clock 与 Monotonic Clock

这是整个项目非常关键的设计点。

### Wall Clock

用于：

```text
今天几点
星期几
2026-10-01
America/Los_Angeles
```

### Monotonic Clock

用于：

```text
等待 30 秒
等待下一个 scheduler deadline
进程 timeout
```

不能使用：

```text
current_time + duration
```

简单代替所有计时逻辑。

否则用户手动修改系统时间后会发生异常。

---

# 25. 系统时间变化

需要检测：

```text
Clock moved forward
Clock moved backward
Timezone changed
```

检测到后：

```text
invalidate scheduler queue
        ↓
reload enabled triggers
        ↓
recalculate all next occurrences
        ↓
rebuild priority queue
```

---

# 26. 睡眠与唤醒

必须定义：

```text
machine sleeps
       ↓
hours pass
       ↓
machine wakes
```

不能简单认为：

```text
sleep 8 hours
```

然后继续执行。

唤醒以后：

```text
now = current wall clock
```

重新计算所有过期 Trigger。

---

# 27. Missed Trigger Policy

每个任务都应该有：

```text
MissedRunPolicy
```

推荐：

```text
RunOnce
Skip
RunAll
```

V1 建议只做：

```text
RunOnce
Skip
```

例如：

```text
任务每天 09:00

电脑 08:50 睡眠
10:00 唤醒
```

### RunOnce

10:00 唤醒后立即执行一次。

### Skip

当天 09:00 已错过，则当天不执行。

---

# 28. 时间窗口

允许配置：

```text
Trigger tolerance
```

例如：

```text
09:00 ± 30 seconds
```

主要用于：

- 系统负载。
- Agent 恢复。
- 睡眠唤醒。

---

# 29. DST

涉及 timezone 的 Trigger 必须明确：

```text
America/Los_Angeles
Asia/Shanghai
Europe/Berlin
```

对于不存在的时间，例如 DST 跳过：

```text
02:30 不存在
```

建议策略：

```text
ShiftForward
Skip
```

对于重复时间：

```text
01:30 出现两次
```

建议默认：

```text
FirstOccurrence
```

高级设置提供：

```text
First
Second
Both
```

---

# 30. Action Model

Action 类型：

```text
ExecuteProgram
ExecuteShell
ExecutePowerShell
ExecuteCmd
ExecuteScript
```

建议不要让前端直接把：

```text
"backup.exe -x -y"
```

当成一个不可解析的字符串保存。

优先模型：

```text
program
arguments[]
working_directory
environment{}
```

---

# 31. ExecuteProgram

```text
Executable:
C:\Tools\backup.exe

Arguments:
--source
D:\data
--target
D:\backup
```

内部：

```text
program
args[]
```

避免字符串 shell parsing。

---

# 32. CMD

Windows：

```text
cmd.exe /c ...
```

但应把：

```text
command string
```

明确标记为：

```text
shell command
```

因为 shell command 天然存在：

- quoting
- expansion
- escaping
- redirection

等语义。

---

# 33. PowerShell

建议支持：

```text
powershell.exe
pwsh.exe
```

并允许：

```text
Interpreter
Arguments
Script File
```

例如：

```text
pwsh
-NoProfile
-File
backup.ps1
```

---

# 34. Shell

macOS 支持：

```text
/bin/sh
/bin/zsh
```

可选：

```text
bash
fish
```

脚本文件模式：

```text
Interpreter + Script Path + Arguments
```

不应根据文件扩展名自动推断唯一解释器。

---

# 35. Execution Manager

执行链：

```text
Scheduler
   ↓
ExecutionManager
   ↓
ConcurrencyPolicy
   ↓
ProcessRunner
   ↓
Process
```

Execution Manager 不负责判断“什么时候运行”。

只负责：

> 现在有人请求运行这个任务，我该怎么运行。

---

# 36. 并发策略

每个 Task 可以设置：

```text
AllowParallel
SkipIfRunning
Queue
Replace
```

推荐 V1：

```text
AllowParallel
SkipIfRunning
QueueOne
```

例如备份任务：

```text
08:00 started
08:05 still running
09:00 trigger
```

如果：

```text
SkipIfRunning
```

09:00 直接跳过。

如果：

```text
QueueOne
```

则记录一次 pending execution。

---

# 37. 全局并发

应用级别也需要：

```text
max_concurrent_processes
```

例如：

```text
8
```

防止用户建立 500 个任务并同时启动 500 个进程。

调度器触发不等于立即创建无限进程。

---

# 38. Process Manager

Process Manager 负责：

```text
spawn
stdin
stdout
stderr
exit
timeout
terminate
kill tree
```

其中：

```text
terminate
```

应该尽可能优雅。

```text
kill
```

用于最终强制结束。

---

# 39. 进程树

必须考虑：

```text
Task
 ↓
PowerShell
 ↓
script
 ↓
child process
```

不能只保存第一层 PID。

任务停止时，应尽量终止整个 process tree/group。

Windows 与 macOS 的实现分别放入 Platform Adapter。

---

# 40. Timeout

每个 Action：

```text
timeout
```

例如：

```text
5 minutes
```

超时后：

```text
Graceful termination
       ↓
wait
       ↓
Force kill
```

最终 Execution：

```text
TimedOut
```

---

# 41. Environment

任务可以指定：

```text
PATH
HOME
TEMP
CUSTOM_VAR
```

建议两种模式：

```text
inherit
replace
```

默认：

```text
inherit parent environment
+
task overrides
```

---

# 42. Working Directory

每个任务支持：

```text
inherit
explicit path
script directory
```

默认建议：

```text
explicit path
```

因为后台运行环境与用户在 Terminal 中运行时经常不同。

---

# 43. Execution Result

执行结束以后统一形成：

```text
Execution
├── id
├── task_id
├── trigger_id
├── started_at
├── finished_at
├── duration
├── status
├── exit_code
├── stdout
├── stderr
└── error
```

Status：

```text
Queued
Running
Succeeded
Failed
TimedOut
Cancelled
Skipped
```

---

# 44. 日志设计

日志分两类。

## Agent Log

用于诊断：

```text
scheduler restarted
database opened
IPC connected
task queue rebuilt
```

## Execution Log

属于用户任务：

```text
stdout
stderr
exit_code
```

两者必须分离。

---

# 45. 日志保存

建议：

```text
logs/
├── agent.log
├── agent.1.log
├── agent.2.log
└── executions/
```

Execution stdout/stderr 不建议无限增长。

可以配置：

```text
max_output_size
```

超出后：

```text
truncate
```

必要时保存完整日志到单独文件。

---

# 46. SQLite 数据设计

推荐数据库：

```text
SQLite
```

采用：

```text
WAL
foreign keys
migration
transaction
```

主要表：

```text
tasks
triggers
actions
task_runs
run_outputs
settings
agent_state
```

---

# 47. tasks

建议字段：

```text
id
name
description
enabled
created_at
updated_at
version
```

其中：

```text
version
```

用于检测编辑冲突和运行时配置更新。

---

# 48. triggers

```text
id
task_id
type
config_json
enabled
created_at
updated_at
```

Trigger 结构采用：

```text
type + config_json
```

原因是未来增加：

```text
Cron
FileChanged
ProcessStarted
NetworkChanged
```

不需要不停修改表结构。

核心字段稳定：

```text
id
task_id
type
config_json
```

---

# 49. actions

```text
id
task_id
sequence
type
config_json
enabled
```

这样一个任务可以：

```text
Trigger
  ↓
Action 1
  ↓
Action 2
  ↓
Action 3
```

---

# 50. Action Chain

V1 建议支持顺序执行：

```text
Action 1
   ↓ success
Action 2
   ↓ success
Action 3
```

失败策略：

```text
Stop
Continue
```

未来可支持：

```text
OnSuccess
OnFailure
```

形成简单 DAG。

---

# 51. task_runs

```text
id
task_id
trigger_id
status
scheduled_at
started_at
finished_at
exit_code
error_message
```

---

# 52. run_outputs

不要把巨大 stdout/stderr 全部塞进 task_runs。

建议：

```text
task_runs
      ↓
run_outputs
```

字段：

```text
run_id
stream
content
chunk_index
```

对于极大量日志可以存文件，只在数据库保存：

```text
log_file_path
```

---

# 53. Agent State

Agent 需要保存：

```text
last_started_at
last_shutdown_at
last_crash_detected_at
schema_version
```

但不要把易变的 runtime queue 全部持久化。

Scheduler 的优先队列应该能够从：

```text
tasks + triggers
```

重新构建。

这样 Agent 崩溃以后可以：

```text
restart
↓
load DB
↓
recalculate queue
```

---

# 54. Agent Recovery

启动：

```text
Agent starts
     ↓
Open database
     ↓
Run migrations
     ↓
Validate tasks
     ↓
Load executions still marked Running
     ↓
Mark them as Interrupted
     ↓
Rebuild Scheduler
```

之前：

```text
Running
```

但 Agent 崩溃后没有结束的任务，应修改为：

```text
Interrupted
```

不能伪造为：

```text
Succeeded
```

---

# 55. Crashed Execution

如果 Agent 崩溃：

```text
Execution = Running
```

重启后：

```text
Running → Interrupted
```

增加字段：

```text
termination_reason = agent_crash
```

---

# 56. UI 状态同步

UI 启动：

```text
connect Agent
     ↓
get snapshot
     ↓
subscribe events
```

不要完全依赖事件恢复 UI。

原因：

```text
UI 曾经关闭
↓
错过很多 events
```

所以必须：

```text
Snapshot + Event Stream
```

架构：

```text
Initial Snapshot
       ↓
event stream
       ↓
local store
```

---

# 57. UI Store

Pinia stores：

```text
agentStore
taskStore
executionStore
settingsStore
```

原则：

> Pinia 只是 UI 缓存，不是真实数据源。

真实数据源：

```text
SQLite + Agent runtime
```

---

# 58. Task Editor

建议分为：

```text
General
Trigger
Action
Execution
Advanced
```

### General

```text
Name
Description
Enabled
```

### Trigger

```text
+ Add Trigger
```

### Action

```text
+ Add Action
```

### Execution

```text
Concurrency
Timeout
Retry
Missed Trigger
```

### Advanced

```text
Environment
Working Directory
Timezone
```

---

# 59. Dashboard

首页显示：

```text
Active Tasks
Running Tasks
Next Scheduled Runs
Failed Runs
Agent Status
```

例如：

```text
Next Run
Backup         Today 18:00
Cleanup        Today 23:00
Sync           Tomorrow 08:30
```

---

# 60. Task List

列表：

```text
Status
Name
Trigger
Next Run
Last Run
Last Result
```

支持：

```text
Run Now
Enable
Disable
Edit
Duplicate
Delete
```

---

# 61. History

支持：

```text
All Runs
Succeeded
Failed
TimedOut
Cancelled
Skipped
Interrupted
```

过滤：

```text
Task
Date Range
Status
```

---

# 62. Execution Detail

显示：

```text
Task
Trigger
Start
Finish
Duration
Exit Code
Status
```

实时输出：

```text
STDOUT
STDERR
```

如果正在执行：

```text
Stop
```

---

# 63. 错误处理模型

分为：

### Configuration Error

```text
Script not found
Invalid cron
Invalid timezone
```

### Runtime Error

```text
Process failed
Permission denied
Executable missing
```

### Infrastructure Error

```text
Database failure
IPC disconnected
Agent crash
```

错误需要分类，不要全部显示：

```text
Unknown error
```

---

# 64. 配置校验

任务保存前必须验证：

```text
Trigger valid
Action valid
Executable exists
Working directory valid
Timezone valid
Cron valid
```

但：

> 不建议要求执行文件必须在保存瞬间存在。

例如：

```text
backup.exe
```

可能以后才安装。

因此：

```text
path invalid
```

可以：

```text
warning
```

真正执行时再报：

```text
ExecutableNotFound
```

---

# 65. Security

这是一个可以执行任意系统命令的软件，因此必须明确安全边界。

UI / Agent IPC 不得允许：

```text
任意客户端直接执行 command
```

IPC 必须要求：

```text
authenticated local client
```

---

# 66. IPC 安全

Agent 启动时生成：

```text
random auth token
```

保存于：

```text
per-user runtime directory
```

Tauri 启动 Agent 后拿到 token。

Agent IPC 请求必须：

```text
authenticate
```

成功以后才允许：

```text
task.*
execution.*
settings.*
```

---

# 67. 权限原则

默认：

```text
Run as current user
```

不要默认：

```text
Administrator
root
```

如果未来支持提权执行：

```text
Explicit user consent
+
OS privilege prompt
```

不能静默提权。

---

# 68. Shell Injection

对于直接程序执行：

```text
program
args[]
```

禁止：

```text
拼接字符串
```

例如：

```text
program = "foo.exe"
args = ["--name", userInput]
```

而不是：

```text
"foo.exe --name " + userInput
```

Shell Action 则明确标记：

```text
This command will be interpreted by shell.
```

---

# 69. 脚本安全提示

任务编辑器明确提示：

> 任务可执行本机用户权限下的任意代码，请仅运行可信脚本和程序。

---

# 70. Task Versioning

Task 每次修改：

```text
version += 1
```

Scheduler 收到：

```text
TaskUpdated
```

以后：

```text
invalidate old queue entry
```

可以通过：

```text
generation
```

避免旧队列元素触发。

---

# 71. Scheduler Generation

队列元素：

```text
task_id
trigger_id
generation
next_fire_at
```

Task 修改后：

```text
generation++
```

旧元素即使还在 heap 中：

```text
generation mismatch
```

直接丢弃。

这样无需频繁从 priority queue 删除任意中间节点。

---

# 72. Dynamic Scheduling

以下操作必须实时生效：

```text
Enable Task
Disable Task
Edit Trigger
Delete Task
Manual Run
```

不需要重启 Agent。

---

# 73. Manual Run

手动执行：

```text
User clicks Run Now
      ↓
Agent validates task
      ↓
ExecutionManager
      ↓
Process
```

注意：

```text
Manual Run
```

不应修改：

```text
next scheduled time
```

除非用户明确选择：

```text
Treat manual run as scheduled occurrence
```

V1 默认不修改。

---

# 74. Retry

支持：

```text
max_attempts
backoff
retry_on
```

例如：

```text
3 attempts
30s
60s
120s
```

retry 条件：

```text
ProcessExitNonZero
Timeout
```

配置错误：

```text
ExecutableNotFound
```

默认不重试。

---

# 75. Execution State Machine

```text
Queued
  │
  ▼
Starting
  │
  ▼
Running
  ├──► Succeeded
  ├──► Failed
  ├──► TimedOut
  ├──► Cancelled
  └──► Interrupted
```

Scheduler 产生：

```text
Queued
```

Executor 负责后续状态。

---

# 76. Event Bus

Agent 内部使用：

```text
Event Bus
```

事件：

```text
TaskCreated
TaskUpdated
TaskDeleted

ExecutionQueued
ExecutionStarted
ExecutionOutput
ExecutionFinished

AgentStarted
AgentStopping
AgentStopped
```

UI 和 Logger 都可以订阅。

这样避免：

```text
Scheduler → UI
Scheduler → Database
Scheduler → Logger
Scheduler → IPC
```

全部强耦合。

---

# 77. Repository Pattern

业务层不要直接 SQL。

使用：

```text
TaskRepository
TriggerRepository
ActionRepository
ExecutionRepository
SettingsRepository
```

Scheduler 依赖：

```text
TaskRepository
```

而不是：

```text
SQLiteConnection
```

便于单元测试。

---

# 78. Transaction

例如编辑任务：

```text
BEGIN
  update task
  delete old triggers
  insert triggers
  delete old actions
  insert actions
COMMIT
```

Commit 成功以后：

```text
publish TaskUpdated
```

不要在 transaction 尚未成功时通知 Scheduler。

---

# 79. 数据迁移

每个 schema 变化：

```text
migration N
```

启动：

```text
backup DB
↓
run migration
↓
verify schema
↓
start agent
```

对桌面应用而言，数据库迁移失败时必须：

```text
stop agent
show repair UI
```

不要在不完整 schema 上继续运行。

---

# 80. 数据备份

建议支持：

```text
Export tasks
Import tasks
Backup database
```

推荐导出格式：

```text
JSON
```

而不是直接暴露 SQLite 文件作为唯一迁移格式。

---

# 81. 导入导出

导出应包含：

```text
task
triggers
actions
policies
```

不包含：

```text
execution history
stdout
stderr
```

除非用户选择：

```text
Full Backup
```

---

# 82. Platform Adapter

定义统一能力：

```text
Platform
├── process
├── shell
├── startup
├── lifecycle events
├── process tree
└── environment
```

Windows：

```text
WindowsPlatform
```

macOS：

```text
MacOSPlatform
```

业务层不出现：

```text
#[cfg(target_os = "windows")]
```

除非在 platform crate 内部。

---

# 83. Platform Boundary

推荐：

```text
domain
scheduler
executor
     │
     ▼
Platform trait
     │
 ┌───┴────┐
 ▼        ▼
Windows   macOS
```

这样未来增加 Linux：

```text
LinuxPlatform
```

不会重写业务核心。

---

# 84. Agent 不依赖 UI

Agent 应能：

```text
cargo run --bin agent
```

独立运行。

因此测试可以：

```text
Agent
+
SQLite
+
fake platform
```

完全不打开 Tauri。

---

# 85. Test Architecture

测试分四层。

## Unit Test

测试：

```text
Trigger calculation
Cron
Timezone
DST
Missed run
Retry
Concurrency
```

## Integration Test

测试：

```text
SQLite
Repository
Scheduler
Executor
IPC
```

## Platform Test

Windows/macOS 分别测试：

```text
process creation
shell execution
kill tree
startup
```

## E2E

测试：

```text
User
 ↓
Vue
 ↓
Tauri
 ↓
Agent
 ↓
Process
 ↓
History
```

---

# 86. 时间相关测试

这是最重要的测试模块之一。

必须覆盖：

```text
正常日期
月末
闰年
DST 开始
DST 结束
timezone change
clock backward
clock forward
sleep/wake
```

Tokio 提供可控时间测试能力，可通过 paused time / advance 等机制测试异步 timer，而不必让测试真实等待数小时。 citeturn410358search9

---

# 87. Scheduler Test Clock

不要让 Scheduler 直接调用：

```text
SystemTime::now()
```

业务层应该依赖：

```text
Clock
```

实现：

```text
SystemClock
FakeClock
```

测试：

```text
FakeClock.advance(...)
```

这样可以快速模拟：

```text
1 year
```

---

# 88. Execution Test Double

定义：

```text
ProcessRunner
```

测试时替换成：

```text
FakeProcessRunner
```

这样可以测试：

```text
timeout
retry
failure
concurrency
```

而不需要真的运行 PowerShell。

---

# 89. Observability

Agent 使用：

```text
tracing
```

日志字段建议：

```text
timestamp
level
component
task_id
execution_id
message
```

例如：

```text
INFO scheduler task_id=abc123 next_run=...
INFO execution id=run456 status=started
ERROR execution id=run456 exit_code=1
```

---

# 90. 性能目标

V1 建议目标：

```text
10,000 tasks
```

正常状态下：

```text
CPU ≈ idle
```

因为 Scheduler 只等待最近一个 deadline。

内存：

```text
与任务数量近似线性
```

执行日志不应长期无限增长。

---

# 91. 推荐的调度性能模型

不要：

```text
10,000 tasks
×
every second
```

应该：

```text
10,000 tasks
      ↓
heap
      ↓
next task only
      ↓
sleep
```

插入/更新任务：

```text
O(log N)
```

取最近任务：

```text
O(log N)
```

---

# 92. Shutdown

正常关闭：

```text
Stop accepting new executions
        ↓
mark AgentStopping
        ↓
wait running executions
        ↓
persist runtime info
        ↓
close IPC
        ↓
close DB
        ↓
exit
```

但如果用户选择：

```text
Force Quit
```

则：

```text
kill
```

并让下次启动识别：

```text
Interrupted executions
```

---

# 93. Single Instance

Tauri Desktop：

```text
single instance
```

Agent：

```text
single instance
```

启动第二个 Agent 时：

```text
connect existing Agent
```

而不是创建第二个 Scheduler。

否则会出现：

```text
same task
↓
two agents
↓
execute twice
```

这是必须避免的问题。

---

# 94. File Lock

建议 Agent 使用：

```text
agent.lock
```

或平台等价机制。

如果锁已存在：

```text
try IPC existing agent
```

如果能够连接：

```text
reuse
```

否则：

```text
stale lock recovery
```

---

# 95. Crash Recovery

Agent 每次启动：

```text
read agent state
```

如果发现：

```text
previous session did not shutdown cleanly
```

则：

```text
scan Running executions
```

全部：

```text
Interrupted
```

随后：

```text
rebuild scheduler
```

---

# 96. 更新机制

更新时不能同时替换正在运行的 Agent 可执行文件。

推荐：

```text
Updater
   ↓
download
   ↓
verify signature
   ↓
stop agent
   ↓
replace binaries
   ↓
restart agent
```

更新过程中：

```text
new executions not accepted
```

---

# 97. Installer

Windows：

```text
MSI / NSIS
```

macOS：

```text
.app
.dmg
```

应用内包含：

```text
Desktop
Agent
```

Agent 应被视为一个明确的产品组件，而不是临时脚本。

---

# 98. Code Signing

正式发布必须：

```text
Windows code signing
macOS Developer ID
macOS notarization
```

否则系统可能：

```text
Gatekeeper
SmartScreen
```

产生额外提示。

---

# 99. 配置目录

推荐把：

```text
Database
Config
Logs
Runtime
```

分开。

概念结构：

```text
App Data
├── database/
├── config/
├── logs/
├── runtime/
└── backups/
```

不要把这些文件放在安装目录。

---

# 100. 用户数据

任务数据默认属于：

```text
Current User
```

因此不同用户：

```text
Alice
```

和：

```text
Bob
```

使用不同数据库、不同 Agent。

V1 不支持跨用户共享任务。

---

# 101. Scheduler 与 UI 的核心原则

UI 可以：

```text
pause
resume
run now
enable
disable
edit
```

但是 UI 不拥有调度权。

真正的权威状态：

```text
Agent
```

因此：

```text
Vue says: enable task
         ↓
Agent validates
         ↓
DB commits
         ↓
Scheduler receives update
```

而不是：

```text
Vue local state = scheduler truth
```

---

# 102. MVP

建议第一版只实现：

### Trigger

```text
Once
Daily
Weekly
Interval
AgentStarted
```

### Action

```text
Program
CMD
PowerShell
Shell Script
```

### Execution

```text
stdout/stderr
exit code
timeout
cancel
retry
```

### Policy

```text
SkipIfRunning
AllowParallel
RunOnce when missed
```

### UI

```text
Task List
Task Editor
Execution History
Execution Detail
Settings
Tray
```

---

# 103. V1.1

增加：

```text
Monthly
Cron
Timezone selector
DST options
QueueOne
Import/Export
```

---

# 104. V1.2

增加：

```text
FileChanged
ProcessStarted
SystemWake
NetworkChanged
```

---

# 105. V2

增加：

```text
Action Chain
Conditional execution
OnSuccess / OnFailure
Task dependency
Variables
Secret store
Advanced expressions
```

---

# 106. 暂时不做

建议明确禁止 MVP 膨胀：

```text
Cloud sync
Remote management
Team collaboration
Web server
Distributed scheduler
Plugin marketplace
```

先把：

```text
scheduler
executor
recovery
cross-platform
```

做稳定。

---

# 107. 最关键的架构决策汇总

| 决策 | 选择 |
|---|---|
| Desktop framework | Tauri 2 |
| Frontend | Vue 3 + TypeScript |
| Backend | Rust |
| Runtime | Tokio |
| Database | SQLite |
| Scheduler | 自研 |
| Windows Task Scheduler | 不使用 |
| macOS launchd | 不使用 |
| Trigger engine | 自研 |
| Process execution | Rust |
| UI IPC | Tauri Commands + Events |
| Agent IPC | Local IPC |
| Time calculation | Wall Clock |
| Waiting | Monotonic Timer |
| Queue | Priority Queue |
| Task state | SQLite |
| UI cache | Pinia |
| Logs | tracing |
| Process concurrency | Per-task + Global |
| Recovery | Agent startup rebuild |
| UI close | Agent continues |
| Full quit | Configurable |
| Privilege | Current user by default |

---

# 108. 最终推荐的核心模块关系

```text
                    ┌────────────────┐
                    │   Vue 3 UI     │
                    └───────┬────────┘
                            │
                     Tauri Commands
                            │
                            ▼
                    ┌────────────────┐
                    │ Tauri Core     │
                    └───────┬────────┘
                            │
                         Local IPC
                            │
                            ▼
┌────────────────────────────────────────────────┐
│                    Rust Agent                  │
│                                                │
│  ┌────────────┐       ┌──────────────────┐     │
│  │ Repository │───────►│ Trigger Engine   │     │
│  └────────────┘       └────────┬─────────┘     │
│                                │               │
│                                ▼               │
│                       ┌──────────────────┐      │
│                       │ Scheduler        │      │
│                       └────────┬─────────┘      │
│                                │               │
│                                ▼               │
│                       ┌──────────────────┐      │
│                       │ ExecutionManager │      │
│                       └────────┬─────────┘      │
│                                │               │
│                                ▼               │
│                       ┌──────────────────┐      │
│                       │ ProcessManager   │      │
│                       └────────┬─────────┘      │
│                                │               │
└────────────────────────────────┼───────────────┘
                                 │
                     ┌───────────┴───────────┐
                     ▼                       ▼
              Windows Adapter          macOS Adapter
```

---

# 109. 最终产品运行模型

整个产品最终应该呈现为：

```text
用户登录
   ↓
Tauri App 启动
   ↓
Tray
   ↓
启动 Rust Agent
   ↓
Agent 加载 SQLite
   ↓
验证并恢复任务
   ↓
计算所有 next_fire_at
   ↓
建立 Priority Queue
   ↓
等待最近触发点
   ↓
触发 Task
   ↓
Execution Manager
   ↓
Process Manager
   ↓
CMD / PowerShell / Shell / Program
   ↓
收集输出
   ↓
记录 Execution
   ↓
计算下一次触发
   ↓
重新进入 Scheduler
```

核心原则只有一句：

> **操作系统负责提供进程和用户会话；本产品自己负责“什么时候运行什么任务”。**

---

# 110. 结论

本项目最重要的技术资产不是 Tauri UI，而是：

```text
Trigger Engine
+
Scheduler
+
Execution Manager
+
Process Manager
+
Recovery
```

因此开发顺序不应该是：

```text
先把 UI 做漂亮
```

而应该是：

```text
Domain Model
    ↓
Trigger Engine
    ↓
Scheduler
    ↓
Execution Manager
    ↓
SQLite Persistence
    ↓
Agent IPC
    ↓
Tauri
    ↓
Vue UI
```

这样即使未来：

```text
Vue 改成 React
Tauri 改成其他 GUI
增加 Linux
增加 Web UI
```

核心调度引擎仍然可以保留。

---

## 参考

- Tauri 2 IPC：Commands 用于请求/响应，Events 用于状态和生命周期通知。Tauri 官方文档：
  https://v2.tauri.app/concept/inter-process-communication/
- Tokio Timer：`sleep`、`sleep_until`、`interval` 以及可测试时间控制能力：
  https://docs.rs/tokio/latest/tokio/time/
