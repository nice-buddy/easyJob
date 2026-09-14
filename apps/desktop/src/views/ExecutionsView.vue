<script setup lang="ts">
import { h, onMounted } from 'vue';
import { NDataTable, NTag, NButton } from 'naive-ui';
import type { DataTableColumns } from 'naive-ui';
import { useExecutionStore } from '../stores/executionStore';
import type { Execution } from '../types/execution';

const executionStore = useExecutionStore();

onMounted(() => {
  executionStore.loadExecutions();
});

const columns: DataTableColumns<Execution> = [
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
            executionStore.activeExecutionId = row.id;
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
  </div>
</template>
