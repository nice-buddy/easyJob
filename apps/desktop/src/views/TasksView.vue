<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { NButton, NInput, NSwitch, NTag, NEmpty, useMessage, useDialog } from 'naive-ui';
import { Plus, Search, Play, Edit2, Trash2, Download, Upload } from 'lucide-vue-next';
import TaskDrawer from '../components/task/TaskDrawer.vue';
import LiveLogDrawer from '../components/console/LiveLogDrawer.vue';
import TaskExportModal from '../components/task/TaskExportModal.vue';
import TaskImportModal from '../components/task/TaskImportModal.vue';
import { useTaskStore } from '../stores/taskStore';
import { useExecutionStore } from '../stores/executionStore';
import type { Task } from '../types/task';

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
});

function openCreateDrawer() {
  editingTask.value = null;
  showDrawer.value = true;
  emit('create-task');
}

function openEditDrawer(task: Task) {
  editingTask.value = JSON.parse(JSON.stringify(task));
  showDrawer.value = true;
  emit('edit-task', task);
}

async function handleToggleEnabled(task: Task, enabled: boolean) {
  try {
    const updated = { ...task, enabled };
    await taskStore.saveTask(updated);
    task.enabled = enabled;
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

          <NButton size="small" secondary @click="openEditDrawer(task)">
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

    <!-- Task Drawer for Create / Edit -->
    <TaskDrawer
      v-model:show="showDrawer"
      :task="editingTask"
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
