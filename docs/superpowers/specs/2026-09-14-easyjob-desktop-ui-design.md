# easyJob Phase 3: Tauri 2 + Vue 3 桌面端架构与界面设计规范

## 1. 概述与设计目标

本文档规范 easyJob **第三阶段：桌面端界面开发（Phase 3: Tauri 2 + Vue 3 Desktop UI）** 的架构、通信协议、组件设计及工程结构。

### 核心原则
- **Agent 负责调度、执行与状态核心**：Tauri 2 客户端不承担任何定时器轮询和进程执行逻辑；所有核心操作均通过本地 IPC (`easyjob-ipc`) 委托给后台守护进程 `easyjob-agent`。
- **WebView 崩溃或刷新零影响**：Tauri 窗口关闭或刷新绝不影响后台守护进程的定时调度与执行。
- **本地安全**：所有 IPC 严格使用本地套接字（Unix Socket `0600` / Windows Named Pipe），不开放任何外部网络端口。
- **流畅直观的桌面体验**：基于 Naive UI 与 Tailwind CSS 构建，提供抽屉式任务编辑、无需手写 Cron 的可视化触发器配置、以及深色终端风格的实时流式日志监控。

---

## 2. 系统拓扑与架构

```text
┌─────────────────────────────────────────────────────────────┐
│                 Vue 3 Frontend (src/)                       │
│    Naive UI + Tailwind CSS + Pinia + Lucide Icons           │
│                                                             │
│   [Views]               [Stores]             [Services]     │
│   - TasksView           - taskStore          - tauri.ts     │
│   - ExecutionsView      - executionStore     - events.ts    │
│   - SettingsView        - agentStore                        │
└──────────────────────────────▲──────────────────────────────┘
                               │ Tauri IPC (invoke / listen)
┌──────────────────────────────▼──────────────────────────────┐
│             Tauri 2 Native Core (src-tauri/)                │
│                                                             │
│  - commands.rs:       Tauri Commands 代理转发               │
│  - events.rs:         Agent 广播事件订阅并中继给前端        │
│  - agent_manager.rs:  Agent 守护进程探活与自拉起            │
│  - tray.rs:           系统托盘常驻与窗口 Hide 拦截          │
└──────────────────────────────▲──────────────────────────────┘
                               │ Local OS IPC (easyjob-ipc)
                               │ Unix Socket / Windows Named Pipe
┌──────────────────────────────▼──────────────────────────────┐
│               easyjob-agent (后台守护进程)                  │
│                                                             │
│  - Scheduler (Min-Heap) + ExecutionManager (信号量限流)     │
│  - ProcessRunner (静默进程组隔离) + Sqlite WAL Database     │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. 工程与目录结构

Tauri 2 桌面端工程置于 `apps/desktop`：

```text
easyJob/
├── Cargo.toml                                 # Workspace 根配置，包含 apps/desktop/src-tauri
├── apps/
│   ├── agent/                                 # 后台守护进程
│   └── desktop/                               # 桌面端应用根目录
│       ├── package.json                       # 前端依赖配置
│       ├── pnpm-lock.yaml                     # pnpm 锁定依赖
│       ├── tsconfig.json                      # TypeScript 编译配置
│       ├── vite.config.ts                     # Vite 6 构建配置
│       ├── tailwind.config.js & postcss       # Tailwind 样式配置
│       ├── index.html                         # SPA 入口 HTML
│       ├── src/                               # Vue 3 源码目录
│       │   ├── main.ts                        # Vue 挂载与插件初始化
│       │   ├── App.vue                        # 顶层布局容器与 Naive UI 主题配置
│       │   ├── assets/                        # 样式表与通用静态资产
│       │   ├── types/                         # 强类型定义
│       │   │   ├── task.ts                    # Task, Trigger, Action, Policy
│       │   │   ├── execution.ts               # Execution, ExecutionStatus
│       │   │   └── agent.ts                   # AgentStatus
│       │   ├── services/                      # Tauri API 通信层
│       │   │   ├── tauri.ts                   # invoke 包装函数
│       │   │   └── events.ts                  # listen 包装函数与事件监听器
│       │   ├── stores/                        # Pinia 状态管理
│       │   │   ├── taskStore.ts               # 任务数据及增删改查动作
│       │   │   ├── executionStore.ts          # 执行历史与实时输出流缓存
│       │   │   └── agentStore.ts              # Agent 状态与探活心跳
│       │   ├── views/                         # 核心业务视图
│       │   │   ├── TasksView.vue              # 任务列表与主操作区
│       │   │   ├── ExecutionsView.vue         # 执行历史列表
│       │   │   └── SettingsView.vue           # 状态监测与系统设置
│       │   └── components/                    # 功能组件
│       │       ├── layout/
│       │       │   └── AppSidebar.vue         # 左侧导航栏与状态徽章
│       │       ├── task/
│       │       │   ├── TaskDrawer.vue         # 任务新增/编辑抽屉
│       │       │   ├── TriggerEditor.vue      # 可视化触发器表单
│       │       │   └── ActionEditor.vue       # 执行动作与环境变量表单
│       │       └── console/
│       │           └── LiveLogDrawer.vue      # 实时流式控制台抽屉
│       └── src-tauri/                         # Tauri 2 原生工程
│           ├── Cargo.toml                     # 依赖 tauri, easyjob-ipc, etc.
│           ├── tauri.conf.json                # 窗口、托盘、权限配置
│           ├── build.rs                       # Tauri build 脚本
│           ├── icons/                         # 托盘与应用图标
│           └── src/
│               ├── main.rs                    # 桌面端程序入口
│               ├── lib.rs                     # Tauri Builder 装配与运行
│               ├── agent_manager.rs           # Agent 探活与自拉起管理
│               ├── commands.rs                # Tauri IPC 命令处理
│               ├── events.rs                  # Agent 广播事件订阅与中继
│               └── tray.rs                    # 系统托盘与常驻菜单
```

---

## 4. Tauri Rust 后端网关设计

### 4.1 Agent 守护进程生命周期管理 (`agent_manager.rs`)
- **自动探活**：应用启动时，尝试通过 `IpcClient::connect(default_ipc_path())` 连接。
- **自动自拉起**：若探测失败，定位 `easyjob-agent` 二进制：
  - 开发阶段：检索 `target/debug/easyjob-agent`；
  - 发布阶段：检索可执行文件同级目录或资源包目录。
  - 使用 `std::process::Command`（在 Windows 上附带 `CREATE_NO_WINDOW`）以后台子进程方式拉起。
  - 启动后进行最多 3 秒重试探活（指数退避）。
- **共享连接池**：在 Tauri 的 Managed State 中注入 `AppState { client: Arc<Mutex<Option<IpcClient>>> }`。

### 4.2 Tauri Commands 接口定义 (`commands.rs`)
| Tauri Command | 映射 Agent IPC 方法 | 参数 | 返回值 | 语义 |
|---|---|---|---|---|
| `get_agent_status` | `agent.status` | 无 | `AgentStatus` | 查询 Agent 版本与指标 |
| `list_tasks` | `task.list` | 无 | `Vec<Task>` | 查询所有启用/保存的任务 |
| `get_task` | `task.get` | `id: TaskId` | `Task` | 查询单个任务详情 |
| `save_task` | `task.save` | `task: Task` | `Task` | 保存/更新任务并同步调度堆 |
| `delete_task` | `task.delete` | `id: TaskId` | `bool` | 删除任务并作废调度 |
| `trigger_task` | `task.trigger_now` | `id: TaskId` | `bool` | 立即触发一次任务执行 |
| `list_executions` | `execution.list` | `limit: Option<u32>` | `Vec<Execution>` | 查询最近的执行记录 |
| `get_execution` | `execution.get` | `id: ExecutionId` | `Execution` | 查询单次执行详情 |
| `cancel_execution` | `execution.cancel` | `id: ExecutionId` | `bool` | 级联终止运行中的进程树 |
| `restart_agent` | 本地自愈控制 | 无 | `bool` | 重启 Agent 守护进程 |

### 4.3 实时事件中继 (`events.rs`)
- 后台常驻 Tokio 任务调用 `ipc_client.subscribe()`。
- 收到消息后，通过 Tauri 2 原生 `app_handle.emit(&event.event, &event.data)` 推送给所有前端窗口：
  - `execution.started`: `{ execution_id, task_id }`
  - `execution.output`: `{ execution_id, task_id, stream: "stdout"|"stderr", content }`
  - `execution.finished`: `{ execution_id, task_id, status, exit_code }`

### 4.4 系统托盘与关闭拦截 (`tray.rs`)
- 注册系统托盘图标与右键菜单（显示窗口、服务状态显示、退出系统）。
- 窗口关闭拦截：监听 `WindowEvent::CloseRequested`，调用 `api.prevent_close()` 并隐藏窗口，保持后台常驻。

---

## 5. Vue 3 前端界面与状态设计

### 5.1 页面与交互
1. **主布局 (`AppSidebar.vue`)**：
   - 顶部应用品牌与 Logo；
   - 导航菜单：任务管理 (`/tasks`)、执行记录 (`/executions`)、系统设置 (`/settings`)；
   - 底部状态微标：显示 Agent 在线（绿色）/ 离线（红色）及运行中任务计数。
2. **任务管理 (`TasksView.vue`)**：
   - 搜索与筛选器；
   - 任务卡片/列表：名称、描述、触发器摘要、启用/禁用 Switch、即时触发按钮、编辑、删除；
   - 点击“新建任务”或卡片编辑，弹出右侧全屏高度抽屉 `TaskDrawer.vue`。
3. **可视化任务与触发器抽屉 (`TaskDrawer.vue`)**：
   - **基础信息**：名称、描述、超时时间（秒）、并发控制策略（`SkipIfRunning`, `AllowParallel`, `QueueOne`）；
   - **触发器设计器 (`TriggerEditor.vue`)**：
     - 单次触发 (Once)：时间选择器；
     - 间隔触发 (Interval)：每 N 秒 / 分钟 / 小时；
     - 每日定时 (Daily)：每天指定时间 (HH:mm) 及所在时区下拉选择；
     - 每周定时 (Weekly)：多选周一至周日 + 触发时间；
     - 启动时触发 (AgentStarted)：开机自运行开关；
   - **执行动作设计器 (`ActionEditor.vue`)**：
     - 支持切换 Shell 脚本模式或可执行程序模式；
     - 工作目录选择与环境变量键值对输入。
4. **执行历史与实时监控 (`ExecutionsView.vue`)**：
   - 执行历史列表：状态标签（成功、失败、超时、被取消、运行中）、耗时（毫秒/秒）、开始与结束时间；
   - 点击任意记录唤出 `LiveLogDrawer.vue` 实时查看日志内容。
5. **实时控制台抽屉 (`LiveLogDrawer.vue`)**：
   - 纯黑终端风格背景，等宽字体，支持自动吸底滚动与暂停滚动；
   - 针对正在运行的任务，顶部提供醒目的红色“终止执行”操作（调用 `cancel_execution`）。

---

## 6. 开发任务阶段分解（SDD Tasks）

- **Task 1: Tauri 2 骨架与前端工程初始化**
  - 初始化 `apps/desktop`，安装 Vue 3, Naive UI, Tailwind CSS, Pinia, Tauri 2 依赖。
  - 配置 `Cargo.toml` 纳入工作区，打通前后端基础编译通道。
- **Task 2: Tauri Rust 桥接与 Agent 进程守护**
  - 实现 `agent_manager.rs`, `commands.rs`, `events.rs`, `tray.rs`。
  - 编写 Tauri 层面命令与事件代理测试。
- **Task 3: 前端类型模型与 Pinia 状态层**
  - 建立强类型模型，封装 `services/tauri.ts` 与 `services/events.ts`。
  - 编写 `taskStore`, `executionStore`, `agentStore`。
- **Task 4: 主界面布局与核心视图开发**
  - 实现 `App.vue`, `AppSidebar.vue`, `TasksView.vue`, `ExecutionsView.vue`, `SettingsView.vue`。
- **Task 5: 可视化任务与触发器表单抽屉**
  - 实现 `TaskDrawer.vue`, `TriggerEditor.vue`, `ActionEditor.vue`。
  - 实现无需记忆 Cron 的定时、间隔可视化表单及前端校验。
- **Task 6: 实时流式控制台与在线任务取消**
  - 实现 `LiveLogDrawer.vue`，实现流式日志接收与实时终止取消。
- **Task 7: 全端联合构建、自动化测试与整体验收**
  - 执行 `cargo test --all`、`cargo clippy`、`vue-tsc`、`pnpm build`，编写 Walkthrough 成果报告。
