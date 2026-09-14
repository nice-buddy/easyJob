<script setup lang="ts">
import { onMounted } from 'vue';
import { NButton, NInput, NSwitch, NTag, NEmpty, useMessage } from 'naive-ui';
import { Plus, Search, Play, Edit2, Trash2 } from 'lucide-vue-next';
import { useTaskStore } from '../stores/taskStore';
import type { Task } from '../types/task';

defineEmits<{
  (e: 'create-task'): void;
  (e: 'edit-task', task: Task): void;
}>();

const taskStore = useTaskStore();

let message: { success: (msg: string) => void; error: (msg: string) => void };
try {
  message = useMessage();
} catch {
  message = {
    success: (msg: string) => console.log(msg),
    error: (msg: string) => console.error(msg),
  };
}

onMounted(() => {
  taskStore.loadTasks();
});

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
    await taskStore.triggerTask(task.id);
    message.success(`已下发执行指令: ${task.name}`);
  } catch (e: any) {
    message.error('触发失败: ' + (e?.message || e));
  }
}

async function handleDelete(task: Task) {
  if (confirm(`确认删除任务 "${task.name}" 吗？`)) {
    try {
      await taskStore.deleteTask(task.id);
      message.success('任务已删除');
    } catch (e: any) {
      message.error('删除失败: ' + (e?.message || e));
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

        <NButton
          type="primary"
          size="medium"
          class="bg-emerald-600 hover:bg-emerald-500"
          @click="$emit('create-task')"
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

          <NButton size="small" secondary @click="$emit('edit-task', task)">
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
