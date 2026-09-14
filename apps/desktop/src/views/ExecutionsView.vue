<script setup lang="ts">
import { h, ref, onMounted } from 'vue';
import { NDataTable, NTag, NButton } from 'naive-ui';
import type { DataTableColumns } from 'naive-ui';
import LiveLogDrawer from '../components/console/LiveLogDrawer.vue';
import { useExecutionStore } from '../stores/executionStore';
import { useTaskStore } from '../stores/taskStore';
import type { Execution } from '../types/execution';

const executionStore = useExecutionStore();
const taskStore = useTaskStore();
const selectedExecId = ref<string | null>(null);
const showLog = ref(false);

onMounted(() => {
  executionStore.loadExecutions();
  if (taskStore.tasks.length === 0) {
    taskStore.loadTasks();
  }
});

const columns: DataTableColumns<Execution> = [
  {
    title: '关联任务',
    key: 'task_id',
    ellipsis: true,
    render(row: Execution) {
      const task = taskStore.tasks.find((t) => t.id === row.task_id);
      if (task) {
        return h('div', { class: 'flex flex-col' }, [
          h('span', { class: 'font-medium text-slate-800 dark:text-zinc-200' }, task.name),
          h('span', { class: 'text-[11px] text-slate-400 dark:text-zinc-500 font-mono truncate' }, row.task_id),
        ]);
      }
      return h('span', { class: 'font-mono text-xs text-slate-600 dark:text-zinc-400' }, row.task_id);
    },
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
      return h(
        NTag,
        { type: typeMap[row.status] || 'default', size: 'small', bordered: false },
        { default: () => row.status }
      );
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
      return h(
        NButton,
        {
          size: 'tiny',
          secondary: true,
          onClick: () => {
            selectedExecId.value = row.id;
            executionStore.activeExecutionId = row.id;
            showLog.value = true;
          },
        },
        { default: () => '查看输出' }
      );
    },
  },
];
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

    <LiveLogDrawer
      v-model:show="showLog"
      :execution-id="selectedExecId"
    />
  </div>
</template>
