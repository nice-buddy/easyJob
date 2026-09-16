# 任务导入导出与三栏纵向手风琴差异合并实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 easyJob 桌面端任务的勾选批量导出（JSON 文件）、导入解析校验、重名冲突检测，以及三栏纵向手风琴差异比对与手动合并面板。

**Architecture:** 
- 纯前端无依赖 Diff 引擎：`apps/desktop/src/utils/taskDiff.ts` 进行多维度深度对比，输出字段级与区块级差异标记；
- 模块化交互弹窗：
  - `TaskExportModal.vue`：任务多选列表、搜索过滤、全选与标准 JSON 文件下载；
  - `TaskImportModal.vue`：JSON 文件上传校验、同名冲突识别、单个/批量【覆盖/跳过/手动合并】；
  - `TaskMergeModal.vue` & `TaskAccordionContent.vue`：三栏宽屏手风琴（第一栏已有任务只读、第二栏导入任务只读+高亮、第三栏最终结果可编辑+块级一键采用）；
- `TasksView.vue` 顶栏串联两个功能入口并统一刷新任务列表。

**Tech Stack:** Vue 3, TypeScript, Naive UI (`NModal`, `NCollapse`, `NCollapseItem`, `NCheckbox`, `NTag`, `NButton`, `NInput`), Vitest.

## Global Constraints

- 导出的 JSON 必须携带 `easyjob_version`、`exported_at` 与 `tasks` 数组。
- 导入新任务时必须为其重新生成全新 UUID，防止外部 ID 与本地现有 ID 产生物理冲突。
- 覆盖或合并已有任务时，保留已有任务的原始 `id` 并递增其版本号。
- 三栏合并弹框中的每一栏都必须采用纵向手风琴（`NCollapse`）展示（替代原有的横向 TAB）。
- 第二栏必须对与第一栏存在差异的配置项进行视觉高亮或标识。
- 第三栏默认展示导入任务配置，且仅第三栏可编辑，各手风琴块提供【采用已有配置】与【还原导入配置】快捷按钮。
- 所有单元测试 100% 通过，`cargo clippy` 0 警告，`vue-tsc` 0 类型错误。

---

### Task 1: 任务深度比对工具库与单元测试 (`taskDiff.ts`)

**Files:**
- Create: `apps/desktop/src/utils/taskDiff.ts`
- Create: `apps/desktop/tests/taskDiff.test.ts`

**Interfaces:**
- Produces:
  ```typescript
  export interface TaskDiffResult {
    hasDiff: boolean;
    basicDiff: boolean;
    policyDiff: boolean;
    triggersDiff: boolean;
    actionsDiff: boolean;
    envDiff: boolean;
    diffFields: Set<string>;
  }
  export function compareTasks(existingTask: Task, importedTask: Task): TaskDiffResult;
  ```

- [ ] **Step 1: 编写测试用例 `apps/desktop/tests/taskDiff.test.ts`**

```typescript
import { describe, it, expect } from 'vitest';
import { compareTasks } from '../src/utils/taskDiff';
import { getEmptyTask } from '../src/types/task';
import type { Task } from '../src/types/task';

describe('taskDiff utility', () => {
  it('returns hasDiff=false when tasks are identical in configuration', () => {
    const t1 = getEmptyTask();
    t1.name = 'Test Task';
    t1.description = 'Identical description';
    const t2 = JSON.parse(JSON.stringify(t1));

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(false);
    expect(diff.basicDiff).toBe(false);
    expect(diff.policyDiff).toBe(false);
    expect(diff.triggersDiff).toBe(false);
    expect(diff.actionsDiff).toBe(false);
    expect(diff.envDiff).toBe(false);
    expect(diff.diffFields.size).toBe(0);
  });

  it('detects basic info differences', () => {
    const t1 = getEmptyTask();
    t1.description = 'Old description';
    const t2 = JSON.parse(JSON.stringify(t1));
    t2.description = 'New description';
    t2.working_directory = '/tmp/new';

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.basicDiff).toBe(true);
    expect(diff.diffFields.has('basic.description')).toBe(true);
    expect(diff.diffFields.has('basic.working_directory')).toBe(true);
    expect(diff.policyDiff).toBe(false);
  });

  it('detects execution policy differences', () => {
    const t1 = getEmptyTask();
    t1.execution_policy.timeout_secs = 3600;
    const t2 = JSON.parse(JSON.stringify(t1));
    t2.execution_policy.timeout_secs = 7200;
    t2.execution_policy.concurrency_policy = 'AllowParallel';

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.policyDiff).toBe(true);
    expect(diff.diffFields.has('policy.timeout_secs')).toBe(true);
    expect(diff.diffFields.has('policy.concurrency_policy')).toBe(true);
  });

  it('detects triggers and actions differences', () => {
    const t1 = getEmptyTask();
    t1.triggers = [
      {
        id: 'tr-1',
        task_id: t1.id,
        enabled: true,
        kind: { Interval: { seconds: 60 } },
        created_at: '',
        updated_at: '',
      },
    ];
    t1.actions = [
      {
        id: 'act-1',
        task_id: t1.id,
        sequence: 0,
        enabled: true,
        kind: { ExecuteShell: { command: 'echo 1' } },
      },
    ];

    const t2 = JSON.parse(JSON.stringify(t1));
    t2.triggers[0].kind = { Interval: { seconds: 120 } };
    t2.actions.push({
      id: 'act-2',
      task_id: t2.id,
      sequence: 1,
      enabled: true,
      kind: { ExecuteShell: { command: 'echo 2' } },
    });

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.triggersDiff).toBe(true);
    expect(diff.actionsDiff).toBe(true);
    expect(diff.diffFields.has('triggers')).toBe(true);
    expect(diff.diffFields.has('actions')).toBe(true);
  });

  it('detects environment variable differences', () => {
    const t1 = getEmptyTask();
    t1.environment = { FOO: 'bar' };
    const t2 = JSON.parse(JSON.stringify(t1));
    t2.environment = { FOO: 'baz', EXTRA: '123' };

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.envDiff).toBe(true);
    expect(diff.diffFields.has('env.FOO')).toBe(true);
    expect(diff.diffFields.has('env.EXTRA')).toBe(true);
  });
});
```

- [ ] **Step 2: 运行测试验证失败**

运行：`pnpm -C apps/desktop vitest run tests/taskDiff.test.ts`
预期：FAIL（文件不存在）

- [ ] **Step 3: 编写 `apps/desktop/src/utils/taskDiff.ts` 实现**

```typescript
import type { Task, Trigger, Action } from '../types/task';

export interface TaskDiffResult {
  hasDiff: boolean;
  basicDiff: boolean;
  policyDiff: boolean;
  triggersDiff: boolean;
  actionsDiff: boolean;
  envDiff: boolean;
  diffFields: Set<string>;
}

function normalizeTrigger(tr: Trigger) {
  return {
    enabled: tr.enabled,
    kind: tr.kind,
  };
}

function normalizeAction(act: Action) {
  return {
    enabled: act.enabled,
    sequence: act.sequence,
    kind: act.kind,
  };
}

export function compareTasks(existingTask: Task, importedTask: Task): TaskDiffResult {
  const diffFields = new Set<string>();

  // 1. Basic configuration
  if ((existingTask.description || '') !== (importedTask.description || '')) {
    diffFields.add('basic.description');
  }
  if (existingTask.enabled !== importedTask.enabled) {
    diffFields.add('basic.enabled');
  }
  if ((existingTask.working_directory || '') !== (importedTask.working_directory || '')) {
    diffFields.add('basic.working_directory');
  }
  const basicDiff =
    diffFields.has('basic.description') ||
    diffFields.has('basic.enabled') ||
    diffFields.has('basic.working_directory');

  // 2. Execution policy
  const ep1 = existingTask.execution_policy;
  const ep2 = importedTask.execution_policy;

  if (ep1.concurrency_policy !== ep2.concurrency_policy) {
    diffFields.add('policy.concurrency_policy');
  }
  if (ep1.missed_run_policy !== ep2.missed_run_policy) {
    diffFields.add('policy.missed_run_policy');
  }
  if (ep1.timeout_secs !== ep2.timeout_secs) {
    diffFields.add('policy.timeout_secs');
  }
  if (ep1.retry_policy.max_retries !== ep2.retry_policy.max_retries) {
    diffFields.add('policy.retry_max_retries');
  }
  if (ep1.retry_policy.delay_secs !== ep2.retry_policy.delay_secs) {
    diffFields.add('policy.retry_delay_secs');
  }

  const policyDiff =
    diffFields.has('policy.concurrency_policy') ||
    diffFields.has('policy.missed_run_policy') ||
    diffFields.has('policy.timeout_secs') ||
    diffFields.has('policy.retry_max_retries') ||
    diffFields.has('policy.retry_delay_secs');

  // 3. Triggers
  const normTriggers1 = existingTask.triggers.map(normalizeTrigger);
  const normTriggers2 = importedTask.triggers.map(normalizeTrigger);
  const triggersDiff = JSON.stringify(normTriggers1) !== JSON.stringify(normTriggers2);
  if (triggersDiff) {
    diffFields.add('triggers');
  }

  // 4. Actions
  const normActions1 = existingTask.actions.map(normalizeAction);
  const normActions2 = importedTask.actions.map(normalizeAction);
  const actionsDiff = JSON.stringify(normActions1) !== JSON.stringify(normActions2);
  if (actionsDiff) {
    diffFields.add('actions');
  }

  // 5. Environment variables
  const env1 = existingTask.environment || {};
  const env2 = importedTask.environment || {};
  const allEnvKeys = new Set([...Object.keys(env1), ...Object.keys(env2)]);
  let envDiff = false;

  for (const key of allEnvKeys) {
    if (env1[key] !== env2[key]) {
      diffFields.add(`env.${key}`);
      envDiff = true;
    }
  }

  const hasDiff = basicDiff || policyDiff || triggersDiff || actionsDiff || envDiff;

  return {
    hasDiff,
    basicDiff,
    policyDiff,
    triggersDiff,
    actionsDiff,
    envDiff,
    diffFields,
  };
}
```

- [ ] **Step 4: 运行测试验证通过**

运行：`pnpm -C apps/desktop vitest run tests/taskDiff.test.ts`
预期：PASS（5/5 passed）

- [ ] **Step 5: 提交代码**

```bash
git add apps/desktop/src/utils/taskDiff.ts apps/desktop/tests/taskDiff.test.ts
git commit -m "feat(desktop): implement task diff engine with detailed field comparison"
```

---

### Task 2: 任务多选批量导出弹窗 (`TaskExportModal.vue`) 与 TasksView 接入

**Files:**
- Create: `apps/desktop/src/components/task/TaskExportModal.vue`
- Modify: `apps/desktop/src/views/TasksView.vue`
- Create: `apps/desktop/tests/taskExport.test.ts`

**Interfaces:**
- Consumes: `taskStore.tasks`
- Produces: `TaskExportModal.vue` 组件，支持勾选、全选、反选与导出生成标准 JSON 文件。

- [ ] **Step 1: 编写 `tests/taskExport.test.ts` 测试**

```typescript
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { getEmptyTask } from '../src/types/task';
import type { Task } from '../src/types/task';

describe('Task Export payload builder', () => {
  it('generates standard export payload format with metadata', () => {
    const task1 = { ...getEmptyTask(), id: 'task-1', name: 'Backup' };
    const task2 = { ...getEmptyTask(), id: 'task-2', name: 'Cleanup' };
    const selected = [task1, task2];

    const payload = {
      easyjob_version: '0.1.0',
      exported_at: new Date().toISOString(),
      tasks: selected,
    };

    expect(payload.easyjob_version).toBe('0.1.0');
    expect(payload.tasks).toHaveLength(2);
    expect(payload.tasks[0].name).toBe('Backup');
    expect(payload.tasks[1].name).toBe('Cleanup');
  });

  it('filters selected tasks correctly from ID set', () => {
    const task1 = { ...getEmptyTask(), id: 'task-1', name: 'T1' };
    const task2 = { ...getEmptyTask(), id: 'task-2', name: 'T2' };
    const all = [task1, task2];
    const selectedIds = new Set(['task-2']);

    const exported = all.filter((t) => selectedIds.has(t.id));
    expect(exported).toHaveLength(1);
    expect(exported[0].id).toBe('task-2');
  });
});
```

- [ ] **Step 2: 运行测试验证**

运行：`pnpm -C apps/desktop vitest run tests/taskExport.test.ts`
预期：PASS

- [ ] **Step 3: 创建 `apps/desktop/src/components/task/TaskExportModal.vue`**

```vue
<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import {
  NModal,
  NCard,
  NInput,
  NCheckbox,
  NCheckboxGroup,
  NButton,
  NTag,
  NEmpty,
  useMessage,
} from 'naive-ui';
import { Search, Download, CheckSquare, Square } from 'lucide-vue-next';
import type { Task } from '../../types/task';

const props = defineProps<{
  show: boolean;
  tasks: Task[];
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
}>();

const message = useMessage();
const searchQuery = ref('');
const selectedTaskIds = ref<string[]>([]);

// 每次打开弹窗默认全选
watch(
  () => props.show,
  (show) => {
    if (show) {
      selectedTaskIds.value = props.tasks.map((t) => t.id);
      searchQuery.value = '';
    }
  }
);

const filteredTasks = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return props.tasks;
  return props.tasks.filter(
    (t) =>
      t.name.toLowerCase().includes(q) ||
      (t.description && t.description.toLowerCase().includes(q))
  );
});

const isAllSelected = computed(() => {
  return (
    filteredTasks.value.length > 0 &&
    filteredTasks.value.every((t) => selectedTaskIds.value.includes(t.id))
  );
});

function handleToggleSelectAll() {
  if (isAllSelected.value) {
    const currentFilteredSet = new Set(filteredTasks.value.map((t) => t.id));
    selectedTaskIds.value = selectedTaskIds.value.filter((id) => !currentFilteredSet.has(id));
  } else {
    const set = new Set(selectedTaskIds.value);
    filteredTasks.value.forEach((t) => set.add(t.id));
    selectedTaskIds.value = Array.from(set);
  }
}

function handleExport() {
  if (selectedTaskIds.value.length === 0) {
    message.warning('请至少选择一个需要导出的任务');
    return;
  }

  const selectedTasks = props.tasks.filter((t) => selectedTaskIds.value.includes(t.id));
  const payload = {
    easyjob_version: '0.1.0',
    exported_at: new Date().toISOString(),
    tasks: selectedTasks,
  };

  const jsonStr = JSON.stringify(payload, null, 2);
  const blob = new Blob([jsonStr], { type: 'application/json' });
  const url = URL.createObjectURL(blob);

  const timestamp = new Date().toISOString().replace(/[-:T]/g, '').slice(0, 14);
  const a = document.createElement('a');
  a.href = url;
  a.download = `easyjob-tasks-${timestamp}.json`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);

  message.success(`成功导出 ${selectedTasks.length} 个任务`);
  emit('update:show', false);
}
</script>

<template>
  <NModal
    :show="show"
    @update:show="$emit('update:show', $event)"
    preset="card"
    title="导出任务配置"
    style="width: 580px; max-width: 95vw;"
    size="small"
  >
    <div class="space-y-4">
      <div class="flex items-center justify-between gap-3">
        <NInput
          v-model:value="searchQuery"
          placeholder="搜索任务名称或描述..."
          size="small"
          clearable
          class="flex-1"
        >
          <template #prefix>
            <Search class="w-3.5 h-3.5 text-slate-400" />
          </template>
        </NInput>

        <NButton size="small" secondary @click="handleToggleSelectAll">
          <template #icon>
            <component :is="isAllSelected ? Square : CheckSquare" class="w-3.5 h-3.5" />
          </template>
          {{ isAllSelected ? '取消全选' : '全选' }}
        </NButton>
      </div>

      <div class="text-xs text-slate-500 dark:text-zinc-400 flex items-center justify-between px-1">
        <span>勾选需要导出的任务：</span>
        <span>已选中 {{ selectedTaskIds.length }} / {{ tasks.length }} 个任务</span>
      </div>

      <!-- Task Selection List -->
      <div class="max-h-80 overflow-y-auto border border-slate-200 dark:border-zinc-800 rounded-lg p-2 space-y-1">
        <div v-if="filteredTasks.length === 0" class="py-8">
          <NEmpty description="未找到匹配的任务" />
        </div>
        <div
          v-for="task in filteredTasks"
          :key="task.id"
          class="flex items-center justify-between p-2 rounded hover:bg-slate-50 dark:hover:bg-zinc-800/60 transition cursor-pointer"
          @click="
            selectedTaskIds.includes(task.id)
              ? (selectedTaskIds = selectedTaskIds.filter((id) => id !== task.id))
              : selectedTaskIds.push(task.id)
          "
        >
          <div class="flex items-center gap-3 overflow-hidden">
            <NCheckbox
              :checked="selectedTaskIds.includes(task.id)"
              @click.stop
              @update:checked="
                $event
                  ? selectedTaskIds.push(task.id)
                  : (selectedTaskIds = selectedTaskIds.filter((id) => id !== task.id))
              "
            />
            <div class="min-w-0">
              <div class="font-medium text-xs truncate text-slate-800 dark:text-zinc-200">
                {{ task.name }}
              </div>
              <div class="text-[11px] text-slate-400 dark:text-zinc-500 truncate mt-0.5">
                {{ task.description || '暂无描述' }}
              </div>
            </div>
          </div>

          <div class="flex items-center gap-1.5 shrink-0">
            <NTag size="tiny" :bordered="false" type="info">
              {{ task.triggers.length }} 触发器
            </NTag>
            <NTag size="tiny" :bordered="false" type="default">
              {{ task.actions.length }} 动作
            </NTag>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex justify-end gap-2">
        <NButton size="small" @click="$emit('update:show', false)">取消</NButton>
        <NButton
          size="small"
          type="primary"
          class="bg-emerald-600 hover:bg-emerald-500"
          :disabled="selectedTaskIds.length === 0"
          @click="handleExport"
        >
          <template #icon>
            <Download class="w-3.5 h-3.5" />
          </template>
          确认导出 ({{ selectedTaskIds.length }})
        </NButton>
      </div>
    </template>
  </NModal>
</template>
```

- [ ] **Step 4: 在 `apps/desktop/src/views/TasksView.vue` 接入导出按钮与弹窗**

在顶部“新建任务”左侧添加“导出任务”按钮，并引入 `TaskExportModal`：

```vue
<!-- 引入并在 header 右侧添加 -->
<NButton size="medium" secondary @click="showExportModal = true">
  <template #icon>
    <Download class="w-4 h-4 text-slate-500" />
  </template>
  导出任务
</NButton>
```

- [ ] **Step 5: 验证编译与测试**

运行：`pnpm -C apps/desktop test && pnpm -C apps/desktop run build`
预期：所有 Vitest 测试通过，Vue-TSC 0 错误。

- [ ] **Step 6: 提交代码**

```bash
git add apps/desktop/src/components/task/TaskExportModal.vue apps/desktop/src/views/TasksView.vue apps/desktop/tests/taskExport.test.ts
git commit -m "feat(desktop): implement TaskExportModal with multi-select and JSON download"
```

---

### Task 3: 三栏纵向手风琴合并弹窗与配置比对组件 (`TaskAccordionContent.vue` & `TaskMergeModal.vue`)

**Files:**
- Create: `apps/desktop/src/components/task/TaskAccordionContent.vue`
- Create: `apps/desktop/src/components/task/TaskMergeModal.vue`
- Create: `apps/desktop/tests/taskMerge.test.ts`

**Interfaces:**
- Consumes: `Task`, `TaskDiffResult`, `TriggerEditor.vue`, `ActionEditor.vue`
- Produces:
  - `TaskAccordionContent.vue`: 渲染五项纵向折叠面板（基本信息、策略、触发器、动作、环境变量），支持只读高亮模式与可编辑模式；
  - `TaskMergeModal.vue`: 三栏水平排列对比合并弹窗，支持 `[◀ 采用已有]` 与 `[◀ 还原导入]` 快捷按钮，确认合并并返回合并后 Task。

- [ ] **Step 1: 编写 `tests/taskMerge.test.ts` 测试**

```typescript
import { describe, it, expect } from 'vitest';
import { getEmptyTask } from '../src/types/task';
import { compareTasks } from '../src/utils/taskDiff';
import type { Task } from '../src/types/task';

describe('TaskMergeModal quick copy actions', () => {
  it('clones existing basic info into target task', () => {
    const existing: Task = {
      ...getEmptyTask(),
      name: 'Old Name',
      description: 'Old Desc',
      working_directory: '/old/dir',
      enabled: false,
    };
    const target: Task = {
      ...getEmptyTask(),
      name: 'New Name',
      description: 'New Desc',
      working_directory: '/new/dir',
      enabled: true,
    };

    // Simulate "采用已有配置 - 基本信息"
    target.description = existing.description;
    target.working_directory = existing.working_directory;
    target.enabled = existing.enabled;

    expect(target.description).toBe('Old Desc');
    expect(target.working_directory).toBe('/old/dir');
    expect(target.enabled).toBe(false);
  });

  it('clones existing policy into target task', () => {
    const existing = getEmptyTask();
    existing.execution_policy.timeout_secs = 120;
    existing.execution_policy.concurrency_policy = 'AllowParallel';

    const target = getEmptyTask();
    target.execution_policy = JSON.parse(JSON.stringify(existing.execution_policy));

    expect(target.execution_policy.timeout_secs).toBe(120);
    expect(target.execution_policy.concurrency_policy).toBe('AllowParallel');
  });

  it('clones existing triggers and actions into target task', () => {
    const existing = getEmptyTask();
    existing.triggers = [
      {
        id: 't-1',
        task_id: existing.id,
        enabled: true,
        kind: { Daily: { time: '08:00', timezone: 'UTC' } },
        created_at: '',
        updated_at: '',
      },
    ];
    existing.actions = [
      {
        id: 'a-1',
        task_id: existing.id,
        sequence: 0,
        enabled: true,
        kind: { ExecuteShell: { command: 'ls -la' } },
      },
    ];

    const target = getEmptyTask();
    target.triggers = JSON.parse(JSON.stringify(existing.triggers));
    target.actions = JSON.parse(JSON.stringify(existing.actions));

    expect(target.triggers).toHaveLength(1);
    expect(target.actions).toHaveLength(1);
    expect(target.actions[0].kind).toEqual({ ExecuteShell: { command: 'ls -la' } });
  });
});
```

- [ ] **Step 2: 运行测试验证**

运行：`pnpm -C apps/desktop vitest run tests/taskMerge.test.ts`
预期：PASS

- [ ] **Step 3: 创建 `apps/desktop/src/components/task/TaskAccordionContent.vue`**

封装纵向 `NCollapse`：
- Panel 1: 基本配置 (Basic Info)
- Panel 2: 执行策略 (Execution Policy)
- Panel 3: 触发规则 (Triggers)
- Panel 4: 执行动作 (Actions)
- Panel 5: 环境变量 (Environment)
根据 `props.editable` 切换展示：只读模式下展示清晰键值卡片，并依据 `props.diffFields` 进行黄色琥珀色高亮描边；编辑模式下展示输入控件与 `TriggerEditor` / `ActionEditor`。

```vue
<script setup lang="ts">
import { computed } from 'vue';
import {
  NCollapse,
  NCollapseItem,
  NTag,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NSwitch,
} from 'naive-ui';
import TriggerEditor from './TriggerEditor.vue';
import ActionEditor from './ActionEditor.vue';
import type { Task, ConcurrencyPolicy, MissedRunPolicy } from '../../types/task';
import { getTriggerType, getActionType } from '../../types/task';

const props = defineProps<{
  task: Task;
  editable: boolean;
  diffFields?: Set<string>;
  columnTitle: string;
}>();

const isDiff = (field: string) => props.diffFields?.has(field) ?? false;

const concurrencyOptions: { label: string; value: ConcurrencyPolicy }[] = [
  { label: '单例跳过 (SkipIfRunning)', value: 'SkipIfRunning' },
  { label: '允许并行 (AllowParallel)', value: 'AllowParallel' },
  { label: '至多排队一个 (QueueOne)', value: 'QueueOne' },
];

const missedRunOptions: { label: string; value: MissedRunPolicy }[] = [
  { label: '补跑一次 (RunOnce)', value: 'RunOnce' },
  { label: '直接跳过 (Skip)', value: 'Skip' },
];

const envEntries = computed({
  get: () => Object.entries(props.task.environment || {}),
  set: (val) => {
    const obj: Record<string, string> = {};
    val.forEach(([k, v]) => {
      if (k.trim()) obj[k.trim()] = v;
    });
    props.task.environment = obj;
  },
});
</script>

<template>
  <div class="h-full flex flex-col">
    <NCollapse default-expanded-names="['basic', 'policy', 'triggers', 'actions', 'env']">
      <!-- 1. 基本配置 -->
      <NCollapseItem title="基本配置" name="basic">
        <template #header-extra>
          <NTag
            v-if="!editable && (isDiff('basic.description') || isDiff('basic.enabled') || isDiff('basic.working_directory'))"
            size="tiny"
            type="warning"
          >
            有差异
          </NTag>
        </template>

        <div v-if="!editable" class="space-y-2 text-xs">
          <div :class="['p-2 rounded', isDiff('basic.description') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">描述：</span>
            <span class="font-medium text-slate-800 dark:text-zinc-200">{{ task.description || '无' }}</span>
          </div>
          <div :class="['p-2 rounded flex items-center justify-between', isDiff('basic.enabled') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">启用状态：</span>
            <NTag size="tiny" :type="task.enabled ? 'success' : 'default'">{{ task.enabled ? '启用' : '停用' }}</NTag>
          </div>
          <div :class="['p-2 rounded', isDiff('basic.working_directory') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">工作目录：</span>
            <span class="font-medium text-slate-800 dark:text-zinc-200">{{ task.working_directory || '默认' }}</span>
          </div>
        </div>

        <div v-else class="space-y-3 pt-1">
          <NForm label-placement="top" size="small">
            <NFormItem label="任务描述">
              <NInput v-model:value="task.description" type="textarea" :autosize="{ minRows: 2, maxRows: 3 }" />
            </NFormItem>
            <div class="flex items-center justify-between p-2 bg-slate-50 dark:bg-zinc-800/40 rounded mb-3">
              <span class="text-xs">任务启用状态</span>
              <NSwitch v-model:value="task.enabled" size="small" />
            </div>
            <NFormItem label="工作目录">
              <NInput v-model:value="task.working_directory" placeholder="默认运行路径" />
            </NFormItem>
          </NForm>
        </div>
      </NCollapseItem>

      <!-- 2. 执行策略 -->
      <NCollapseItem title="执行策略" name="policy">
        <template #header-extra>
          <NTag
            v-if="!editable && (isDiff('policy.concurrency_policy') || isDiff('policy.missed_run_policy') || isDiff('policy.timeout_secs') || isDiff('policy.retry_max_retries'))"
            size="tiny"
            type="warning"
          >
            有差异
          </NTag>
        </template>

        <div v-if="!editable" class="space-y-2 text-xs">
          <div :class="['p-2 rounded', isDiff('policy.concurrency_policy') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">并发策略：</span>
            <span class="font-medium">{{ task.execution_policy.concurrency_policy }}</span>
          </div>
          <div :class="['p-2 rounded', isDiff('policy.missed_run_policy') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">错失策略：</span>
            <span class="font-medium">{{ task.execution_policy.missed_run_policy }}</span>
          </div>
          <div :class="['p-2 rounded', isDiff('policy.timeout_secs') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">超时限制：</span>
            <span class="font-medium">{{ task.execution_policy.timeout_secs ? `${task.execution_policy.timeout_secs} 秒` : '无限制' }}</span>
          </div>
          <div :class="['p-2 rounded', isDiff('policy.retry_max_retries') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">重试策略：</span>
            <span class="font-medium">重试 {{ task.execution_policy.retry_policy.max_retries }} 次 (延迟 {{ task.execution_policy.retry_policy.delay_secs }} 秒)</span>
          </div>
        </div>

        <div v-else class="space-y-3 pt-1">
          <NForm label-placement="top" size="small">
            <NFormItem label="并发策略">
              <NSelect v-model:value="task.execution_policy.concurrency_policy" :options="concurrencyOptions" />
            </NFormItem>
            <NFormItem label="错失触发策略">
              <NSelect v-model:value="task.execution_policy.missed_run_policy" :options="missedRunOptions" />
            </NFormItem>
            <div class="grid grid-cols-2 gap-2">
              <NFormItem label="超时 (秒)">
                <NInputNumber v-model:value="task.execution_policy.timeout_secs" :min="1" placeholder="不限" class="w-full" />
              </NFormItem>
              <NFormItem label="重试次数">
                <NInputNumber v-model:value="task.execution_policy.retry_policy.max_retries" :min="0" class="w-full" />
              </NFormItem>
            </div>
          </NForm>
        </div>
      </NCollapseItem>

      <!-- 3. 触发规则 -->
      <NCollapseItem title="触发规则" name="triggers">
        <template #header-extra>
          <NTag v-if="!editable && isDiff('triggers')" size="tiny" type="warning">有差异</NTag>
          <span class="text-xs text-slate-400 ml-1.5">({{ task.triggers.length }} 个)</span>
        </template>

        <div v-if="!editable" class="space-y-1.5 text-xs">
          <div
            v-for="(tr, idx) in task.triggers"
            :key="idx"
            :class="['p-2 rounded border', isDiff('triggers') ? 'border-amber-300 dark:border-amber-800/60 bg-amber-50/40 dark:bg-amber-950/20' : 'border-slate-200 dark:border-zinc-800 bg-slate-50 dark:bg-zinc-800/40']"
          >
            <div class="flex items-center justify-between mb-1">
              <NTag size="tiny" type="info">{{ getTriggerType(tr.kind) }}</NTag>
              <span :class="tr.enabled ? 'text-emerald-500' : 'text-slate-400'">{{ tr.enabled ? '启用' : '停用' }}</span>
            </div>
            <div class="text-[11px] text-slate-600 dark:text-zinc-300 font-mono truncate">
              {{ JSON.stringify(tr.kind) }}
            </div>
          </div>
          <div v-if="task.triggers.length === 0" class="text-slate-400 text-xs py-2 text-center">无触发器</div>
        </div>

        <div v-else>
          <TriggerEditor v-model:triggers="task.triggers" />
        </div>
      </NCollapseItem>

      <!-- 4. 执行动作 -->
      <NCollapseItem title="执行动作" name="actions">
        <template #header-extra>
          <NTag v-if="!editable && isDiff('actions')" size="tiny" type="warning">有差异</NTag>
          <span class="text-xs text-slate-400 ml-1.5">({{ task.actions.length }} 个)</span>
        </template>

        <div v-if="!editable" class="space-y-1.5 text-xs">
          <div
            v-for="(act, idx) in task.actions"
            :key="idx"
            :class="['p-2 rounded border', isDiff('actions') ? 'border-amber-300 dark:border-amber-800/60 bg-amber-50/40 dark:bg-amber-950/20' : 'border-slate-200 dark:border-zinc-800 bg-slate-50 dark:bg-zinc-800/40']"
          >
            <div class="flex items-center justify-between mb-1">
              <NTag size="tiny" type="default">步骤 {{ idx + 1 }}: {{ getActionType(act.kind) }}</NTag>
              <span :class="act.enabled ? 'text-emerald-500' : 'text-slate-400'">{{ act.enabled ? '启用' : '停用' }}</span>
            </div>
            <div class="text-[11px] text-slate-600 dark:text-zinc-300 font-mono truncate">
              {{ JSON.stringify(act.kind) }}
            </div>
          </div>
          <div v-if="task.actions.length === 0" class="text-slate-400 text-xs py-2 text-center">无执行动作</div>
        </div>

        <div v-else>
          <ActionEditor v-model:actions="task.actions" />
        </div>
      </NCollapseItem>

      <!-- 5. 环境变量 -->
      <NCollapseItem title="环境变量" name="env">
        <template #header-extra>
          <span class="text-xs text-slate-400 ml-1.5">({{ Object.keys(task.environment || {}).length }} 个)</span>
        </template>

        <div v-if="!editable" class="space-y-1 text-xs">
          <div
            v-for="(val, key) in task.environment || {}"
            :key="key"
            :class="['p-1.5 rounded flex justify-between font-mono text-[11px]', isDiff(`env.${key}`) ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']"
          >
            <span class="text-slate-500">{{ key }}</span>
            <span class="text-slate-800 dark:text-zinc-200">{{ val }}</span>
          </div>
          <div v-if="Object.keys(task.environment || {}).length === 0" class="text-slate-400 text-xs py-2 text-center">无环境变量</div>
        </div>

        <div v-else class="space-y-2">
          <div
            v-for="([k, v], idx) in envEntries"
            :key="idx"
            class="flex items-center gap-2"
          >
            <NInput :value="k" size="small" placeholder="KEY" @update:value="envEntries[idx][0] = $event" />
            <NInput :value="v" size="small" placeholder="VALUE" @update:value="envEntries[idx][1] = $event" />
            <NButton size="tiny" secondary type="error" @click="envEntries.splice(idx, 1)">×</NButton>
          </div>
          <NButton size="tiny" secondary @click="envEntries.push(['', ''])">+ 添加环境变量</NButton>
        </div>
      </NCollapseItem>
    </NCollapse>
  </div>
</template>
```

- [ ] **Step 4: 创建 `apps/desktop/src/components/task/TaskMergeModal.vue`**

构建三栏水平排列布局：
- 顶部：显示当前冲突任务名称与合并提示；
- 中间三栏（各占 1/3 宽度）：
  - Col 1: 已有任务（只读）
  - Col 2: 导入任务（只读 + `diffFields` 高亮）
  - Col 3: 合并最终结果（可编辑表单，顶部提供 `[◀ 采用已有]` / `[◀ 还原导入]` 各模块快捷按钮）
- 底部：【取消】与【确认合并此任务】。

```vue
<script setup lang="ts">
import { ref, watch, computed } from 'vue';
import { NModal, NCard, NButton, NTag, useMessage } from 'naive-ui';
import { GitMerge, Check } from 'lucide-vue-next';
import TaskAccordionContent from './TaskAccordionContent.vue';
import type { Task } from '../../types/task';
import { compareTasks } from '../../utils/taskDiff';

const props = defineProps<{
  show: boolean;
  existingTask: Task | null;
  importedTask: Task | null;
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
  (e: 'merged', task: Task): void;
}>();

const message = useMessage();
const mergedTask = ref<Task | null>(null);

watch(
  () => [props.show, props.importedTask],
  ([show]) => {
    if (show && props.importedTask) {
      mergedTask.value = JSON.parse(JSON.stringify(props.importedTask));
    }
  },
  { immediate: true }
);

const diffResult = computed(() => {
  if (!props.existingTask || !props.importedTask) {
    return {
      hasDiff: false,
      basicDiff: false,
      policyDiff: false,
      triggersDiff: false,
      actionsDiff: false,
      envDiff: false,
      diffFields: new Set<string>(),
    };
  }
  return compareTasks(props.existingTask, props.importedTask);
});

// 块级一键采用已有配置
function copyExistingSection(section: 'basic' | 'policy' | 'triggers' | 'actions' | 'env') {
  if (!props.existingTask || !mergedTask.value) return;
  const src = props.existingTask;
  const dst = mergedTask.value;

  if (section === 'basic') {
    dst.description = src.description;
    dst.enabled = src.enabled;
    dst.working_directory = src.working_directory;
  } else if (section === 'policy') {
    dst.execution_policy = JSON.parse(JSON.stringify(src.execution_policy));
  } else if (section === 'triggers') {
    dst.triggers = JSON.parse(JSON.stringify(src.triggers));
  } else if (section === 'actions') {
    dst.actions = JSON.parse(JSON.stringify(src.actions));
  } else if (section === 'env') {
    dst.environment = JSON.parse(JSON.stringify(src.environment || {}));
  }
  message.success('已应用已有配置到最终结果');
}

// 块级一键还原导入配置
function restoreImportedSection(section: 'basic' | 'policy' | 'triggers' | 'actions' | 'env') {
  if (!props.importedTask || !mergedTask.value) return;
  const src = props.importedTask;
  const dst = mergedTask.value;

  if (section === 'basic') {
    dst.description = src.description;
    dst.enabled = src.enabled;
    dst.working_directory = src.working_directory;
  } else if (section === 'policy') {
    dst.execution_policy = JSON.parse(JSON.stringify(src.execution_policy));
  } else if (section === 'triggers') {
    dst.triggers = JSON.parse(JSON.stringify(src.triggers));
  } else if (section === 'actions') {
    dst.actions = JSON.parse(JSON.stringify(src.actions));
  } else if (section === 'env') {
    dst.environment = JSON.parse(JSON.stringify(src.environment || {}));
  }
  message.success('已还原导入配置到最终结果');
}

function handleConfirmMerge() {
  if (!mergedTask.value) return;
  emit('merged', mergedTask.value);
  emit('update:show', false);
  message.success('任务合并配置已保存');
}
</script>

<template>
  <NModal
    :show="show"
    @update:show="$emit('update:show', $event)"
    preset="card"
    title="手动合并任务配置差异"
    style="width: 94vw; max-width: 1400px; height: 90vh;"
    size="small"
  >
    <template #header-extra>
      <span class="text-xs text-slate-500">
        任务名称: <strong class="text-slate-800 dark:text-zinc-200">{{ existingTask?.name }}</strong>
      </span>
    </template>

    <div v-if="existingTask && importedTask && mergedTask" class="grid grid-cols-3 gap-4 h-[75vh] overflow-hidden">
      <!-- Column 1: 已有配置 (只读) -->
      <div class="flex flex-col h-full border border-slate-200 dark:border-zinc-800 rounded-lg p-3 bg-slate-50/50 dark:bg-zinc-900/50 overflow-y-auto">
        <div class="flex items-center justify-between pb-2 mb-2 border-b border-slate-200 dark:border-zinc-800">
          <span class="font-bold text-xs text-slate-700 dark:text-zinc-300">第一栏：已有任务配置</span>
          <NTag size="tiny" type="default">只读</NTag>
        </div>
        <TaskAccordionContent
          :task="existingTask"
          :editable="false"
          column-title="已有任务"
        />
      </div>

      <!-- Column 2: 导入配置 (只读 + 高亮) -->
      <div class="flex flex-col h-full border border-amber-200 dark:border-amber-900/50 rounded-lg p-3 bg-amber-50/20 dark:bg-amber-950/10 overflow-y-auto">
        <div class="flex items-center justify-between pb-2 mb-2 border-b border-amber-200/60 dark:border-amber-900/50">
          <div class="flex items-center gap-1.5">
            <span class="font-bold text-xs text-slate-700 dark:text-zinc-300">第二栏：导入任务配置</span>
            <NTag v-if="diffResult.hasDiff" size="tiny" type="warning">含差异高亮</NTag>
          </div>
          <NTag size="tiny" type="default">只读</NTag>
        </div>
        <TaskAccordionContent
          :task="importedTask"
          :editable="false"
          :diff-fields="diffResult.diffFields"
          column-title="导入任务"
        />
      </div>

      <!-- Column 3: 最终合并结果 (可编辑) -->
      <div class="flex flex-col h-full border-2 border-emerald-500/40 dark:border-emerald-500/30 rounded-lg p-3 bg-white dark:bg-zinc-900 overflow-y-auto shadow-sm">
        <div class="flex items-center justify-between pb-2 mb-2 border-b border-slate-200 dark:border-zinc-800">
          <div class="flex items-center gap-1.5">
            <span class="font-bold text-xs text-emerald-600 dark:text-emerald-400">第三栏：最终合并结果</span>
            <NTag size="tiny" type="success">可编辑</NTag>
          </div>
          <div class="flex items-center gap-1">
            <NButton size="tiny" secondary @click="mergedTask = JSON.parse(JSON.stringify(existingTask!))">
              全部采用已有
            </NButton>
            <NButton size="tiny" secondary @click="mergedTask = JSON.parse(JSON.stringify(importedTask!))">
              全部还原导入
            </NButton>
          </div>
        </div>

        <!-- 快捷操作工具条 -->
        <div class="bg-slate-100 dark:bg-zinc-800/80 p-1.5 rounded flex items-center justify-between text-[11px] mb-2">
          <span class="text-slate-500">块级快捷操作：</span>
          <div class="flex gap-1">
            <NButton size="tiny" text type="primary" @click="copyExistingSection('basic')">◀ 基本信息</NButton>
            <NButton size="tiny" text type="primary" @click="copyExistingSection('policy')">◀ 策略</NButton>
            <NButton size="tiny" text type="primary" @click="copyExistingSection('triggers')">◀ 触发器</NButton>
            <NButton size="tiny" text type="primary" @click="copyExistingSection('actions')">◀ 动作</NButton>
            <NButton size="tiny" text type="primary" @click="copyExistingSection('env')">◀ 环境</NButton>
          </div>
        </div>

        <TaskAccordionContent
          :task="mergedTask"
          :editable="true"
          column-title="合并结果"
        />
      </div>
    </div>

    <template #footer>
      <div class="flex justify-end gap-2">
        <NButton size="small" @click="$emit('update:show', false)">取消</NButton>
        <NButton
          size="small"
          type="primary"
          class="bg-emerald-600 hover:bg-emerald-500"
          @click="handleConfirmMerge"
        >
          <template #icon>
            <Check class="w-3.5 h-3.5" />
          </template>
          确认合并此任务
        </NButton>
      </div>
    </template>
  </NModal>
</template>
```

- [ ] **Step 5: 验证编译与测试**

运行：`pnpm -C apps/desktop test && pnpm -C apps/desktop run build`
预期：所有 Vitest 测试通过，Vue-TSC 0 错误。

- [ ] **Step 6: 提交代码**

```bash
git add apps/desktop/src/components/task/TaskAccordionContent.vue apps/desktop/src/components/task/TaskMergeModal.vue apps/desktop/tests/taskMerge.test.ts
git commit -m "feat(desktop): implement 3-column accordion TaskMergeModal with diff highlights"
```

---

### Task 4: 任务文件导入与冲突处理确认弹窗 (`TaskImportModal.vue`) & 全面串联

**Files:**
- Create: `apps/desktop/src/components/task/TaskImportModal.vue`
- Modify: `apps/desktop/src/views/TasksView.vue`
- Create: `apps/desktop/tests/taskImport.test.ts`

**Interfaces:**
- Consumes: `Task`, `TaskExportPayload`, `TaskMergeModal.vue`
- Produces: `TaskImportModal.vue` 处理文件导入、同名冲突识别、单项覆盖/跳过/手动合并、批量操作与入库保存。

- [ ] **Step 1: 编写 `tests/taskImport.test.ts` 测试**

```typescript
import { describe, it, expect, beforeEach } from 'vitest';
import { getEmptyTask } from '../src/types/task';
import type { Task } from '../src/types/task';

describe('TaskImport conflict detection logic', () => {
  it('categorizes imported tasks into new vs conflict correctly', () => {
    const existingTask = { ...getEmptyTask(), id: 'local-1', name: 'Database Backup' };
    const existingList = [existingTask];

    const importedTask1 = { ...getEmptyTask(), id: 'ext-1', name: 'Database Backup' };
    const importedTask2 = { ...getEmptyTask(), id: 'ext-2', name: 'Log Archiver' };
    const importedList = [importedTask1, importedTask2];

    const existingNames = new Set(existingList.map((t) => t.name));

    const conflicts = importedList.filter((t) => existingNames.has(t.name));
    const nonConflicts = importedList.filter((t) => !existingNames.has(t.name));

    expect(conflicts).toHaveLength(1);
    expect(conflicts[0].name).toBe('Database Backup');
    expect(nonConflicts).toHaveLength(1);
    expect(nonConflicts[0].name).toBe('Log Archiver');
  });

  it('re-generates fresh UUIDs for newly imported non-conflicting tasks', () => {
    const imported = getEmptyTask();
    imported.id = 'fixed-old-id';
    imported.triggers = [
      {
        id: 'old-tr',
        task_id: 'fixed-old-id',
        enabled: true,
        kind: 'AgentStarted',
        created_at: '',
        updated_at: '',
      },
    ];

    // Simulate UUID regeneration
    const newTaskId = 'new-task-uuid-1';
    const prepared = {
      ...imported,
      id: newTaskId,
      version: 1,
      triggers: imported.triggers.map((tr) => ({
        ...tr,
        id: 'new-tr-uuid-1',
        task_id: newTaskId,
      })),
    };

    expect(prepared.id).not.toBe('fixed-old-id');
    expect(prepared.triggers[0].task_id).toBe(newTaskId);
  });
});
```

- [ ] **Step 2: 运行测试验证**

运行：`pnpm -C apps/desktop vitest run tests/taskImport.test.ts`
预期：PASS

- [ ] **Step 3: 创建 `apps/desktop/src/components/task/TaskImportModal.vue`**

```vue
<script setup lang="ts">
import { ref, computed } from 'vue';
import {
  NModal,
  NButton,
  NTag,
  NRadioGroup,
  NRadio,
  NEmpty,
  useMessage,
} from 'naive-ui';
import { UploadCloud, FileText, Check, AlertTriangle } from 'lucide-vue-next';
import TaskMergeModal from './TaskMergeModal.vue';
import type { Task } from '../../types/task';
import { useTaskStore } from '../../stores/taskStore';

const props = defineProps<{
  show: boolean;
  existingTasks: Task[];
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
  (e: 'imported'): void;
}>();

const taskStore = useTaskStore();
const message = useMessage();

interface ImportItem {
  task: Task;
  isConflict: boolean;
  existingMatch?: Task;
  action: 'create' | 'overwrite' | 'skip' | 'merged';
  mergedTask?: Task;
}

const importItems = ref<ImportItem[]>([]);
const hasFile = ref(false);
const fileName = ref('');
const showMergeModal = ref(false);
const activeMergingItem = ref<ImportItem | null>(null);

function handleFileChange(event: Event) {
  const target = event.target as HTMLInputElement;
  const file = target.files?.[0];
  if (!file) return;

  fileName.value = file.name;
  const reader = new FileReader();
  reader.onload = (e) => {
    try {
      const content = e.target?.result as string;
      const parsed = JSON.parse(content);
      const rawTasks: Task[] = Array.isArray(parsed)
        ? parsed
        : Array.isArray(parsed?.tasks)
        ? parsed.tasks
        : [];

      if (rawTasks.length === 0) {
        message.warning('文件格式正确，但未找到可导入的任务数据');
        return;
      }

      const existingMap = new Map<string, Task>();
      props.existingTasks.forEach((t) => existingMap.set(t.name, t));

      importItems.value = rawTasks.map((t) => {
        const existing = existingMap.get(t.name);
        if (existing) {
          return {
            task: t,
            isConflict: true,
            existingMatch: existing,
            action: 'overwrite', // 默认推荐覆盖
          };
        }
        return {
          task: t,
          isConflict: false,
          action: 'create',
        };
      });

      hasFile.value = true;
    } catch (err: any) {
      message.error('解析 JSON 失败: ' + (err?.message || err));
    }
  };
  reader.readAsText(file);
}

function openManualMerge(item: ImportItem) {
  activeMergingItem.value = item;
  showMergeModal.value = true;
}

function handleMergeResolved(merged: Task) {
  if (activeMergingItem.value) {
    activeMergingItem.value.mergedTask = merged;
    activeMergingItem.value.action = 'merged';
  }
}

function setAllConflictAction(act: 'overwrite' | 'skip') {
  importItems.value.forEach((item) => {
    if (item.isConflict) {
      item.action = act;
    }
  });
}

async function handleConfirmImport() {
  const toSave: Task[] = [];

  for (const item of importItems.value) {
    if (item.action === 'skip') continue;

    if (item.action === 'create') {
      const newId = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'task-' + Math.random().toString(36).substring(2, 9);
      const cloned = JSON.parse(JSON.stringify(item.task)) as Task;
      cloned.id = newId;
      cloned.version = 1;
      cloned.triggers.forEach((tr) => {
        tr.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'tr-' + Math.random().toString(36).substring(2, 9);
        tr.task_id = newId;
      });
      cloned.actions.forEach((act) => {
        act.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'act-' + Math.random().toString(36).substring(2, 9);
        act.task_id = newId;
      });
      toSave.push(cloned);
    } else if (item.action === 'overwrite' && item.existingMatch) {
      const existingId = item.existingMatch.id;
      const cloned = JSON.parse(JSON.stringify(item.task)) as Task;
      cloned.id = existingId;
      cloned.version = (item.existingMatch.version || 1) + 1;
      cloned.triggers.forEach((tr) => {
        tr.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'tr-' + Math.random().toString(36).substring(2, 9);
        tr.task_id = existingId;
      });
      cloned.actions.forEach((act) => {
        act.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'act-' + Math.random().toString(36).substring(2, 9);
        act.task_id = existingId;
      });
      toSave.push(cloned);
    } else if (item.action === 'merged' && item.existingMatch && item.mergedTask) {
      const existingId = item.existingMatch.id;
      const cloned = JSON.parse(JSON.stringify(item.mergedTask)) as Task;
      cloned.id = existingId;
      cloned.version = (item.existingMatch.version || 1) + 1;
      cloned.triggers.forEach((tr) => {
        tr.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'tr-' + Math.random().toString(36).substring(2, 9);
        tr.task_id = existingId;
      });
      cloned.actions.forEach((act) => {
        act.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'act-' + Math.random().toString(36).substring(2, 9);
        act.task_id = existingId;
      });
      toSave.push(cloned);
    }
  }

  if (toSave.length === 0) {
    message.info('未选择任何需导入的任务');
    emit('update:show', false);
    return;
  }

  try {
    for (const t of toSave) {
      await taskStore.saveTask(t);
    }
    message.success(`成功导入并更新 ${toSave.length} 个任务`);
    emit('imported');
    emit('update:show', false);
  } catch (e: any) {
    message.error('导入保存失败: ' + (e?.message || e));
  }
}
</script>

<template>
  <NModal
    :show="show"
    @update:show="$emit('update:show', $event)"
    preset="card"
    title="导入任务配置"
    style="width: 720px; max-width: 95vw;"
    size="small"
  >
    <!-- 步骤 1: 待选文件 -->
    <div v-if="!hasFile" class="py-8 flex flex-col items-center justify-center border-2 border-dashed border-slate-200 dark:border-zinc-800 rounded-xl hover:border-emerald-500/60 transition cursor-pointer relative">
      <input
        type="file"
        accept=".json"
        class="absolute inset-0 opacity-0 cursor-pointer"
        @change="handleFileChange"
      />
      <UploadCloud class="w-12 h-12 text-slate-400 mb-2" />
      <span class="text-sm font-semibold text-slate-700 dark:text-zinc-200">点击选择或拖拽 JSON 任务文件</span>
      <span class="text-xs text-slate-400 dark:text-zinc-500 mt-1">支持 easyJob 导出的任务配置包</span>
    </div>

    <!-- 步骤 2: 解析与冲突确认列表 -->
    <div v-else class="space-y-4">
      <div class="flex items-center justify-between text-xs px-1">
        <div class="flex items-center gap-2">
          <FileText class="w-4 h-4 text-emerald-600" />
          <span class="font-medium">{{ fileName }}</span>
          <span class="text-slate-400">({{ importItems.length }} 个任务)</span>
        </div>

        <div class="flex items-center gap-1.5">
          <span class="text-slate-400">同名批量:</span>
          <NButton size="tiny" secondary @click="setAllConflictAction('overwrite')">全部覆盖</NButton>
          <NButton size="tiny" secondary @click="setAllConflictAction('skip')">全部跳过</NButton>
        </div>
      </div>

      <div class="max-h-96 overflow-y-auto border border-slate-200 dark:border-zinc-800 rounded-lg divide-y divide-slate-100 dark:divide-zinc-800/80">
        <div
          v-for="(item, idx) in importItems"
          :key="idx"
          class="p-3 flex items-center justify-between gap-4 hover:bg-slate-50 dark:hover:bg-zinc-800/40 transition text-xs"
        >
          <div class="space-y-1 min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="font-semibold text-slate-800 dark:text-zinc-200 truncate">{{ item.task.name }}</span>
              <NTag v-if="!item.isConflict" size="tiny" type="success" :bordered="false">新增</NTag>
              <NTag v-else size="tiny" type="warning" :bordered="false">同名冲突</NTag>
            </div>
            <div class="text-slate-400 text-[11px] truncate">
              {{ item.task.description || '暂无描述' }} · {{ item.task.triggers.length }} 触发器 · {{ item.task.actions.length }} 动作
            </div>
          </div>

          <!-- 操作选项 -->
          <div class="shrink-0 flex items-center gap-2">
            <div v-if="item.isConflict" class="flex items-center gap-2">
              <NRadioGroup v-model:value="item.action" size="small">
                <NRadio value="overwrite">覆盖</NRadio>
                <NRadio value="skip">跳过</NRadio>
                <NRadio value="merged" :disabled="!item.mergedTask">已合并</NRadio>
              </NRadioGroup>

              <NButton size="tiny" secondary type="primary" @click="openManualMerge(item)">
                {{ item.mergedTask ? '重新合并' : '手动合并' }}
              </NButton>
            </div>
            <div v-else>
              <NTag size="tiny" type="info">就绪导入</NTag>
            </div>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex justify-between items-center">
        <NButton v-if="hasFile" size="small" text @click="hasFile = false; importItems = []">重新选择文件</NButton>
        <div v-else></div>

        <div class="flex gap-2">
          <NButton size="small" @click="$emit('update:show', false)">取消</NButton>
          <NButton
            size="small"
            type="primary"
            class="bg-emerald-600 hover:bg-emerald-500"
            :disabled="!hasFile || importItems.length === 0"
            @click="handleConfirmImport"
          >
            确认导入
          </NButton>
        </div>
      </div>
    </template>
  </NModal>

  <!-- 手动合并弹窗 -->
  <TaskMergeModal
    v-model:show="showMergeModal"
    :existing-task="activeMergingItem?.existingMatch || null"
    :imported-task="activeMergingItem?.task || null"
    @merged="handleMergeResolved"
  />
</template>
```

- [ ] **Step 4: 在 `apps/desktop/src/views/TasksView.vue` 接入导入按钮与弹窗**

在顶部“导出任务”右侧添加“导入任务”按钮并引入 `TaskImportModal`：

```vue
<NButton size="medium" secondary @click="showImportModal = true">
  <template #icon>
    <Upload class="w-4 h-4 text-slate-500" />
  </template>
  导入任务
</NButton>
```

- [ ] **Step 5: 运行前端全套测试与打包构建**

运行：`pnpm -C apps/desktop test && pnpm -C apps/desktop run build`
预期：所有 Vitest 测试通过，Vue-TSC 0 错误。

- [ ] **Step 6: 提交代码**

```bash
git add apps/desktop/src/components/task/TaskImportModal.vue apps/desktop/src/views/TasksView.vue apps/desktop/tests/taskImport.test.ts
git commit -m "feat(desktop): implement TaskImportModal with conflict detection and merge modal binding"
```

---

### Task 5: 全工作区集成、打包与端到端测试验证

**Files:**
- Test: All tests across workspace

- [ ] **Step 1: 运行后端与集成测试**

运行：`cargo test --all`
预期：79/79 passed

- [ ] **Step 2: 运行前端测试**

运行：`pnpm -C apps/desktop test`
预期：全部 Vitest 测试（95+ passed）

- [ ] **Step 3: 运行代码规范检查**

运行：
`cargo clippy --workspace --all-targets -- -D warnings`
`cargo fmt --check`
`pnpm -C apps/desktop run build`
预期：0 警告，0 错误。

- [ ] **Step 4: 提交合并**

```bash
git commit --allow-empty -m "chore(release): task import export and 3-column accordion merge complete"
```

---

## Plan Self-Review Checklist

1. **Spec Coverage**:
   - 勾选批量导出（JSON）: Task 2 完整覆盖。
   - 导入解析与同名冲突选项（覆盖/跳过/手动合并）: Task 4 完整覆盖。
   - 三栏展示与第二栏差异高亮: Task 1 (Diff 计算) & Task 3 (三栏手风琴与高亮) 完整覆盖。
   - 纵向手风琴布局（替代原有 TAB）: Task 3 `TaskAccordionContent` 完整覆盖。
   - 第三栏可编辑、默认展示导入任务、块级一键采用按钮: Task 3 完整覆盖。
2. **Placeholder Scan**: 0 个 TODO/TBD，全部代码完整。
3. **Type Consistency**: 严格使用 `Task`, `Trigger`, `Action`, `ExecutionPolicy`。
