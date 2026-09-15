# easyJob 桌面端系统设置增强、开机自启与新应用图标设计规范

## 1. 概述与背景

本文档定义 easyJob 桌面端（Tauri 2 + Vue 3）的三项体验增强与外观重构功能：
1. **系统开机自启（默认开启）**：集成官方 `tauri-plugin-autostart` 插件。在 Windows 下必须使用注册表（`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`）方式实现，严禁使用任务计划程序（Task Scheduler）；自启时支持静默常驻系统托盘（`--minimized`）。
2. **系统设置页面布局优化（横向展示）**：将 Agent 守护进程状态参数修改为横向并列布局（`参数名: 参数值`），不再使用上下垂直堆叠展示。
3. **全新品牌应用程序图标设计与资源替换**：设计现代流线时钟与执行闪电图标（深蓝至电光紫微拟物渐变），替换桌面端全套图标、浏览器 favicon 与侧边栏 Logo。

---

## 2. 架构与详细设计

### 2.1 开机自启机制 (`tauri-plugin-autostart`)

#### 跨平台底层实现原理
- **Windows**：
  `tauri-plugin-autostart` 底层依赖 `auto-launch` 库，在 Windows 目标平台上使用 Rust 原生 `winreg` 库直接读写注册表项：
  `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`
  注册键名为 `easyJob`，注册值格式为 `"<exe_path>" --minimized`。绝不调用 Windows 任务计划程序（`schtasks`），无多余服务或权限申请，保持注册表纯净。
- **macOS**：
  采用 `MacosLauncher::LaunchAgent`，写入 `~/Library/LaunchAgents/com.easyjob.desktop.plist`，携带 `--minimized` 启动参数。
- **Linux**：
  写入 `~/.config/autostart/easyjob.desktop`，`Exec="<exe_path>" --minimized`。

#### Tauri 2 原生配置
1. `apps/desktop/src-tauri/Cargo.toml`：
   添加依赖 `tauri-plugin-autostart = "2"`。
2. `apps/desktop/src-tauri/capabilities/default.json`：
   在 permissions 列表中添加 `"autostart:default"`。
3. `apps/desktop/src-tauri/src/lib.rs`：
   - 注册插件：
     ```rust
     .plugin(tauri_plugin_autostart::init(
         tauri_plugin_autostart::MacosLauncher::LaunchAgent,
         Some(vec!["--minimized"]),
     ))
     ```
   - 在 `setup` 闭包中检测命令行参数：
     ```rust
     let args: Vec<String> = std::env::args().collect();
     if args.iter().any(|arg| arg == "--minimized") {
         if let Some(window) = app.get_webview_window("main") {
             let _ = window.hide();
         }
     }
     ```
   - 保证自启时窗口处于隐藏状态，系统托盘图标常驻后台。

#### 前端自启交互与默认开启策略
- 依赖 `@tauri-apps/plugin-autostart` 2.x。
- **首次运行默认开启**：
  在应用启动或进入设置页面时，检查本地存储 `localStorage.getItem('easyjob_autostart_init')`：
  - 若为首次启动（未初始化），自动调用 `enable()` 并标记 `easyjob_autostart_init = 'true'`，同时自启开关显示为开启状态。
  - 用户手动切换开关时，调用 `enable()` / `disable()` 并给出操作反馈。

---

### 2.2 系统设置界面横向展示重构 (`SettingsView.vue`)

#### 布局调整方案
原界面采用上下结构展示参数名与状态值，现调整为横向键值行布局（`label-placement="left"` 配合水平排列）：
- **展示项**：
  1. `连接状态`：`🟢 运行中 (Connected)` / `🔴 未连接 (Disconnected)`
  2. `Agent 版本`：`v0.1.0`
  3. `运行时长`：`120 s`
  4. `当前活跃任务`：`3 个`
  5. `并发执行中`：`0 个`
- 视觉风格：参数名保持次要文字颜色（`text-slate-500 dark:text-zinc-400`），参数值紧随其后以主要字重高亮呈现。

#### 新增“启动与系统偏好”卡片
- 放置于 Agent 状态卡片下方或上方。
- 包含“开机自启动”项：
  - 左侧标题：“开机自启动”
  - 副标题：“开机时在后台静默启动 easyJob 并最小化到系统托盘”
  - 右侧：“开启/关闭”开关（`NSwitch`），支持加载中状态。

---

### 2.3 应用程序图标设计与全套资源替换

#### 视觉设计方案
- **设计主题**：极简流线时钟 + 任务执行闪电箭头。
- **色彩方案**：
  - 背景：深蓝至电光紫微拟物圆角矩形渐变（`#0F172A` -> `#312E81` -> `#581C87`）。
  - 核心图形：高光流线时钟刻度轮廓，内部贯穿一道充满速度感的电光青蓝闪电箭头（`#38BDF8` 至 `#818CF8`）。
- **资源导出与替换清单**：
  1. `apps/desktop/src-tauri/icons/icon.png` (512x512 高清原图)
  2. `apps/desktop/src-tauri/icons/128x128.png` (128x128)
  3. `apps/desktop/src-tauri/icons/32x32.png` (32x32)
  4. `apps/desktop/public/favicon.svg` (矢量图标，用于 Web 端与浏览器标签)
  5. `apps/desktop/src/components/layout/AppSidebar.vue` (侧边栏头部 Logo 替换为新图标流线图形)

---

## 3. 验证方案

1. **自动化测试**：
   - 运行前端单元测试 `pnpm -C apps/desktop test`（覆盖设置界面与自启辅助逻辑）。
   - 运行全工作区 Rust 单元测试 `cargo test --all`。
2. **代码质量与静态检查**：
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo fmt --check`
   - `vue-tsc --noEmit`
3. **打包与功能验证**：
   - 验证 `pnpm -C apps/desktop run build`。
   - 验证 `--minimized` 启动参数下主窗口静默隐藏与托盘显示。
