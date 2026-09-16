# easyJob 任务导入导出与三栏差异合并功能设计规范

## 1. 概述与设计背景

本文档定义 easyJob 桌面端（Tauri 2 + Vue 3）的任务批量导入、导出与同名冲突差异合并功能：
1. **任务勾选批量导出**：支持在桌面端弹窗中勾选特定任务或全选，导出为符合标准模式的 JSON 结构化数据文件。
2. **任务文件导入与冲突检测**：解析导入的 JSON 文件，按任务名称（`name`）与当前已有任务进行重名检测。
3. **三模式冲突处理**：针对同名冲突任务，支持【覆盖】、【跳过】与【手动合并】。
4. **三栏手风琴手动合并面板**：
   - 采用三栏水平并排展示：第一栏为已有任务配置（只读）、第二栏为导入任务配置（只读 + 差异高亮）、第三栏为最终结果（可编辑表单）；
   - 各栏内容采用纵向手风琴（`NCollapse`）布局展示五个核心模块：基本信息、执行策略、触发规则、执行动作、环境变量；
   - 第三栏初始默认展示导入任务配置，各手风琴面板头部提供【采用已有配置】与【还原导入配置】的块级快捷合并功能。

---

## 2. 导出与导入数据协议规范

### 2.1 导出 JSON 数据格式 (`ExportPayload`)
```typescript
export interface TaskExportPayload {
  easyjob_version: string;     // 如 "0.1.0"
  exported_at: string;         // ISO 8601 时间戳
  tasks: Task[];               // 选中的任务完整配置数组
}
```
- 导出的文件名格式：`easyjob-tasks-YYYYMMDD-HHmmss.json`。
- 采用 Web 标准 `Blob` 与 `URL.createObjectURL` 触发浏览器/原生 Webview 下载，保证无多余原生插件依赖。

### 2.2 导入校验与 ID 重建规则
- **格式校验**：检查文件是否为合法 JSON，且包含 `tasks` 数组或直接为 `Task[]` 数组。过滤缺少核心字段（如 `name`, `triggers`, `actions`, `execution_policy`）的脏数据。
- **无冲突任务（新增）**：
  导入时自动为任务重新生成全新的 UUID（包括 `task.id`、`triggers[i].id`、`actions[i].id`），并将 `version` 重置为 1，生成新的 `created_at` 与 `updated_at`，防止外部 ID 碰撞。
- **覆盖与合并任务**：
  保持已有任务的 `id`，更新其 `name`、`description`、`enabled`、`working_directory`、`execution_policy`、`environment`，并重新生成/替换内部的 `triggers` 与 `actions` 关联，版本号递增。

---

## 3. 差异对比引擎设计 (`taskDiff.ts`)

### 3.1 差异比对维度与结果定义
```typescript
export interface TaskDiffResult {
  hasDiff: boolean;
  basicDiff: boolean;
  policyDiff: boolean;
  triggersDiff: boolean;
  actionsDiff: boolean;
  envDiff: boolean;
  diffFields: Set<string>; // 记录具体的差异字段 key
}
```

### 3.2 详细对比算法
1. **基本配置 (`basic`)**：
   - `name`（虽然同名进入合并，但保留比对）、`description`、`enabled`、`working_directory`。
2. **执行策略 (`policy`)**：
   - `concurrency_policy`、`missed_run_policy`、`timeout_secs`；
   - `retry_policy.max_retries`、`retry_policy.delay_secs`。
3. **触发规则 (`triggers`)**：
   - 数量是否相等；
   - 按照触发种类（Once, Interval, Daily, Weekly, AgentStarted）及内部时间、时区、周期等参数进行深度 JSON 字符串比对。
4. **执行动作 (`actions`)**：
   - 动作数量与执行顺序（`sequence`）；
   - 动作种类与脚本/命令内容（Shell command, Cmd command, PowerShell script, Program path & args）。
5. **环境变量 (`environment`)**：
   - 键值对增删与内容修改比对。

### 3.3 第二栏视觉高亮标记
- **手风琴折叠面板头部**：若该面板中任意字段存在差异，在面板标题右侧标记琥珀色黄色标签 `<NTag size="small" type="warning">有差异</NTag>`。
- **字段级与卡片级高亮**：对发生变化的配置项，背景应用浅琥珀色高亮（`bg-amber-50/60 dark:bg-amber-950/30 border border-amber-300 dark:border-amber-800/80 rounded-lg p-2.5`），直观凸显与已有配置不同的项。

---

## 4. UI 组件与交互流程设计

### 4.1 任务管理顶栏 (`TasksView.vue`)
- 在原有的“新建任务”左侧新增两个操作按钮：
  - 【导出任务】按钮（带导出图标）；
  - 【导入任务】按钮（带导入图标）。

### 4.2 任务导出弹窗 (`TaskExportModal.vue`)
- **展示内容**：
  - 顶部：搜索过滤输入框、已选中计数（如 `已选择 3 / 10 个任务`）、【全选】与【取消全选】按钮。
  - 中部：可滚动的任务复选框列表，展示任务名称、描述及触发器/动作数量。
  - 底部：【取消】与【确认导出选中任务】按钮。

### 4.3 任务导入与冲突确认弹窗 (`TaskImportModal.vue`)
- **流程状态**：
  1. **待选文件状态**：显示文件拖拽/选择区域，支持选择单个 `.json` 文件。
  2. **任务确认解析状态**：解析成功后渲染待导入任务列表：
     - **无冲突任务**：标记为“🟢 新增”，默认勾选导入；
     - **同名冲突任务**：标记为“🟡 同名冲突”，右侧操作列提供单选切换：
       - `覆盖` (Overwrite)
       - `跳过` (Skip)
       - `手动合并` (Manual Merge) -> 点击后弹出三栏合并弹窗；
     - 顶部提供批量操作按钮：【同名全部覆盖】、【同名全部跳过】。
  3. **执行导入**：遍历所有决策，调用 `taskStore.saveTask` 批量入库，更新本地任务列表并给出导入成功摘要。

### 4.4 三栏纵向手风琴合并弹窗 (`TaskMergeModal.vue`)
- **弹窗规格**：宽屏设计（`width="92vw"`，最大 1350px），包含清晰的标题“合并任务冲突: [任务名称]”。
- **三栏布局**：
  - **第一栏（已有任务配置 - 纯只读）**：
    - 浅灰背景，手风琴展示已有任务的所有 5 个模块。
  - **第二栏（导入任务配置 - 只读 + 差异高亮）**：
    - 与第一栏不同的字段带有明显的黄色警告高亮框与“配置不同”标记。
  - **第三栏（最终合并结果 - 全功能可编辑）**：
    - 初始状态默认深拷贝第二栏（导入任务）配置；
    - 每个手风琴面板头部右侧配备快捷按钮：
      - `[◀ 采用已有]`：一键将第一栏该面板的对应数据覆盖到第三栏；
      - `[◀ 还原导入]`：一键将第二栏该面板的对应数据重置到第三栏；
    - 面板内部嵌入完整的编辑输入控件（文本输入、数字步进器、策略下拉选择器、触发器规则选择器、执行动作编辑器）。
- **合并结果持久化**：
  点击【确认合并此任务】后，将第三栏修改后的完整配置对象暂存回 `TaskImportModal` 的冲突队列中（标记为“已合并”），关闭三栏弹窗。

---

## 5. 验证与质量保障

1. **单元测试与集成测试 (`views.test.ts` / `taskDiff.test.ts`)**：
   - 针对 `taskDiff.ts` 编写专门测试用例：覆盖基本信息、策略、触发器增减修改、动作类型变化、环境变量变动的 Diff 检测；
   - 针对导入/导出逻辑编写单元测试：覆盖格式合法性校验、同名冲突识别、UUID 重新生成逻辑；
   - 保证全部 Vitest 测试（93+ 项）与全工作区 Rust 测试（79 项）持续 100% 通过。
2. **代码质量与静态检查**：
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo fmt --check`
   - `vue-tsc --noEmit`
   - `pnpm -C apps/desktop run build`
