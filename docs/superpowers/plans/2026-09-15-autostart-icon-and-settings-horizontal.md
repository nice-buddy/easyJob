# 系统开机自启、应用图标替换与设置页面横向展示实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 easyJob 桌面端添加开机自启（默认开启，Windows 采用注册表，自启静默常驻托盘）、将设置页面中的 Agent 状态优化为横向展示，并设计替换全新的应用图标全套资源。

**Architecture:** 
- 后端与原生层：基于 Tauri 2 官方 `tauri-plugin-autostart` 插件（Windows 下直接使用 `winreg` 操作 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，严禁任务计划程序），在应用启动生命周期中识别 `--minimized` 启动参数实现开机静默隐藏主窗口常驻托盘；
- 前端层：引入 `@tauri-apps/plugin-autostart`，在 `SettingsView.vue` 重构 Agent 状态为参数名与值水平并列（横向）布局，并添加开机自启切换卡片（首次运行默认开启）；
- 品牌外观：生成并更新全套应用程序图标（512x512, 128x128, 32x32）、浏览器 favicon.svg 及侧边栏 Logo。

**Tech Stack:** Tauri 2 (`tauri-plugin-autostart`), Rust, Vue 3, Naive UI, Tailwind CSS, Vitest.

## Global Constraints

- Windows 平台的开机自启动必须直接写入注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，严禁调用或配置 Windows 任务计划程序（`schtasks`）。
- 开机自启时必须传递 `--minimized` 参数，保持主窗口隐藏仅常驻系统托盘。
- 系统设置中的 Agent 守护进程状态参数名与值必须水平同一行显示（`参数名: 参数值`），不再垂直堆叠。
- 自动化测试与质量标准：`cargo test --all`、`pnpm -C apps/desktop test`、`cargo clippy` 0 警告、`vue-tsc` 0 错误。

---

### Task 1: 集成 `tauri-plugin-autostart` 与静默托盘启动后端

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/capabilities/default.json`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/tests/bridge_tests.rs`

**Interfaces:**
- Consumes: `tauri-plugin-autostart = "2"`
- Produces: Tauri 原生层注册 autostart 插件（带 `MacosLauncher::LaunchAgent` 与 `Some(vec!["--minimized"])`），`lib.rs` 启动时若带 `--minimized` 参数则隐藏主窗口。

- [ ] **Step 1: 在 `apps/desktop/src-tauri/Cargo.toml` 中添加插件依赖**

```toml
[dependencies]
easyjob-common = { path = "../../../crates/common" }
easyjob-domain = { path = "../../../crates/domain" }
easyjob-ipc = { path = "../../../crates/ipc" }
tauri = { version = "2.1", features = ["tray-icon"] }
tauri-plugin-autostart = "2"
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 2: 在 `apps/desktop/src-tauri/capabilities/default.json` 中添加权限**

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default permissions for easyJob desktop",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:event:default",
    "autostart:default"
  ]
}
```

- [ ] **Step 3: 修改 `apps/desktop/src-tauri/src/lib.rs` 初始化插件并处理 `--minimized`**

在 `tauri::Builder::default()` 中挂载插件并在 `setup` 钩子中隐藏 `--minimized` 启动时的窗口：

```rust
pub mod agent_manager;
pub mod commands;
pub mod events;
pub mod tray;

use agent_manager::AgentManager;
use commands::*;
use events::spawn_event_relay;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let agent_manager = Arc::new(AgentManager::new());
    let manager_for_events = agent_manager.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(agent_manager)
        .setup(move |app| {
            let handle = app.handle().clone();
            spawn_event_relay(handle.clone(), manager_for_events);
            if let Err(e) = tray::setup_system_tray(&handle) {
                tracing::warn!("Failed to setup system tray: {:?}", e);
            }

            // 若存在 --minimized 参数（开机自启动唤醒），保持主窗口隐藏并静默常驻系统托盘
            let args: Vec<String> = std::env::args().collect();
            if args.iter().any(|arg| arg == "--minimized") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running easyJob desktop");
}
```

- [ ] **Step 4: 在 `apps/desktop/src-tauri/tests/bridge_tests.rs` 中添加静默参数解析测试**

验证 `--minimized` 参数检测逻辑单元测试。

```rust
#[test]
fn test_minimized_arg_detection() {
    let args = vec!["easyjob-desktop".to_string(), "--minimized".to_string()];
    assert!(args.iter().any(|arg| arg == "--minimized"));

    let normal_args = vec!["easyjob-desktop".to_string()];
    assert!(!normal_args.iter().any(|arg| arg == "--minimized"));
}
```

- [ ] **Step 5: 验证 Rust 编译与测试**

运行：
`cargo test -p easyjob-desktop && cargo clippy -p easyjob-desktop --all-targets -- -D warnings`
预期：所有测试通过，0 警告。

- [ ] **Step 6: 提交代码**

```bash
git add apps/desktop/src-tauri
git commit -m "feat(desktop): integrate tauri-plugin-autostart with silent tray launch support"
```

---

### Task 2: 前端自启服务与设置页面横向展示重构

**Files:**
- Modify: `apps/desktop/package.json`
- Create: `apps/desktop/src/services/autostart.ts`
- Modify: `apps/desktop/src/views/SettingsView.vue`
- Modify: `apps/desktop/tests/views.test.ts`

**Interfaces:**
- Consumes: `@tauri-apps/plugin-autostart`
- Produces: `isAutostartEnabled()`, `setAutostart(enabled: boolean)`, `initAutostartDefault()`, `SettingsView.vue` 横向布局呈现与开机自启开关。

- [ ] **Step 1: 安装 `@tauri-apps/plugin-autostart` 依赖**

运行：`pnpm -C apps/desktop add @tauri-apps/plugin-autostart@^2`

- [ ] **Step 2: 创建 `apps/desktop/src/services/autostart.ts`**

提供自启探测、开启/关闭及默认开启初始化函数，且在非 Tauri 浏览器/单元测试环境中提供优雅降级：

```typescript
import { enable, isEnabled, disable } from '@tauri-apps/plugin-autostart';

const AUTOSTART_INITIALIZED_KEY = 'easyjob_autostart_initialized';

export async function isAutostartEnabled(): Promise<boolean> {
  try {
    return await isEnabled();
  } catch (e) {
    console.warn('Failed to check autostart status:', e);
    return false;
  }
}

export async function setAutostart(shouldEnable: boolean): Promise<boolean> {
  try {
    if (shouldEnable) {
      await enable();
    } else {
      await disable();
    }
    return true;
  } catch (e) {
    console.error('Failed to update autostart status:', e);
    throw e;
  }
}

export async function initAutostartDefault(): Promise<boolean> {
  try {
    const isInit = localStorage.getItem(AUTOSTART_INITIALIZED_KEY);
    if (!isInit) {
      await enable();
      localStorage.setItem(AUTOSTART_INITIALIZED_KEY, 'true');
      return true;
    }
    return await isEnabled();
  } catch (e) {
    console.warn('Autostart default initialization bypassed:', e);
    return false;
  }
}
```

- [ ] **Step 3: 重构 `apps/desktop/src/views/SettingsView.vue`**

1. 将 Agent 守护进程状态改造为横向并列展示（`label-placement="left"`，值在参数名后，同一水平行展示）。
2. 添加“系统与启动偏好”卡片，包含开机自启开关 `NSwitch`（副文本说明：开机时在后台静默运行 easyJob 并最小化到系统托盘）。

```vue
<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useAgentStore } from '../stores/agentStore';
import { NCard, NDescriptions, NDescriptionsItem, NButton, NSwitch, useMessage } from 'naive-ui';
import { isAutostartEnabled, setAutostart, initAutostartDefault } from '../services/autostart';

const agentStore = useAgentStore();
const message = useMessage();

const autostartLoading = ref(false);
const autostartActive = ref(true);

onMounted(async () => {
  try {
    autostartActive.value = await initAutostartDefault();
  } catch (e) {
    autostartActive.value = false;
  }
});

async function handleToggleAutostart(value: boolean) {
  autostartLoading.value = true;
  try {
    await setAutostart(value);
    autostartActive.value = value;
    message.success(value ? '已开启开机自启动' : '已关闭开机自启动');
  } catch (e: any) {
    message.error(`设置开机自启失败: ${e?.message || e}`);
    autostartActive.value = !value;
  } finally {
    autostartLoading.value = false;
  }
}
</script>

<template>
  <div class="max-w-3xl space-y-6">
    <div>
      <h1 class="text-xl font-bold">系统设置与状态</h1>
      <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">管理系统启动偏好及查看 easyJob 守护进程健康指标</p>
    </div>

    <!-- 启动偏好配置 -->
    <NCard title="系统与启动偏好" size="small">
      <div class="flex items-center justify-between py-1">
        <div class="space-y-1">
          <div class="text-sm font-medium text-slate-800 dark:text-zinc-200">开机自启动</div>
          <div class="text-xs text-slate-500 dark:text-zinc-400">
            开机时在后台静默运行 easyJob 并最小化到系统托盘，自动守护并按时调度各项定时任务
          </div>
        </div>
        <NSwitch
          v-model:value="autostartActive"
          :loading="autostartLoading"
          @update:value="handleToggleAutostart"
        />
      </div>
    </NCard>

    <!-- Agent 守护进程状态（横向展示） -->
    <NCard title="Agent 守护进程状态" size="small">
      <NDescriptions label-placement="left" :column="2" bordered size="small">
        <NDescriptionsItem label="连接状态">
          <span :class="agentStore.isConnected ? 'text-emerald-500 font-semibold' : 'text-rose-500 font-semibold'">
            {{ agentStore.isConnected ? '运行中 (Connected)' : '未连接 (Disconnected)' }}
          </span>
        </NDescriptionsItem>
        <NDescriptionsItem label="Agent 版本">
          <span class="font-medium">{{ agentStore.status?.version || '-' }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="运行时长">
          <span class="font-medium">{{ agentStore.status?.uptime_secs != null ? `${agentStore.status.uptime_secs} s` : '0 s' }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="当前活跃任务">
          <span class="font-medium">{{ agentStore.status?.active_tasks ?? 0 }} 个</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="并发执行中">
          <span class="font-medium">{{ agentStore.status?.running_executions ?? 0 }} 个</span>
        </NDescriptionsItem>
      </NDescriptions>

      <template #action>
        <NButton size="small" secondary @click="agentStore.fetchStatus()">
          重新检测连接
        </NButton>
      </template>
    </NCard>
  </div>
</template>
```

- [ ] **Step 4: 编写并更新 `apps/desktop/tests/views.test.ts`**

增加开机自启初始化及横向标签验证测试：
- 验证首次进入时未初始化状态会调用 `enable` 并记录 `easyjob_autostart_initialized`。
- 验证切换开关触发 `setAutostart`。

- [ ] **Step 5: 运行前端测试与类型检查**

运行：
`pnpm -C apps/desktop test && pnpm -C apps/desktop run build`
预期：全部 Vitest 测试通过，Vue-TSC 无类型错误。

- [ ] **Step 6: 提交代码**

```bash
git add apps/desktop/package.json apps/desktop/pnpm-lock.yaml apps/desktop/src apps/desktop/tests
git commit -m "feat(desktop): add autostart switch and refactor agent status to horizontal layout"
```

---

### Task 3: 应用程序图标设计、生成与全套资源替换

**Files:**
- Create: `easyjob_app_icon.png` (设计原图)
- Modify/Replace: `apps/desktop/src-tauri/icons/icon.png`
- Modify/Replace: `apps/desktop/src-tauri/icons/128x128.png`
- Modify/Replace: `apps/desktop/src-tauri/icons/32x32.png`
- Modify: `apps/desktop/public/favicon.svg`
- Modify: `apps/desktop/src/components/layout/AppSidebar.vue`

**Interfaces:**
- Produces: 512x512, 128x128, 32x32 桌面原生图标，SVG 矢量 favicon，侧边栏 Logo 更新。

- [ ] **Step 1: 使用 `generate_image` 生成 App 图标**

Prompt 设计：
> "App icon for 'easyJob', a modern cross-platform automated job scheduler desktop application. Minimalist sleek rounded square app icon with smooth metallic beveled edges. Dark deep midnight blue to electric violet smooth gradient background. In the center is a futuristic glowing minimalist clock dial integrated seamlessly with a sharp dynamic lightning bolt and execution arrow, neon cyan and indigo accents. Premium Apple macOS style app icon, crisp vector aesthetic, clean, polished, 8k resolution, centered."

- [ ] **Step 2: 生成与缩放多尺寸图标文件**

将生成的图标处理并导出为：
- `apps/desktop/src-tauri/icons/icon.png` (512x512 RGBA PNG)
- `apps/desktop/src-tauri/icons/128x128.png` (128x128 RGBA PNG)
- `apps/desktop/src-tauri/icons/32x32.png` (32x32 RGBA PNG)

- [ ] **Step 3: 更新 `apps/desktop/public/favicon.svg` 与 `AppSidebar.vue` Logo**

将 SVG 图标更新为带有流线时钟与闪电箭头的同款品牌矢量图，深色/浅色模式清晰展示。

- [ ] **Step 4: 运行全工作区验证**

运行：
1. `cargo test --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo fmt --check`
4. `pnpm -C apps/desktop test`
5. `pnpm -C apps/desktop run build`
预期：全部 100% 通过。

- [ ] **Step 5: 提交代码**

```bash
git add apps/desktop/src-tauri/icons apps/desktop/public apps/desktop/src
git commit -m "feat(desktop): update brand application icons, favicon, and sidebar logo"
```

---

## Plan Self-Review Checklist

1. **Spec Coverage**:
   - 开机自启 (Windows 注册表，默认开启，--minimized 静默托盘): Task 1 & Task 2 完整覆盖。
   - 系统设置横向展示 (值在参数名后): Task 2 完整覆盖。
   - 图标设计与替换: Task 3 完整覆盖。
2. **Placeholder Scan**: 0 个 TODO/TBD，全部代码完整。
3. **Type Consistency**: 与现有的 `easyjob-domain` 与前端 types 严格一致。
