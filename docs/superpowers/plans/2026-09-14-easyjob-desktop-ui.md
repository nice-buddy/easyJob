# easyJob Phase 3: Tauri 2 + Vue 3 Desktop UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the complete cross-platform desktop UI for easyJob using Tauri 2, Vue 3, Naive UI, Tailwind CSS, and Pinia, bridging user interactions to the `easyjob-agent` background daemon via local OS IPC.

**Architecture:** An independent desktop client (`apps/desktop`) where Tauri 2 Rust Core acts as a secure local proxy and daemon manager. Tauri commands forward UI requests to `easyjob-agent` over local IPC (`easyjob-ipc`), and a background subscriber relays Agent broadcast events directly to Vue 3 frontend windows.

**Tech Stack:**
- Desktop: Tauri 2.x (`@tauri-apps/api 2.x`, `tauri 2.x`)
- Frontend: Vue 3 (SFC `<script setup lang="ts">`), Vite 6, TypeScript, Naive UI, Tailwind CSS 3, Pinia 2, Lucide Vue Next
- Backend/IPC: Rust 2021, `easyjob-ipc`, `easyjob-common`, `easyjob-domain`, Tokio

## Global Constraints

- Agent is the true runtime; Tauri 2 Desktop acts strictly as an operational console and window manager.
- Tauri Core and Agent communicate strictly via local OS IPC (`easyjob-ipc`: Unix Domain Socket `0600` on macOS/Linux, Named Pipe on Windows). NO external TCP/network listeners.
- WebView crash/reload or window close MUST NOT terminate the background scheduling agent. Closing the window hides to the system tray by default.
- UI built with Vue 3 (`<script setup lang="ts">`), Naive UI, Tailwind CSS, Pinia, Lucide icons.
- Every task MUST conclude with passing automated tests and a git commit.

---

## File Structure

```text
easyJob/
├── Cargo.toml                                      # Workspace root containing apps/desktop/src-tauri
├── apps/
│   └── desktop/
│       ├── package.json                            # Frontend dependencies & scripts
│       ├── tsconfig.json                           # TypeScript configuration
│       ├── vite.config.ts                          # Vite 6 config with Tauri dev server settings
│       ├── tailwind.config.js                      # Tailwind CSS config
│       ├── postcss.config.js                       # PostCSS config
│       ├── index.html                              # Entry HTML
│       ├── src/                                    # Vue 3 application
│       │   ├── main.ts                             # App entry point
│       │   ├── App.vue                             # Root component with NConfigProvider & themes
│       │   ├── assets/
│       │   │   └── main.css                        # Tailwind directives & global styling
│       │   ├── types/
│       │   │   ├── task.ts                         # Task, Trigger, Action, Policy TS interfaces
│       │   │   ├── execution.ts                    # Execution, ExecutionStatus TS interfaces
│       │   │   └── agent.ts                        # AgentStatus TS interfaces
│       │   ├── services/
│       │   │   ├── tauri.ts                        # Typed wrappers around Tauri invoke()
│       │   │   └── events.ts                       # Typed wrappers around Tauri listen()
│       │   ├── stores/
│       │   │   ├── taskStore.ts                    # Task CRUD, filters & manual trigger
│       │   │   ├── executionStore.ts               # Execution history & live output buffer
│       │   │   └── agentStore.ts                   # Agent status & connection heartbeat
│       │   ├── views/
│       │   │   ├── TasksView.vue                   # Tasks list, cards, quick actions
│       │   │   ├── ExecutionsView.vue              # Executions history table & filters
│       │   │   └── SettingsView.vue                # Agent health metrics & settings
│       │   └── components/
│       │       ├── layout/
│       │       │   └── AppSidebar.vue              # Navigation sidebar & agent badge
│       │       ├── task/
│       │       │   ├── TaskDrawer.vue              # Create/edit task drawer
│       │       │   ├── TriggerEditor.vue           # Visual trigger form (Once/Interval/Daily/Weekly)
│       │       │   └── ActionEditor.vue            # Visual action form (Shell/Program)
│       │       └── console/
│       │           └── LiveLogDrawer.vue           # Terminal-style real-time log drawer
│       └── src-tauri/
│           ├── Cargo.toml                          # Tauri native crate definition
│           ├── tauri.conf.json                     # Tauri configuration & permissions
│           ├── build.rs                            # Tauri build script
│           ├── icons/                              # App & Tray icons
│           └── src/
│               ├── main.rs                         # Tauri entry point
│               ├── lib.rs                          # Builder assembly & state injection
│               ├── agent_manager.rs                # Agent daemon probing & auto-spawn
│               ├── commands.rs                     # Tauri IPC proxy commands
│               ├── events.rs                       # Agent IPC event listener & Tauri relay
│               └── tray.rs                         # System tray menu & close interception
```

---

### Task 1: Tauri 2 Scaffolding & Dependencies Setup

**Files:**
- Create: `apps/desktop/package.json`
- Create: `apps/desktop/tsconfig.json`
- Create: `apps/desktop/vite.config.ts`
- Create: `apps/desktop/tailwind.config.js`
- Create: `apps/desktop/postcss.config.js`
- Create: `apps/desktop/index.html`
- Create: `apps/desktop/src-tauri/Cargo.toml`
- Create: `apps/desktop/src-tauri/build.rs`
- Create: `apps/desktop/src-tauri/tauri.conf.json`
- Create: `apps/desktop/src-tauri/src/main.rs`
- Create: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `Cargo.toml` (add `apps/desktop/src-tauri` to workspace)

**Interfaces:**
- Consumes: Workspace root `Cargo.toml`
- Produces: Working Tauri 2 + Vite 6 scaffolding with passing `cargo check -p easyjob-desktop` and `pnpm build`

- [ ] **Step 1: Create frontend configuration files in `apps/desktop`**

```json
// apps/desktop/package.json
{
  "name": "easyjob-desktop",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "preview": "vite preview",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.2.0",
    "@tauri-apps/plugin-shell": "^2.2.0",
    "lucide-vue-next": "^0.468.0",
    "naive-ui": "^2.40.1",
    "pinia": "^2.3.0",
    "vue": "^3.5.13"
  },
  "devDependencies": {
    "@vitejs/plugin-vue": "^5.2.1",
    "autoprefixer": "^10.4.20",
    "postcss": "^8.4.49",
    "tailwindcss": "^3.4.16",
    "typescript": "^5.7.2",
    "vite": "^6.0.3",
    "vue-tsc": "^2.2.0"
  }
}
```

```json
// apps/desktop/tsconfig.json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "preserve",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": ["src/**/*.ts", "src/**/*.d.ts", "src/**/*.tsx", "src/**/*.vue"]
}
```

```typescript
// apps/desktop/vite.config.ts
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: process.env.TAURI_ENV_PLATFORM == "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
```

```javascript
// apps/desktop/tailwind.config.js
/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{vue,js,ts,jsx,tsx}",
  ],
  darkMode: 'class',
  theme: {
    extend: {},
  },
  plugins: [],
}
```

```javascript
// apps/desktop/postcss.config.js
export default {
  plugins: {
    tailwindcss: {},
    autoprefixer: {},
  },
}
```

```html
<!-- apps/desktop/index.html -->
<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <link rel="icon" type="image/svg+xml" href="/favicon.svg" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>easyJob</title>
  </head>
  <body class="bg-slate-50 dark:bg-zinc-900 text-slate-900 dark:text-zinc-100 antialiased select-none">
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 2: Create Tauri Rust crate configuration in `apps/desktop/src-tauri`**

```toml
# apps/desktop/src-tauri/Cargo.toml
[package]
name = "easyjob-desktop"
version = "0.1.0"
description = "easyJob Desktop Task Scheduler"
authors = ["easyJob Team"]
edition = "2021"
publish = false

[lib]
name = "easyjob_desktop_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2.0", features = [] }

[dependencies]
easyjob-common = { path = "../../../crates/common" }
easyjob-domain = { path = "../../../crates/domain" }
easyjob-ipc = { path = "../../../crates/ipc" }
tauri = { version = "2.1", features = ["tray-icon"] }
tauri-plugin-shell = "2.0"
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
```

```rust
// apps/desktop/src-tauri/build.rs
fn main() {
    tauri_build::build()
}
```

```json
// apps/desktop/src-tauri/tauri.conf.json
{
  "$schema": "https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-cli/schema.json",
  "productName": "easyJob",
  "version": "0.1.0",
  "identifier": "com.easyjob.desktop",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "easyJob",
        "width": 1100,
        "height": 720,
        "minWidth": 900,
        "minHeight": 600,
        "resizable": true,
        "fullscreen": false
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/icon.png"
    ]
  }
}
```

- [ ] **Step 3: Update root `Cargo.toml` and create initial `src-tauri/src/lib.rs` and `src-tauri/src/main.rs`**

Update `Cargo.toml`:
```toml
[workspace]
members = [
    ".",
    "crates/*",
    "apps/*",
    "apps/desktop/src-tauri",
]
```

Create placeholder `apps/desktop/src/main.ts`:
```typescript
import { createApp } from 'vue';
import App from './App.vue';
import './assets/main.css';

const app = createApp(App);
app.mount('#app');
```

Create placeholder `apps/desktop/src/App.vue`:
```vue
<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">easyJob Desktop Initialized</h1>
  </div>
</template>
```

Create `apps/desktop/src/assets/main.css`:
```css
@tailwind base;
@tailwind components;
@tailwind utilities;
```

Create `apps/desktop/src-tauri/src/lib.rs`:
```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .run(tauri::generate_context!())
        .expect("error while running easyJob desktop");
}
```

Create `apps/desktop/src-tauri/src/main.rs`:
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    easyjob_desktop_lib::run();
}
```

- [ ] **Step 4: Install dependencies and verify build**

Run: `pnpm -C apps/desktop install`
Run: `pnpm -C apps/desktop build`
Run: `cargo check -p easyjob-desktop`
Expected: 0 errors

- [ ] **Step 5: Commit**

```bash
git add apps/desktop Cargo.toml Cargo.lock
git commit -m "chore(desktop): scaffold Tauri 2 and Vue 3 workspace application"
```

---

### Task 2: Tauri Rust Backend Bridge & Agent Process Manager

**Files:**
- Create: `apps/desktop/src-tauri/src/agent_manager.rs`
- Create: `apps/desktop/src-tauri/src/commands.rs`
- Create: `apps/desktop/src-tauri/src/events.rs`
- Create: `apps/desktop/src-tauri/src/tray.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/tests/bridge_tests.rs`

**Interfaces:**
- Consumes: `easyjob-ipc`, `easyjob-common`, `easyjob-domain`
- Produces: Registered Tauri commands (`list_tasks`, `save_task`, `trigger_task`, `cancel_execution`, etc.), system tray and event forwarder.

- [ ] **Step 1: Write integration tests for Tauri commands and event bridge**

Create `apps/desktop/src-tauri/tests/bridge_tests.rs`:
```rust
use easyjob_common::TaskId;
use easyjob_domain::task::Task;
use easyjob_ipc::client::IpcClient;
use easyjob_ipc::protocol::{IpcEvent, IpcRequest, IpcResponse};
use easyjob_ipc::server::{IpcServer, RequestHandler};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Mutex;

struct MockAgentHandler;

#[async_trait::async_trait]
impl RequestHandler for MockAgentHandler {
    async fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        match req.method.as_str() {
            "agent.status" => IpcResponse::success(req.id, serde_json::json!({
                "version": "0.1.0",
                "uptime_secs": 42,
                "active_tasks": 1,
                "running_executions": 0
            })),
            "task.list" => IpcResponse::success(req.id, serde_json::json!([])),
            "task.trigger_now" => IpcResponse::success(req.id, serde_json::json!(true)),
            "execution.cancel" => IpcResponse::success(req.id, serde_json::json!(true)),
            _ => IpcResponse::error(req.id, "Not found"),
        }
    }
}

#[tokio::test]
async fn test_ipc_bridge_client_calls() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("mock.sock");

    let server = IpcServer::bind(&sock, Arc::new(MockAgentHandler)).await.unwrap();
    tokio::spawn(server.run());

    let client = IpcClient::connect(&sock).await.unwrap();
    let res = client.call("agent.status", serde_json::json!({})).await.unwrap();
    assert_eq!(res["version"], "0.1.0");
    assert_eq!(res["active_tasks"], 1);

    let trig = client.call("task.trigger_now", serde_json::json!({ "id": TaskId::new() })).await.unwrap();
    assert_eq!(trig, serde_json::json!(true));
}
```

- [ ] **Step 2: Run test to verify bridge logic passes**

Run: `cargo test -p easyjob-desktop`
Expected: PASS

- [ ] **Step 3: Implement `agent_manager.rs`**

Create `apps/desktop/src-tauri/src/agent_manager.rs`:
```rust
use easyjob_ipc::client::IpcClient;
use easyjob_ipc::default_ipc_path;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub struct AgentManager {
    client: Arc<Mutex<Option<IpcClient>>>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
        }
    }

    pub fn client_handle(&self) -> Arc<Mutex<Option<IpcClient>>> {
        self.client.clone()
    }

    pub async fn ensure_connected(&self) -> Result<IpcClient, String> {
        let mut guard = self.client.lock().await;
        if let Some(ref client) = *guard {
            return Ok(client.clone());
        }

        let ipc_path = default_ipc_path();
        if let Ok(client) = IpcClient::connect(&ipc_path).await {
            *guard = Some(client.clone());
            return Ok(client);
        }

        // Try spawning agent binary
        info!("easyjob-agent is not running; attempting to spawn daemon");
        if let Err(e) = Self::spawn_agent_process() {
            warn!("Failed to spawn easyjob-agent: {}", e);
        }

        // Retry connecting with backoff
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Ok(client) = IpcClient::connect(&ipc_path).await {
                info!("Successfully connected to easyjob-agent IPC");
                *guard = Some(client.clone());
                return Ok(client);
            }
        }

        Err("Failed to connect to easyjob-agent daemon".to_string())
    }

    fn find_agent_binary() -> Result<PathBuf, String> {
        if let Ok(current_exe) = std::env::current_exe() {
            let mut dir = current_exe;
            dir.pop();
            #[cfg(target_os = "windows")]
            let bin_name = "easyjob-agent.exe";
            #[cfg(not(target_os = "windows"))]
            let bin_name = "easyjob-agent";

            let target_bin = dir.join(bin_name);
            if target_bin.exists() {
                return Ok(target_bin);
            }

            if dir.ends_with("deps") {
                dir.pop();
            }
            let target_bin = dir.join(bin_name);
            if target_bin.exists() {
                return Ok(target_bin);
            }
        }

        // Search PATH fallback
        Ok(PathBuf::from("easyjob-agent"))
    }

    fn spawn_agent_process() -> Result<(), String> {
        let bin = Self::find_agent_binary()?;
        let mut cmd = std::process::Command::new(bin);
        cmd.arg("--daemon");

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        cmd.spawn()
            .map_err(|e| format!("Spawn process error: {}", e))?;
        Ok(())
    }
}
```

- [ ] **Step 4: Implement `commands.rs`**

Create `apps/desktop/src-tauri/src/commands.rs`:
```rust
use crate::agent_manager::AgentManager;
use easyjob_common::{ExecutionId, TaskId};
use easyjob_domain::execution::Execution;
use easyjob_domain::task::Task;
use easyjob_ipc::protocol::AgentStatus;
use tauri::State;

#[tauri::command]
pub async fn get_agent_status(manager: State<'_, AgentManager>) -> Result<AgentStatus, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("agent.status", serde_json::json!({}))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_tasks(manager: State<'_, AgentManager>) -> Result<Vec<Task>, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.list", serde_json::json!({}))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_task(id: TaskId, manager: State<'_, AgentManager>) -> Result<Task, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.get", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_task(task: Task, manager: State<'_, AgentManager>) -> Result<Task, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.save", serde_json::json!({ "task": task }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_task(id: TaskId, manager: State<'_, AgentManager>) -> Result<bool, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.delete", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn trigger_task(id: TaskId, manager: State<'_, AgentManager>) -> Result<bool, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.trigger_now", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_executions(
    limit: Option<u32>,
    manager: State<'_, AgentManager>,
) -> Result<Vec<Execution>, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call(
            "execution.list",
            serde_json::json!({ "limit": limit.unwrap_or(50) }),
        )
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_execution(
    id: ExecutionId,
    manager: State<'_, AgentManager>,
) -> Result<Execution, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("execution.get", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_execution(
    id: ExecutionId,
    manager: State<'_, AgentManager>,
) -> Result<bool, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("execution.cancel", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}
```

- [ ] **Step 5: Implement `events.rs` and `tray.rs`**

Create `apps/desktop/src-tauri/src/events.rs`:
```rust
use crate::agent_manager::AgentManager;
use tauri::{AppHandle, Emitter};
use tracing::{error, info};

pub fn spawn_event_relay(app_handle: AppHandle, manager: std::sync::Arc<AgentManager>) {
    tokio::spawn(async move {
        loop {
            match manager.ensure_connected().await {
                Ok(client) => {
                    let mut rx = client.subscribe();
                    info!("Event relay listening to Agent IPC broadcast");
                    while let Ok(event) = rx.recv().await {
                        if let Err(e) = app_handle.emit(&event.event, &event.data) {
                            error!("Failed to emit Tauri event {}: {:?}", event.event, e);
                        }
                    }
                }
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    });
}
```

Create `apps/desktop/src-tauri/src/tray.rs`:
```rust
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

pub fn setup_system_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItemBuilder::with_id("show", "显示主窗口").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出 easyJob").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&show)
        .separator()
        .item(&quit)
        .build()?;

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("easyJob Task Scheduler")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
```

Update `apps/desktop/src-tauri/src/lib.rs`:
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
        .plugin(tauri_plugin_shell::init())
        .manage(agent_manager)
        .setup(move |app| {
            let handle = app.handle().clone();
            spawn_event_relay(handle.clone(), manager_for_events);
            let _ = tray::setup_system_tray(&handle);
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running easyJob desktop");
}
```

- [ ] **Step 6: Run tests and verify build**

Run: `cargo test -p easyjob-desktop`
Run: `cargo check -p easyjob-desktop`
Expected: 0 errors, all tests passing

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src-tauri
git commit -m "feat(desktop): implement Tauri commands, IPC agent manager, event relay, and tray"
```

---

### Task 3: Frontend TypeScript Types, Services & Pinia Stores

**Files:**
- Create: `apps/desktop/src/types/task.ts`
- Create: `apps/desktop/src/types/execution.ts`
- Create: `apps/desktop/src/types/agent.ts`
- Create: `apps/desktop/src/services/tauri.ts`
- Create: `apps/desktop/src/services/events.ts`
- Create: `apps/desktop/src/stores/taskStore.ts`
- Create: `apps/desktop/src/stores/executionStore.ts`
- Create: `apps/desktop/src/stores/agentStore.ts`

**Interfaces:**
- Consumes: `@tauri-apps/api`
- Produces: Reactive Pinia stores for tasks, executions and agent connection state.

- [ ] **Step 1: Define TypeScript models matching Rust domain**

Create `apps/desktop/src/types/task.ts`:
```typescript
export type TaskId = string;
export type TriggerId = string;
export type ActionId = string;

export type ConcurrencyPolicy = 'AllowParallel' | 'SkipIfRunning' | 'QueueOne';
export type MissedRunPolicy = 'RunOnce' | 'Skip';

export interface ExecutionPolicy {
  concurrency_policy: ConcurrencyPolicy;
  missed_run_policy: MissedRunPolicy;
  timeout_secs: number | null;
  max_retries: number;
}

export type TriggerKind =
  | { type: 'Once'; datetime: string }
  | { type: 'Interval'; seconds: number }
  | { type: 'Daily'; time: string; timezone: string }
  | { type: 'Weekly'; days_of_week: number[]; time: string; timezone: string }
  | { type: 'AgentStarted' };

export interface Trigger {
  id: TriggerId;
  task_id: TaskId;
  enabled: boolean;
  kind: TriggerKind;
  created_at: string;
  updated_at: string;
}

export type ActionKind =
  | { type: 'ExecuteShell'; command: string }
  | { type: 'ExecuteProgram'; program: string; args: string[] };

export interface Action {
  id: ActionId;
  task_id: TaskId;
  sequence: number;
  enabled: boolean;
  kind: ActionKind;
}

export interface Task {
  id: TaskId;
  name: string;
  description: string | null;
  enabled: boolean;
  triggers: Trigger[];
  actions: Action[];
  execution_policy: ExecutionPolicy;
  working_directory: string | null;
  environment: Record<string, string>;
  version: number;
  created_at: string;
  updated_at: string;
}
```

Create `apps/desktop/src/types/execution.ts`:
```typescript
import type { TaskId, TriggerId } from './task';

export type ExecutionId = string;

export type ExecutionStatus =
  | 'Pending'
  | 'Running'
  | 'Succeeded'
  | 'Failed'
  | 'TimedOut'
  | 'Cancelled'
  | 'Interrupted';

export interface Execution {
  id: ExecutionId;
  task_id: TaskId;
  trigger_id: TriggerId | null;
  status: ExecutionStatus;
  scheduled_at: string | null;
  started_at: string;
  finished_at: string | null;
  duration_ms: number | null;
  exit_code: number | null;
  error_message: string | null;
}

export interface ExecutionOutputPayload {
  execution_id: ExecutionId;
  task_id: TaskId;
  stream: 'stdout' | 'stderr';
  content: string;
}
```

Create `apps/desktop/src/types/agent.ts`:
```typescript
export interface AgentStatus {
  version: string;
  uptime_secs: number;
  active_tasks: number;
  running_executions: number;
}
```

- [ ] **Step 2: Implement Tauri Service Wrappers**

Create `apps/desktop/src/services/tauri.ts`:
```typescript
import { invoke } from '@tauri-apps/api/core';
import type { Task, TaskId } from '../types/task';
import type { Execution, ExecutionId } from '../types/execution';
import type { AgentStatus } from '../types/agent';

export async function getAgentStatus(): Promise<AgentStatus> {
  return await invoke<AgentStatus>('get_agent_status');
}

export async function listTasks(): Promise<Task[]> {
  return await invoke<Task[]>('list_tasks');
}

export async function getTask(id: TaskId): Promise<Task> {
  return await invoke<Task>('get_task', { id });
}

export async function saveTask(task: Task): Promise<Task> {
  return await invoke<Task>('save_task', { task });
}

export async function deleteTask(id: TaskId): Promise<boolean> {
  return await invoke<boolean>('delete_task', { id });
}

export async function triggerTask(id: TaskId): Promise<boolean> {
  return await invoke<boolean>('trigger_task', { id });
}

export async function listExecutions(limit?: number): Promise<Execution[]> {
  return await invoke<Execution[]>('list_executions', { limit });
}

export async function getExecution(id: ExecutionId): Promise<Execution> {
  return await invoke<Execution>('get_execution', { id });
}

export async function cancelExecution(id: ExecutionId): Promise<boolean> {
  return await invoke<boolean>('cancel_execution', { id });
}
```

Create `apps/desktop/src/services/events.ts`:
```typescript
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { ExecutionOutputPayload } from '../types/execution';

export async function onExecutionStarted(cb: (payload: { execution_id: string; task_id: string }) => void): Promise<UnlistenFn> {
  return await listen('execution.started', (e) => cb(e.payload as any));
}

export async function onExecutionOutput(cb: (payload: ExecutionOutputPayload) => void): Promise<UnlistenFn> {
  return await listen('execution.output', (e) => cb(e.payload as any));
}

export async function onExecutionFinished(cb: (payload: { execution_id: string; task_id: string; status: string; exit_code: number | null }) => void): Promise<UnlistenFn> {
  return await listen('execution.finished', (e) => cb(e.payload as any));
}
```

- [ ] **Step 3: Implement Pinia Stores**

Create `apps/desktop/src/stores/agentStore.ts`:
```typescript
import { defineStore } from 'pinia';
import { ref } from 'vue';
import { getAgentStatus } from '../services/tauri';
import type { AgentStatus } from '../types/agent';

export const useAgentStore = defineStore('agent', () => {
  const status = ref<AgentStatus | null>(null);
  const isConnected = ref(false);
  const lastError = ref<string | null>(null);

  async function fetchStatus() {
    try {
      status.value = await getAgentStatus();
      isConnected.value = true;
      lastError.value = null;
    } catch (e: any) {
      isConnected.value = false;
      lastError.value = String(e);
    }
  }

  return { status, isConnected, lastError, fetchStatus };
});
```

Create `apps/desktop/src/stores/taskStore.ts`:
```typescript
import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import { listTasks, saveTask as apiSaveTask, deleteTask as apiDeleteTask, triggerTask as apiTriggerTask } from '../services/tauri';
import type { Task, TaskId } from '../types/task';

export const useTaskStore = defineStore('tasks', () => {
  const tasks = ref<Task[]>([]);
  const searchQuery = ref('');
  const loading = ref(false);

  const filteredTasks = computed(() => {
    if (!searchQuery.value.trim()) return tasks.value;
    const q = searchQuery.value.toLowerCase();
    return tasks.value.filter(t => t.name.toLowerCase().includes(q) || (t.description && t.description.toLowerCase().includes(q)));
  });

  async function loadTasks() {
    loading.value = true;
    try {
      tasks.value = await listTasks();
    } finally {
      loading.value = false;
    }
  }

  async function saveTask(task: Task) {
    const saved = await apiSaveTask(task);
    const idx = tasks.value.findIndex(t => t.id === saved.id);
    if (idx >= 0) {
      tasks.value[idx] = saved;
    } else {
      tasks.value.unshift(saved);
    }
    return saved;
  }

  async function deleteTask(id: TaskId) {
    await apiDeleteTask(id);
    tasks.value = tasks.value.filter(t => t.id !== id);
  }

  async function triggerTask(id: TaskId) {
    return await apiTriggerTask(id);
  }

  return { tasks, searchQuery, loading, filteredTasks, loadTasks, saveTask, deleteTask, triggerTask };
});
```

Create `apps/desktop/src/stores/executionStore.ts`:
```typescript
import { defineStore } from 'pinia';
import { ref } from 'vue';
import { listExecutions, cancelExecution as apiCancelExecution } from '../services/tauri';
import { onExecutionStarted, onExecutionOutput, onExecutionFinished } from '../services/events';
import type { Execution, ExecutionId } from '../types/execution';

export const useExecutionStore = defineStore('executions', () => {
  const executions = ref<Execution[]>([]);
  const logs = ref<Record<ExecutionId, string[]>>({});
  const activeExecutionId = ref<ExecutionId | null>(null);
  const loading = ref(false);

  async function loadExecutions(limit = 50) {
    loading.value = true;
    try {
      executions.value = await listExecutions(limit);
    } finally {
      loading.value = false;
    }
  }

  async function cancelExecution(id: ExecutionId) {
    return await apiCancelExecution(id);
  }

  function appendLog(id: ExecutionId, content: string) {
    if (!logs.value[id]) {
      logs.value[id] = [];
    }
    logs.value[id].push(content);
  }

  // Setup event listeners
  function initListeners() {
    onExecutionStarted((payload) => {
      // Refresh list or prepend
      loadExecutions();
    });

    onExecutionOutput((payload) => {
      appendLog(payload.execution_id, payload.content);
    });

    onExecutionFinished((payload) => {
      loadExecutions();
    });
  }

  return { executions, logs, activeExecutionId, loading, loadExecutions, cancelExecution, appendLog, initListeners };
});
```

- [ ] **Step 4: Verify TypeScript compilation**

Run: `pnpm -C apps/desktop run build`
Expected: 0 type errors

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/types apps/desktop/src/services apps/desktop/src/stores
git commit -m "feat(desktop): define frontend types, IPC wrappers, and Pinia stores"
```

---

### Task 4: Main Layout & Views (`TasksView`, `ExecutionsView`, `SettingsView`)

**Files:**
- Create: `apps/desktop/src/components/layout/AppSidebar.vue`
- Create: `apps/desktop/src/views/TasksView.vue`
- Create: `apps/desktop/src/views/ExecutionsView.vue`
- Create: `apps/desktop/src/views/SettingsView.vue`
- Modify: `apps/desktop/src/App.vue`
- Modify: `apps/desktop/src/main.ts`

**Interfaces:**
- Consumes: Pinia stores from Task 3
- Produces: Complete navigable desktop UI shell with Naive UI theme and notifications.

- [ ] **Step 1: Set up `main.ts` and `App.vue` with Naive UI provider**

Update `apps/desktop/src/main.ts`:
```typescript
import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './App.vue';
import './assets/main.css';

const app = createApp(App);
app.use(createPinia());
app.mount('#app');
```

Update `apps/desktop/src/App.vue`:
```vue
<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { NConfigProvider, NMessageProvider, NDialogProvider, darkTheme, zhCN, dateZhCN } from 'naive-ui';
import AppSidebar from './components/layout/AppSidebar.vue';
import TasksView from './views/TasksView.vue';
import ExecutionsView from './views/ExecutionsView.vue';
import SettingsView from './views/SettingsView.vue';
import { useAgentStore } from './stores/agentStore';
import { useExecutionStore } from './stores/executionStore';

const isDark = ref(true);
const currentView = ref<'tasks' | 'executions' | 'settings'>('tasks');

const agentStore = useAgentStore();
const executionStore = useExecutionStore();

onMounted(async () => {
  await agentStore.fetchStatus();
  executionStore.initListeners();
  setInterval(() => agentStore.fetchStatus(), 5000);
});
</script>

<template>
  <NConfigProvider :theme="isDark ? darkTheme : null" :locale="zhCN" :date-locale="dateZhCN">
    <NMessageProvider>
      <NDialogProvider>
        <div class="flex h-screen w-screen overflow-hidden bg-white dark:bg-zinc-950 text-slate-800 dark:text-zinc-200">
          <AppSidebar
            :current-view="currentView"
            :is-dark="isDark"
            @change-view="currentView = $event"
            @toggle-theme="isDark = !isDark"
          />
          <main class="flex-1 overflow-y-auto p-6 bg-slate-50/50 dark:bg-zinc-900/40">
            <TasksView v-if="currentView === 'tasks'" />
            <ExecutionsView v-else-if="currentView === 'executions'" />
            <SettingsView v-else-if="currentView === 'settings'" />
          </main>
        </div>
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
```

- [ ] **Step 2: Implement `AppSidebar.vue`**

Create `apps/desktop/src/components/layout/AppSidebar.vue`:
```vue
<script setup lang="ts">
import { useAgentStore } from '../../stores/agentStore';
import { CheckCircle2, AlertCircle, Calendar, History, Settings, Sun, Moon } from 'lucide-vue-next';

defineProps<{
  currentView: 'tasks' | 'executions' | 'settings';
  isDark: boolean;
}>();

defineEmits<{
  (e: 'change-view', view: 'tasks' | 'executions' | 'settings'): void;
  (e: 'toggle-theme'): void;
}>();

const agentStore = useAgentStore();
</script>

<template>
  <aside class="w-60 border-r border-slate-200 dark:border-zinc-800 flex flex-col justify-between p-4 bg-white dark:bg-zinc-900 select-none">
    <div>
      <!-- Brand -->
      <div class="flex items-center gap-3 px-2 py-3 mb-6">
        <div class="w-8 h-8 rounded-lg bg-emerald-600 flex items-center justify-center text-white font-bold shadow-md">
          eJ
        </div>
        <div>
          <div class="font-bold text-base tracking-wide">easyJob</div>
          <div class="text-xs text-slate-400 dark:text-zinc-500">定时任务调度台</div>
        </div>
      </div>

      <!-- Navigation Links -->
      <nav class="space-y-1">
        <button
          @click="$emit('change-view', 'tasks')"
          :class="[
            'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all',
            currentView === 'tasks'
              ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 font-semibold'
              : 'text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800'
          ]"
        >
          <Calendar class="w-4 h-4" />
          任务管理
        </button>

        <button
          @click="$emit('change-view', 'executions')"
          :class="[
            'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all',
            currentView === 'executions'
              ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 font-semibold'
              : 'text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800'
          ]"
        >
          <History class="w-4 h-4" />
          执行记录
        </button>

        <button
          @click="$emit('change-view', 'settings')"
          :class="[
            'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all',
            currentView === 'settings'
              ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 font-semibold'
              : 'text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800'
          ]"
        >
          <Settings class="w-4 h-4" />
          系统设置
        </button>
      </nav>
    </div>

    <!-- Bottom Status & Actions -->
    <div class="pt-4 border-t border-slate-200 dark:border-zinc-800 space-y-3">
      <!-- Agent status badge -->
      <div class="flex items-center justify-between px-2 py-1.5 rounded-md bg-slate-100 dark:bg-zinc-800/60 text-xs">
        <span class="text-slate-500 dark:text-zinc-400">Agent 服务</span>
        <div class="flex items-center gap-1.5 font-medium">
          <span
            class="w-2 h-2 rounded-full"
            :class="agentStore.isConnected ? 'bg-emerald-500 animate-pulse' : 'bg-rose-500'"
          />
          <span :class="agentStore.isConnected ? 'text-emerald-600 dark:text-emerald-400' : 'text-rose-500'">
            {{ agentStore.isConnected ? '已连接' : '未连接' }}
          </span>
        </div>
      </div>

      <!-- Theme Switch -->
      <button
        @click="$emit('toggle-theme')"
        class="w-full flex items-center justify-center gap-2 px-3 py-1.5 rounded-lg border border-slate-200 dark:border-zinc-800 text-xs text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800 transition"
      >
        <component :is="isDark ? Sun : Moon" class="w-3.5 h-3.5" />
        <span>{{ isDark ? '切换浅色' : '切换深色' }}</span>
      </button>
    </div>
  </aside>
</template>
```

- [ ] **Step 3: Implement `TasksView.vue`**

Create `apps/desktop/src/views/TasksView.vue`:
```vue
<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { NButton, NInput, NSwitch, NTag, NEmpty, useMessage } from 'naive-ui';
import { Plus, Search, Play, Edit2, Trash2 } from 'lucide-vue-next';
import { useTaskStore } from '../stores/taskStore';
import type { Task } from '../types/task';

const taskStore = useTaskStore();
const message = useMessage();

onMounted(() => {
  taskStore.loadTasks();
});

async function handleToggleEnabled(task: Task, enabled: boolean) {
  try {
    task.enabled = enabled;
    await taskStore.saveTask(task);
    message.success(enabled ? '任务已启用' : '任务已禁用');
  } catch (e: any) {
    message.error('操作失败: ' + e);
  }
}

async function handleTrigger(task: Task) {
  try {
    await taskStore.triggerTask(task.id);
    message.success(`已下发执行指令: ${task.name}`);
  } catch (e: any) {
    message.error('触发失败: ' + e);
  }
}

async function handleDelete(task: Task) {
  if (confirm(`确认删除任务 "${task.name}" 吗？`)) {
    try {
      await taskStore.deleteTask(task.id);
      message.success('任务已删除');
    } catch (e: any) {
      message.error('删除失败: ' + e);
    }
  }
}
</script>

<template>
  <div>
    <!-- Header bar -->
    <div class="flex items-center justify-between mb-6">
      <div>
        <h1 class="text-xl font-bold">任务管理</h1>
        <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">管理与调度自动化后台作业</p>
      </div>

      <div class="flex items-center gap-3">
        <NInput
          v-model:value="taskStore.searchQuery"
          placeholder="搜索任务名称或描述..."
          size="medium"
          clearable
          class="w-64"
        >
          <template #prefix>
            <Search class="w-4 h-4 text-slate-400" />
          </template>
        </NInput>

        <NButton type="primary" size="medium" class="bg-emerald-600 hover:bg-emerald-500">
          <template #icon>
            <Plus class="w-4 h-4" />
          </template>
          新建任务
        </NButton>
      </div>
    </div>

    <!-- Task List -->
    <div v-if="taskStore.filteredTasks.length === 0" class="py-20 flex justify-center">
      <NEmpty description="暂无任务，点击上方按钮创建第一个任务吧" />
    </div>

    <div v-else class="grid grid-cols-1 gap-3">
      <div
        v-for="task in taskStore.filteredTasks"
        :key="task.id"
        class="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-xl p-4 flex items-center justify-between hover:shadow-sm transition"
      >
        <div class="flex items-center gap-4">
          <NSwitch
            :value="task.enabled"
            @update:value="handleToggleEnabled(task, $event)"
          />
          <div>
            <div class="flex items-center gap-2">
              <span class="font-semibold text-sm">{{ task.name }}</span>
              <NTag size="small" :bordered="false" type="info">
                {{ task.triggers.length }} 个触发器
              </NTag>
              <NTag size="small" :bordered="false" type="default">
                {{ task.actions.length }} 个动作
              </NTag>
            </div>
            <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">
              {{ task.description || '暂无描述' }}
            </p>
          </div>
        </div>

        <div class="flex items-center gap-2">
          <NButton size="small" secondary @click="handleTrigger(task)">
            <template #icon>
              <Play class="w-3.5 h-3.5 text-emerald-500" />
            </template>
            立即执行
          </NButton>

          <NButton size="small" secondary>
            <template #icon>
              <Edit2 class="w-3.5 h-3.5 text-slate-500" />
            </template>
            编辑
          </NButton>

          <NButton size="small" secondary type="error" @click="handleDelete(task)">
            <template #icon>
              <Trash2 class="w-3.5 h-3.5" />
            </template>
          </NButton>
        </div>
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 4: Implement `ExecutionsView.vue` and `SettingsView.vue`**

Create `apps/desktop/src/views/ExecutionsView.vue`:
```vue
<script setup lang="ts">
import { onMounted } from 'vue';
import { NDataTable, NTag, NButton, useMessage } from 'naive-ui';
import { useExecutionStore } from '../stores/executionStore';
import type { Execution } from '../types/execution';

const executionStore = useExecutionStore();
const message = useMessage();

onMounted(() => {
  executionStore.loadExecutions();
});

const columns = [
  {
    title: '任务 ID',
    key: 'task_id',
    ellipsis: true,
  },
  {
    title: '状态',
    key: 'status',
    render(row: Execution) {
      const typeMap: Record<string, 'success' | 'error' | 'warning' | 'info' | 'default'> = {
        Succeeded: 'success',
        Failed: 'error',
        TimedOut: 'warning',
        Running: 'info',
        Cancelled: 'default',
      };
      return h(NTag, { type: typeMap[row.status] || 'default', size: 'small', bordered: false }, { default: () => row.status });
    },
  },
  {
    title: '耗时 (ms)',
    key: 'duration_ms',
    render(row: Execution) {
      return row.duration_ms != null ? `${row.duration_ms} ms` : '-';
    },
  },
  {
    title: '开始时间',
    key: 'started_at',
  },
  {
    title: '操作',
    key: 'actions',
    render(row: Execution) {
      return h(NButton, {
        size: 'tiny',
        secondary: true,
        onClick: () => {
          executionStore.activeExecutionId = row.id;
        },
      }, { default: () => '查看输出' });
    },
  },
];
import { h } from 'vue';
</script>

<template>
  <div>
    <div class="flex items-center justify-between mb-6">
      <div>
        <h1 class="text-xl font-bold">执行记录</h1>
        <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">查看所有自动化任务的历史运行状态与输出流</p>
      </div>
      <NButton size="small" secondary @click="executionStore.loadExecutions()">
        刷新记录
      </NButton>
    </div>

    <NDataTable
      :columns="columns"
      :data="executionStore.executions"
      :loading="executionStore.loading"
      :pagination="{ pageSize: 12 }"
      size="small"
    />
  </div>
</template>
```

Create `apps/desktop/src/views/SettingsView.vue`:
```vue
<script setup lang="ts">
import { useAgentStore } from '../stores/agentStore';
import { NCard, NDescriptions, NDescriptionsItem, NButton } from 'naive-ui';

const agentStore = useAgentStore();
</script>

<template>
  <div class="max-w-3xl space-y-6">
    <div>
      <h1 class="text-xl font-bold">系统设置与状态</h1>
      <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">查看 easyJob 守护进程健康状态与偏好设定</p>
    </div>

    <NCard title="Agent 守护进程状态" size="small">
      <NDescriptions :column="2" bordered size="small">
        <NDescriptionsItem label="连接状态">
          <span :class="agentStore.isConnected ? 'text-emerald-500 font-semibold' : 'text-rose-500 font-semibold'">
            {{ agentStore.isConnected ? '运行中 (Connected)' : '未连接 (Disconnected)' }}
          </span>
        </NDescriptionsItem>
        <NDescriptionsItem label="Agent 版本">
          {{ agentStore.status?.version || '-' }}
        </NDescriptionsItem>
        <NDescriptionsItem label="运行时长 (秒)">
          {{ agentStore.status?.uptime_secs || 0 }} s
        </NDescriptionsItem>
        <NDescriptionsItem label="当前活跃任务">
          {{ agentStore.status?.active_tasks || 0 }}
        </NDescriptionsItem>
        <NDescriptionsItem label="并发执行中">
          {{ agentStore.status?.running_executions || 0 }}
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

- [ ] **Step 5: Verify build**

Run: `pnpm -C apps/desktop run build`
Expected: PASS with 0 errors

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/views apps/desktop/src/components apps/desktop/src/App.vue apps/desktop/src/main.ts
git commit -m "feat(desktop): implement AppSidebar layout and core views for tasks, executions, and settings"
```

---

### Task 5: Visual Task & Trigger/Action Editor Drawers

**Files:**
- Create: `apps/desktop/src/components/task/TriggerEditor.vue`
- Create: `apps/desktop/src/components/task/ActionEditor.vue`
- Create: `apps/desktop/src/components/task/TaskDrawer.vue`
- Modify: `apps/desktop/src/views/TasksView.vue`

**Interfaces:**
- Consumes: `taskStore`, `types/task.ts`
- Produces: Full create/edit drawer with visual trigger/action forms without needing cron syntax.

- [ ] **Step 1: Implement `TriggerEditor.vue`**

Create `apps/desktop/src/components/task/TriggerEditor.vue`:
```vue
<script setup lang="ts">
import { NSelect, NInputNumber, NTimePicker, NCheckboxGroup, NCheckbox, NButton, NCard } from 'naive-ui';
import { Plus, Trash2 } from 'lucide-vue-next';
import type { Trigger, TriggerKind } from '../../types/task';

const props = defineProps<{
  triggers: Trigger[];
  taskId: string;
}>();

const emit = defineEmits<{
  (e: 'update:triggers', triggers: Trigger[]): void;
}>();

const triggerTypeOptions = [
  { label: '间隔触发 (Interval)', value: 'Interval' },
  { label: '每日定时 (Daily)', value: 'Daily' },
  { label: '每周定时 (Weekly)', value: 'Weekly' },
  { label: '启动即运行 (AgentStarted)', value: 'AgentStarted' },
];

const dayOptions = [
  { label: '周一', value: 1 },
  { label: '周二', value: 2 },
  { label: '周三', value: 3 },
  { label: '周四', value: 4 },
  { label: '周五', value: 5 },
  { label: '周六', value: 6 },
  { label: '周日', value: 7 },
];

function addTrigger() {
  const newTrigger: Trigger = {
    id: crypto.randomUUID(),
    task_id: props.taskId,
    enabled: true,
    kind: { type: 'Interval', seconds: 60 },
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  };
  emit('update:triggers', [...props.triggers, newTrigger]);
}

function removeTrigger(index: number) {
  const next = [...props.triggers];
  next.splice(index, 1);
  emit('update:triggers', next);
}

function changeKindType(trigger: Trigger, type: string) {
  if (type === 'Interval') {
    trigger.kind = { type: 'Interval', seconds: 60 };
  } else if (type === 'Daily') {
    trigger.kind = { type: 'Daily', time: '09:00:00', timezone: 'Local' };
  } else if (type === 'Weekly') {
    trigger.kind = { type: 'Weekly', days_of_week: [1, 2, 3, 4, 5], time: '09:00:00', timezone: 'Local' };
  } else if (type === 'AgentStarted') {
    trigger.kind = { type: 'AgentStarted' };
  }
}
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-semibold">触发器列表 (Triggers)</h3>
      <NButton size="tiny" secondary @click="addTrigger">
        <template #icon><Plus class="w-3 h-3" /></template>
        添加触发器
      </NButton>
    </div>

    <div v-for="(tr, idx) in triggers" :key="tr.id" class="p-3 border border-slate-200 dark:border-zinc-800 rounded-lg space-y-3 bg-slate-50/50 dark:bg-zinc-800/30">
      <div class="flex items-center justify-between">
        <NSelect
          :value="tr.kind.type"
          :options="triggerTypeOptions"
          size="small"
          class="w-56"
          @update:value="changeKindType(tr, $event)"
        />
        <NButton size="tiny" text type="error" @click="removeTrigger(idx)">
          <Trash2 class="w-4 h-4" />
        </NButton>
      </div>

      <!-- Interval Editor -->
      <div v-if="tr.kind.type === 'Interval'" class="flex items-center gap-2 text-xs">
        <span>每隔</span>
        <NInputNumber v-model:value="tr.kind.seconds" size="small" :min="1" class="w-28" />
        <span>秒执行一次</span>
      </div>

      <!-- Daily Editor -->
      <div v-if="tr.kind.type === 'Daily'" class="flex items-center gap-2 text-xs">
        <span>每天时间 (HH:mm:ss):</span>
        <input
          type="time"
          step="1"
          v-model="tr.kind.time"
          class="px-2 py-1 rounded border border-slate-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-xs"
        />
      </div>

      <!-- Weekly Editor -->
      <div v-if="tr.kind.type === 'Weekly'" class="space-y-2 text-xs">
        <div class="flex items-center gap-2">
          <span>每周时间:</span>
          <input
            type="time"
            step="1"
            v-model="tr.kind.time"
            class="px-2 py-1 rounded border border-slate-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-xs"
          />
        </div>
        <NCheckboxGroup v-model:value="tr.kind.days_of_week">
          <div class="flex flex-wrap gap-2">
            <NCheckbox v-for="d in dayOptions" :key="d.value" :value="d.value" :label="d.label" size="small" />
          </div>
        </NCheckboxGroup>
      </div>

      <!-- AgentStarted Editor -->
      <div v-if="tr.kind.type === 'AgentStarted'" class="text-xs text-slate-500">
        当 easyJob 后台服务启动或开机自启时，将自动执行一次该任务。
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 2: Implement `ActionEditor.vue`**

Create `apps/desktop/src/components/task/ActionEditor.vue`:
```vue
<script setup lang="ts">
import { NSelect, NInput, NButton } from 'naive-ui';
import { Plus, Trash2 } from 'lucide-vue-next';
import type { Action } from '../../types/task';

const props = defineProps<{
  actions: Action[];
  taskId: string;
}>();

const emit = defineEmits<{
  (e: 'update:actions', actions: Action[]): void;
}>();

function addAction() {
  const newAction: Action = {
    id: crypto.randomUUID(),
    task_id: props.taskId,
    sequence: props.actions.length + 1,
    enabled: true,
    kind: { type: 'ExecuteShell', command: 'echo "Hello easyJob"' },
  };
  emit('update:actions', [...props.actions, newAction]);
}

function removeAction(index: number) {
  const next = [...props.actions];
  next.splice(index, 1);
  emit('update:actions', next);
}
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-semibold">执行动作 (Actions)</h3>
      <NButton size="tiny" secondary @click="addAction">
        <template #icon><Plus class="w-3 h-3" /></template>
        添加动作
      </NButton>
    </div>

    <div v-for="(act, idx) in actions" :key="act.id" class="p-3 border border-slate-200 dark:border-zinc-800 rounded-lg space-y-3 bg-slate-50/50 dark:bg-zinc-800/30">
      <div class="flex items-center justify-between">
        <span class="text-xs font-medium text-slate-500">步骤 {{ idx + 1 }}</span>
        <NButton size="tiny" text type="error" @click="removeAction(idx)">
          <Trash2 class="w-4 h-4" />
        </NButton>
      </div>

      <div class="space-y-1">
        <label class="text-xs text-slate-500">Shell 命令</label>
        <NInput
          v-if="act.kind.type === 'ExecuteShell'"
          v-model:value="act.kind.command"
          type="textarea"
          :autosize="{ minRows: 2, maxRows: 6 }"
          placeholder="输入要执行的 Shell 指令或脚本..."
          size="small"
        />
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 3: Implement `TaskDrawer.vue`**

Create `apps/desktop/src/components/task/TaskDrawer.vue`:
```vue
<script setup lang="ts">
import { ref, watch } from 'vue';
import { NDrawer, NDrawerContent, NForm, NFormItem, NInput, NInputNumber, NSelect, NButton, useMessage } from 'naive-ui';
import TriggerEditor from './TriggerEditor.vue';
import ActionEditor from './ActionEditor.vue';
import type { Task } from '../../types/task';
import { useTaskStore } from '../../stores/taskStore';

const props = defineProps<{
  show: boolean;
  task: Task | null;
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
  (e: 'saved'): void;
}>();

const taskStore = useTaskStore();
const message = useMessage();

const currentTask = ref<Task>(getEmptyTask());

function getEmptyTask(): Task {
  return {
    id: crypto.randomUUID(),
    name: '',
    description: '',
    enabled: true,
    triggers: [],
    actions: [],
    execution_policy: {
      concurrency_policy: 'SkipIfRunning',
      missed_run_policy: 'Skip',
      timeout_secs: 3600,
      max_retries: 0,
    },
    working_directory: null,
    environment: {},
    version: 1,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  };
}

watch(
  () => props.task,
  (t) => {
    if (t) {
      currentTask.value = JSON.parse(JSON.stringify(t));
    } else {
      currentTask.value = getEmptyTask();
    }
  },
  { immediate: true }
);

const concurrencyOptions = [
  { label: '单例跳过 (SkipIfRunning - 推荐)', value: 'SkipIfRunning' },
  { label: '允许并行 (AllowParallel)', value: 'AllowParallel' },
  { label: '至多排队一个 (QueueOne)', value: 'QueueOne' },
];

async function handleSave() {
  if (!currentTask.value.name.trim()) {
    message.warning('请输入任务名称');
    return;
  }
  try {
    await taskStore.saveTask(currentTask.value);
    message.success('任务保存成功');
    emit('update:show', false);
    emit('saved');
  } catch (e: any) {
    message.error('保存失败: ' + e);
  }
}
</script>

<template>
  <NDrawer :show="show" width="550" @update:show="$emit('update:show', $event)">
    <NDrawerContent :title="task ? '编辑任务' : '新建任务'" closable>
      <div class="space-y-6 pb-12">
        <NForm label-placement="top" size="small">
          <NFormItem label="任务名称" required>
            <NInput v-model:value="currentTask.name" placeholder="例如：每日数据库定时备份" />
          </NFormItem>

          <NFormItem label="任务描述">
            <NInput v-model:value="currentTask.description" type="textarea" placeholder="任务详细用途与执行逻辑说明..." />
          </NFormItem>

          <div class="grid grid-cols-2 gap-4">
            <NFormItem label="并发执行策略">
              <NSelect v-model:value="currentTask.execution_policy.concurrency_policy" :options="concurrencyOptions" />
            </NFormItem>

            <NFormItem label="超时时间 (秒)">
              <NInputNumber v-model:value="currentTask.execution_policy.timeout_secs" :min="1" />
            </NFormItem>
          </div>
        </NForm>

        <!-- Triggers -->
        <TriggerEditor
          :triggers="currentTask.triggers"
          :task-id="currentTask.id"
          @update:triggers="currentTask.triggers = $event"
        />

        <!-- Actions -->
        <ActionEditor
          :actions="currentTask.actions"
          :task-id="currentTask.id"
          @update:actions="currentTask.actions = $event"
        />
      </div>

      <template #footer>
        <div class="flex justify-end gap-3">
          <NButton size="small" @click="$emit('update:show', false)">取消</NButton>
          <NButton size="small" type="primary" class="bg-emerald-600 hover:bg-emerald-500" @click="handleSave">
            保存任务
          </NButton>
        </div>
      </template>
    </NDrawerContent>
  </NDrawer>
</template>
```

- [ ] **Step 4: Connect `TaskDrawer` in `TasksView.vue`**

Update `apps/desktop/src/views/TasksView.vue` to declare:
```typescript
const showDrawer = ref(false);
const editingTask = ref<Task | null>(null);

function openCreateDrawer() {
  editingTask.value = null;
  showDrawer.value = true;
}

function openEditDrawer(task: Task) {
  editingTask.value = task;
  showDrawer.value = true;
}
```
And render `<TaskDrawer v-model:show="showDrawer" :task="editingTask" @saved="taskStore.loadTasks()" />`.

- [ ] **Step 5: Verify build**

Run: `pnpm -C apps/desktop run build`
Expected: PASS with 0 errors

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/components/task apps/desktop/src/views/TasksView.vue
git commit -m "feat(desktop): implement TaskDrawer with visual TriggerEditor and ActionEditor"
```

---

### Task 6: Live Streaming Log Console & Task Cancellation

**Files:**
- Create: `apps/desktop/src/components/console/LiveLogDrawer.vue`
- Modify: `apps/desktop/src/views/ExecutionsView.vue`
- Modify: `apps/desktop/src/App.vue`

**Interfaces:**
- Consumes: `executionStore`, `cancel_execution` Tauri command
- Produces: Terminal-style real-time log drawer with auto-scroll and task cancellation.

- [ ] **Step 1: Implement `LiveLogDrawer.vue`**

Create `apps/desktop/src/components/console/LiveLogDrawer.vue`:
```vue
<script setup lang="ts">
import { computed, ref, watch, nextTick } from 'vue';
import { NDrawer, NDrawerContent, NButton, NTag, useMessage } from 'naive-ui';
import { Square, RotateCw } from 'lucide-vue-next';
import { useExecutionStore } from '../../stores/executionStore';
import { getExecution } from '../../services/tauri';
import type { Execution } from '../../types/execution';

const props = defineProps<{
  executionId: string | null;
  show: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
}>();

const executionStore = useExecutionStore();
const message = useMessage();
const currentExecution = ref<Execution | null>(null);
const logContainer = ref<HTMLElement | null>(null);

const logLines = computed(() => {
  if (!props.executionId) return [];
  return executionStore.logs[props.executionId] || [];
});

watch(
  () => props.executionId,
  async (id) => {
    if (id) {
      try {
        currentExecution.value = await getExecution(id);
      } catch (_) {}
    }
  },
  { immediate: true }
);

watch(
  () => logLines.value.length,
  async () => {
    await nextTick();
    if (logContainer.value) {
      logContainer.value.scrollTop = logContainer.value.scrollHeight;
    }
  }
);

async function handleCancel() {
  if (!props.executionId) return;
  try {
    await executionStore.cancelExecution(props.executionId);
    message.success('已下发终止指令');
    if (currentExecution.value) {
      currentExecution.value.status = 'Cancelled';
    }
  } catch (e: any) {
    message.error('终止失败: ' + e);
  }
}
</script>

<template>
  <NDrawer :show="show" width="600" @update:show="$emit('update:show', $event)">
    <NDrawerContent title="运行实时监控" closable>
      <div class="h-full flex flex-col space-y-3 pb-8">
        <!-- Status Bar -->
        <div class="flex items-center justify-between p-3 rounded-lg bg-slate-100 dark:bg-zinc-800/60 text-xs">
          <div class="flex items-center gap-2">
            <span>状态:</span>
            <NTag size="small" :bordered="false" type="info">
              {{ currentExecution?.status || 'Unknown' }}
            </NTag>
            <span v-if="currentExecution?.duration_ms != null">
              耗时: {{ currentExecution.duration_ms }} ms
            </span>
          </div>

          <NButton
            v-if="currentExecution?.status === 'Running'"
            size="tiny"
            type="error"
            secondary
            @click="handleCancel"
          >
            <template #icon><Square class="w-3 h-3" /></template>
            终止执行
          </NButton>
        </div>

        <!-- Terminal View -->
        <div
          ref="logContainer"
          class="flex-1 bg-zinc-950 text-zinc-200 p-4 rounded-lg font-mono text-xs overflow-y-auto leading-relaxed border border-zinc-800 shadow-inner"
        >
          <div v-if="logLines.length === 0" class="text-zinc-500 italic">
            等待输出流数据中...
          </div>
          <div v-for="(line, idx) in logLines" :key="idx" class="whitespace-pre-wrap break-all">
            {{ line }}
          </div>
        </div>
      </div>
    </NDrawerContent>
  </NDrawer>
</template>
```

- [ ] **Step 2: Connect `LiveLogDrawer` in `ExecutionsView.vue`**

Update `apps/desktop/src/views/ExecutionsView.vue`:
```vue
<LiveLogDrawer
  :show="showLog"
  :execution-id="selectedExecId"
  @update:show="showLog = $event"
/>
```

- [ ] **Step 3: Verify build**

Run: `pnpm -C apps/desktop run build`
Expected: PASS with 0 errors

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/components/console apps/desktop/src/views/ExecutionsView.vue
git commit -m "feat(desktop): implement terminal LiveLogDrawer with streaming auto-scroll and cancel execution"
```

---

### Task 7: Full Workspace Integration, Build & Packaging Verification

**Files:**
- Modify: `tests/engine_integration_test.rs` (ensure workspace integrity)
- Create: `walkthrough.md` (updated with Phase 3 walkthrough & results)

**Interfaces:**
- Consumes: All Phase 1, Phase 2, and Phase 3 deliverables
- Produces: 100% verified test suite across all workspace crates and desktop app.

- [ ] **Step 1: Verify all Rust tests in workspace**

Run: `cargo test --all`
Expected: 100% PASS (at least 74 tests)

- [ ] **Step 2: Verify Clippy across entire workspace**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 0 warnings, 0 errors

- [ ] **Step 3: Verify Rust formatting**

Run: `cargo fmt --check`
Expected: PASS

- [ ] **Step 4: Verify Frontend build**

Run: `pnpm -C apps/desktop run build`
Expected: PASS with 0 TypeScript errors and optimized bundle output

- [ ] **Step 5: Verify Desktop app check**

Run: `cargo check -p easyjob-desktop`
Expected: PASS with 0 warnings

- [ ] **Step 6: Commit and documentation**

```bash
git add .
git commit -m "chore(release): Phase 3 Tauri 2 and Vue 3 desktop integration complete"
```
