<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { NButton, NInput, NSwitch, NTag, NEmpty, useMessage, useDialog } from 'naive-ui';
import { Plus, Search, Play, Edit2, Trash2, Download, Upload, Copy, RefreshCw } from 'lucide-vue-next';
import TaskDrawer from '../components/task/TaskDrawer.vue';
import LiveLogDrawer from '../components/console/LiveLogDrawer.vue';
import TaskExportModal from '../components/task/TaskExportModal.vue';
import TaskImportModal from '../components/task/TaskImportModal.vue';
import { useTaskStore } from '../stores/taskStore';
import { useExecutionStore } from '../stores/executionStore';
import type { Task, Trigger } from '../types/task';
import { cloneTaskForDuplicate, describeTriggerShort, formatDateTimeShort, formatDateTimeTitle, getTriggerType, parseDate } from '../types/task';
import { getStatusLabel } from '../types/execution';

const emit = defineEmits<{
  (e: 'create-task'): void;
  (e: 'edit-task', task: Task): void;
}>();

const taskStore = useTaskStore();
const executionStore = useExecutionStore();
const showDrawer = ref(false);
const showLogDrawer = ref(false);
const showExportModal = ref(false);
const showImportModal = ref(false);
const selectedExecutionId = ref<string | null>(null);
const editingTask = ref<Task | null>(null);
const drawerMode = ref<'create' | 'edit' | 'copy'>('create');

let message: { success: (msg: string) => void; error: (msg: string) => void };
try {
  message = useMessage();
} catch {
  message = {
    success: (msg: string) => console.log(msg),
    error: (msg: string) => console.error(msg),
  };
}

let dialog: ReturnType<typeof useDialog> | null = null;
try {
  dialog = useDialog();
} catch {
  dialog = null;
}

onMounted(() => {
  taskStore.loadTasks();
  taskStore.initOverviewListener();
});

function openCreateDrawer() {
  drawerMode.value = 'create';
  editingTask.value = null;
  showDrawer.value = true;
  emit('create-task');
}

function openEditDrawer(task: Task) {
  drawerMode.value = 'edit';
  editingTask.value = JSON.parse(JSON.stringify(task));
  showDrawer.value = true;
  emit('edit-task', task);
}

// 深拷贝副本，点「保存任务」后才通过 task.save 落库；取消不产生任何数据
function openCopyDrawer(task: Task) {
  drawerMode.value = 'copy';
  editingTask.value = cloneTaskForDuplicate(task);
  showDrawer.value = true;
}

async function handleToggleEnabled(task: Task, enabled: boolean) {
  try {
    const updated = { ...task, enabled };
    await taskStore.saveTask(updated);
    task.enabled = enabled;
    await taskStore.loadOverview();
    message.success(enabled ? '任务已启用' : '任务已禁用');
  } catch (e: any) {
    message.error('操作失败: ' + (e?.message || e));
  }
}

async function handleTrigger(task: Task) {
  try {
    // triggerTask now synchronously creates the Execution on the backend and returns it,
    // so we get the execution ID immediately (~2ms) with no race conditions.
    const exec = await taskStore.triggerTask(task.id);

    // Seed the executionStore cache with the execution, automatically merging
    // any early finished event if the task completed before RPC return
    executionStore.addExecution(exec);

    selectedExecutionId.value = exec.id;
    showLogDrawer.value = true;

    message.success(`已下发执行指令: ${task.name}`);
  } catch (e: any) {
    message.error('触发失败: ' + (e?.message || e));
  }
}

async function handleDelete(task: Task) {
  if (dialog) {
    dialog.warning({
      title: '确认删除任务',
      content: `确定要删除任务 "${task.name}" 吗？此操作将彻底移除该任务及其所有调度规则。`,
      positiveText: '确认删除',
      negativeText: '取消',
      onPositiveClick: async () => {
        try {
          await taskStore.deleteTask(task.id);
          message.success('任务已删除');
        } catch (e: any) {
          message.error('删除失败: ' + (e?.message || e));
        }
      },
    });
  } else {
    if (confirm(`确认删除任务 "${task.name}" 吗？`)) {
      try {
        await taskStore.deleteTask(task.id);
        message.success('任务已删除');
      } catch (e: any) {
        message.error('删除失败: ' + (e?.message || e));
      }
    }
  }
}

const rerollingKey = ref<string | null>(null);

function triggerNextFire(taskId: string, triggerId: string): string | null {
  const entry = taskStore.scheduleOverview[taskId];
  return entry?.triggers.find((trigger) => trigger.trigger_id === triggerId)?.next_fire_at ?? null;
}

// 事件类触发器由事件驱动（网络变动 / agent 启动），后端永远算不出下次时间
function isEventTrigger(trigger: Trigger): boolean {
  const type = getTriggerType(trigger.kind);
  return type === 'Network' || type === 'AgentStarted';
}

function isFuzzyTrigger(trigger: Trigger): boolean {
  return getTriggerType(trigger.kind) === 'Fuzzy';
}

function schedulableTriggers(task: Task): Trigger[] {
  return task.triggers.filter((trigger) => !isEventTrigger(trigger));
}

// 多个触发器时只关心最近的一次；没有任何排定值（如已停用、已过期的单次）则返回 null
function nearestNextFireIso(task: Task): string | null {
  let nearestIso: string | null = null;
  let nearestMs: number | null = null;
  for (const trigger of schedulableTriggers(task)) {
    const iso = triggerNextFire(task.id, trigger.id);
    const ms = parseDate(iso);
    if (ms === null) continue;
    if (nearestMs === null || ms < nearestMs) {
      nearestMs = ms;
      nearestIso = iso;
    }
  }
  return nearestIso;
}

// 返回 null 表示整个「下次」片段都不显示（该任务只有事件类触发器）
function nextFireText(task: Task): string | null {
  if (schedulableTriggers(task).length === 0) return null;
  const iso = nearestNextFireIso(task);
  return `下次 ${iso ? formatDateTimeShort(iso) : '—'}`;
}

function nextFireTitle(task: Task): string {
  const iso = nearestNextFireIso(task);
  return iso ? `下次 ${formatDateTimeTitle(iso)}` : '当前没有排定的下次触发时间';
}

async function handleReroll(task: Task, trigger: Trigger) {
  const key = `${task.id}:${trigger.id}`;
  rerollingKey.value = key;
  try {
    await taskStore.rerollTrigger(task.id, trigger.id);
  } catch (e: any) {
    message.error('重摇失败: ' + (e?.message || e));
  } finally {
    rerollingKey.value = null;
  }
}

function lastRunText(taskId: string): string {
  const lastRun = taskStore.scheduleOverview[taskId]?.last_run;
  if (!lastRun) return '上次 —';
  return `上次 ${formatDateTimeShort(lastRun.started_at)} ${getStatusLabel(lastRun.status)}`;
}

function lastRunTitle(taskId: string): string {
  const lastRun = taskStore.scheduleOverview[taskId]?.last_run;
  return lastRun ? formatDateTimeTitle(lastRun.started_at) : '从未执行';
}

function lastRunClass(taskId: string): string {
  const status = taskStore.scheduleOverview[taskId]?.last_run?.status;
  return status === 'Failed' || status === 'TimedOut' || status === 'Interrupted'
    ? 'text-red-500'
    : 'text-slate-500 dark:text-zinc-400';
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

        <NButton size="medium" secondary @click="showExportModal = true">
          <template #icon>
            <Download class="w-4 h-4 text-slate-500" />
          </template>
          导出任务
        </NButton>

        <NButton size="medium" secondary @click="showImportModal = true">
          <template #icon>
            <Upload class="w-4 h-4 text-slate-500" />
          </template>
          导入任务
        </NButton>

        <NButton
          type="primary"
          size="medium"
          class="bg-emerald-600 hover:bg-emerald-500"
          @click="openCreateDrawer"
        >
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
              <NTag size="small" :bordered="false" type="default">
                {{ task.actions.length }} 个动作
              </NTag>
            </div>
            <div
              v-if="task.triggers.length > 0"
              class="flex flex-wrap items-center gap-1.5 text-xs text-slate-500 dark:text-zinc-400 mt-1"
            >
              <template v-for="(trigger, index) in task.triggers" :key="trigger.id">
                <span v-if="index > 0" class="text-slate-400 dark:text-zinc-600">·</span>
                <span>{{ describeTriggerShort(trigger.kind) }}</span>
                <NButton
                  v-if="isFuzzyTrigger(trigger)"
                  size="tiny"
                  secondary
                  :loading="rerollingKey === `${task.id}:${trigger.id}`"
                  @click="handleReroll(task, trigger)"
                >
                  <template #icon>
                    <RefreshCw class="w-3 h-3" />
                  </template>
                </NButton>
              </template>
            </div>
            <div class="flex items-center gap-1.5 text-xs mt-1">
              <span :class="lastRunClass(task.id)" :title="lastRunTitle(task.id)">
                {{ lastRunText(task.id) }}
              </span>
              <template v-if="nextFireText(task)">
                <span class="text-slate-400 dark:text-zinc-600">·</span>
                <span class="text-slate-500 dark:text-zinc-400" :title="nextFireTitle(task)">
                  {{ nextFireText(task) }}
                </span>
              </template>
            </div>
          </div>
        </div>

        <div class="flex items-center gap-2">
          <NButton size="small" secondary @click="handleTrigger(task)">
            <template #icon>
              <Play class="w-3.5 h-3.5 text-emerald-500" />
            </template>
            立即执行
          </NButton>

          <NButton size="small" secondary @click="openEditDrawer(task)">
            <template #icon>
              <Edit2 class="w-3.5 h-3.5 text-slate-500" />
            </template>
            编辑
          </NButton>

          <NButton size="small" secondary @click="openCopyDrawer(task)">
            <template #icon>
              <Copy class="w-3.5 h-3.5 text-slate-500" />
            </template>
            复制
          </NButton>

          <NButton size="small" secondary type="error" @click="handleDelete(task)">
            <template #icon>
              <Trash2 class="w-3.5 h-3.5" />
            </template>
          </NButton>
        </div>
      </div>
    </div>

    <!-- Task Drawer for Create / Edit -->
    <TaskDrawer
      v-model:show="showDrawer"
      :task="editingTask"
      :mode="drawerMode"
      @saved="taskStore.loadTasks()"
    />

    <!-- Live Log Drawer for execution monitoring -->
    <LiveLogDrawer
      v-model:show="showLogDrawer"
      :execution-id="selectedExecutionId"
    />

    <!-- Task Export Modal -->
    <TaskExportModal
      v-model:show="showExportModal"
      :tasks="taskStore.tasks"
    />

    <!-- Task Import Modal -->
    <TaskImportModal
      v-model:show="showImportModal"
      :existing-tasks="taskStore.tasks"
      @imported="taskStore.loadTasks()"
    />
  </div>
</template>
