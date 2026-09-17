# easyJob 原生应用身份通知与生命周期协同设计规范

**日期**: 2026-09-17  
**状态**: Approved (用户已确认)  
**作者**: easyJob 核心架构组  

---

## 1. 背景与问题分析

### 1.1 问题现状
1. **通知身份与图标错乱**：
   在 macOS 上，任务执行完成后的通知显示为“脚本编辑器（Script Editor）”，图标为黄色卷轴，而非 easyJob 的闪电表盘图标。
   - **根因**：底层通过 `osascript -e 'display notification ...'` 执行，macOS 通知中心根据调用方可执行程序（`/usr/bin/osascript`）将通知身份判定为系统的“脚本编辑器”。
2. **通知点击无响应**：
   AppleScript 弹出的通知为无状态单向通知，点击无法唤醒 easyJob，更无法自动跳转至“执行记录”页面。
3. **应用生命周期未协同**：
   - 之前在退出应用（Dock 栏右键退出、菜单栏托盘退出）时，后台守护进程未联动退出，导致常驻老进程引发版本不同步。
   - 用户明确界定生命周期规则：
     - **点击界面左上角关闭按钮 (X)**：仅关闭/隐藏前端主窗口，系统菜单栏托盘图标与后台守护进程保持常驻运行与调度。
     - **Dock 栏右键“退出” / 菜单栏托盘点击“退出 easyJob” / Cmd+Q**：桌面端主应用退出，**后台常驻守护进程协同退出**。

---

## 2. 核心架构设计

### 2.1 通知发出主体与原生身份 (`easyJob.app`)

- **架构优化**：
  - 由桌面主应用 `easyjob-desktop`（拥有正式 Bundle ID `com.easyjob.desktop`、应用程序名 `easyJob`、以及 `icon.icns` 图标）作为原生通知发出者。
  - 在 `apps/desktop/src-tauri` 中集成官方 `tauri-plugin-notification = "2"`。
  - 当后台 `easyjob-agent` 完成任务执行时，通过 IPC 广播 `"execution.finished"` 事件（携带 `task_name`、`status`、`duration_ms`、`error_message`、`notification_policy`）。
  - 桌面主应用在 `events.rs` 中接收该事件，根据通知策略通过 `app_handle.notification().builder()` 发送原生系统通知。
  - **效果**：
    - macOS：通知中心来源严格标识为 **easyJob**，图标自动展示为 **easyJob 原生超椭圆闪电表盘图标**。
    - Windows：Toast 通知来源严格标识为 **easyJob**，展示应用图标。

### 2.2 点击通知自动唤醒主窗口并跳转至执行记录

- **唤醒与导航流**：
  1. 用户在 macOS 或 Windows 上点击通知横幅。
  2. 操作系统自动激活唤起 `easyJob.app` 宿主进程。
  3. 后端拦截应用激活与唤醒事件：
     - macOS 拦截 `tauri::RunEvent::Reopen`：
       - 调用 `set_dock_visible(true)` 恢复 Dock 栏显示；
       - 主窗口 `window.show()` + `window.set_focus()` 置顶前台；
       - 通过 Webview 广播 `navigate: 'executions'`。
     - 配合通知点击时间戳跟踪（若最近 30 秒内触发过通知并在唤醒时命中，自动导航）。
  4. 前端 `App.vue` 监听 `navigate` 事件：
     - 自动将 `currentView` 响应式变更为 `'executions'`；
     - 调用 `executionStore.loadExecutions()` 自动刷新最新执行记录。

### 2.3 统一应用生命周期协同

```text
+-------------------+      Close Button (X)      +-----------------------------+
|   easyJob 主窗口   | ------------------------> | 窗口隐藏 (hide)              |
|   (前台可见)      |                           | 托盘常驻 + Agent 守护常驻     |
+-------------------+                           +-----------------------------+
          |
          | Dock 右键“退出” / Cmd+Q / 托盘菜单“退出 easyJob”
          v
+-------------------------------------------------------------+
| Tauri RunEvent::ExitRequested                               |
| 1. 向 Agent 发送 agent.shutdown RPC 请求                     |
| 2. 等待 Agent 释放 lock 文件并终止                           |
| 3. 桌面客户端彻底退出 (exit 0)                                |
+-------------------------------------------------------------+
```

1. **窗口关闭按钮拦截 (`WindowEvent::CloseRequested`)**：
   - 保持现状：调用 `api.prevent_close()`，仅 `window.hide()` 并 `set_dock_visible(false)`。托盘与 Agent 均不退出。
2. **应用显式退出拦截 (`RunEvent::ExitRequested`)**：
   - 在应用退出前，通过 `AgentManager` 向 Agent 发送 `agent.shutdown` 命令，等待 150ms 确保 Agent 优雅释放文件锁并自杀。
   - 托盘菜单的 `"quit"` 直接调用 `app.exit(0)`，触发相同的 `ExitRequested` 逻辑。

---

## 3. 详细实现规范

### 3.1 桌面端后端 (`apps/desktop/src-tauri`)

1. **Cargo.toml 与权限配置**：
   - 添加 `tauri-plugin-notification = "2"`。
   - `capabilities/default.json` 添加 `"notification:default"`。
2. **`lib.rs` 插件注册与生命周期重构**：
   - 注册 `.plugin(tauri_plugin_notification::init())`。
   - 将 `.run(tauri::generate_context!())` 升级为 `.build().expect().run(|app_handle, event| { ... })`。
   - 处理 `RunEvent::ExitRequested`：调用 `agent.shutdown`。
   - 处理 `RunEvent::Reopen`：显示窗口、聚焦、发出 `navigate: executions`。
3. **`events.rs` 原生通知派发**：
   - 监听 `execution.finished` 事件。
   - 读取 `notification_policy` 并判断是否触发。
   - 使用 `app_handle.notification().builder()` 构建并发送原生通知。
4. **`apps/agent/src/service.rs` IPC 事件负载扩充**：
   - 在 `execution.finished` 中丰富属性：`task_name`、`duration_ms`、`error_message`、`notification_policy`。

### 3.2 桌面端前端 (`apps/desktop/src`)

1. **`App.vue` 导航事件监听**：
   - 监听 Tauri 事件 `'navigate'`。
   - 接收到 `'executions'` 时：
     ```typescript
     currentView.value = 'executions';
     executionStore.loadExecutions();
     ```

---

## 4. 验证与测试标准

1. **构建与类型验证**：
   - `cargo check -p easyjob-desktop` 无报错。
   - `cargo test --all` 100% 通过。
   - `cargo clippy --workspace --all-targets -- -D warnings` 0 警告。
   - `pnpm -C apps/desktop test` 100% 通过。
   - `pnpm -C apps/desktop run build` 0 错误。
2. **功能验证**：
   - macOS 通知展示的标题为 `easyJob`，图标为闪电表盘 Squircle 图标（非脚本编辑器）。
   - 点击通知自动唤起主窗口并进入“执行记录”页面。
   - 点击窗口左上角关闭按钮，托盘和后台 Agent 继续运行。
   - Dock 栏右键退出或托盘菜单退出时，桌面与后台 Agent 同步退出。
