<script setup lang="ts">
import { h, ref, computed, onMounted } from 'vue';
import {
  NDataTable,
  NTag,
  NButton,
  NInput,
  NSelect,
  NDatePicker,
} from 'naive-ui';
import type { DataTableColumns } from 'naive-ui';
import { Search, RotateCcw, RefreshCw } from 'lucide-vue-next';
import LiveLogDrawer from '../components/console/LiveLogDrawer.vue';
import { useExecutionStore } from '../stores/executionStore';
import { useTaskStore } from '../stores/taskStore';
import type { Execution } from '../types/execution';
import { getStatusLabel, getStatusTagType } from '../types/execution';

const executionStore = useExecutionStore();
const taskStore = useTaskStore();
const selectedExecId = ref<string | null>(null);
const showLog = ref(false);

// Filters
const searchTaskName = ref('');
const statusFilter = ref<string>('all');
const timeRange = ref<[number, number] | null>(null);

const statusOptions = [
  { label: '全部状态', value: 'all' },
  { label: '成功', value: 'Succeeded' },
  { label: '失败', value: 'Failed' },
  { label: '运行中', value: 'Running' },
  { label: '超时', value: 'TimedOut' },
  { label: '已取消', value: 'Cancelled' },
  { label: '排队中', value: 'Queued' },
  { label: '异常中断', value: 'Interrupted' },
];

onMounted(() => {
  executionStore.loadExecutions();
  if (taskStore.tasks.length === 0) {
    taskStore.loadTasks();
  }
});

function formatTimestamp(isoStr: string): string {
  if (!isoStr) return '-';
  try {
    const d = new Date(isoStr);
    if (isNaN(d.getTime())) return isoStr;
    const pad = (n: number) => n.toString().padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  } catch {
    return isoStr;
  }
}

const filteredExecutions = computed(() => {
  let list = executionStore.executions;

  // Filter by task name / id fuzzy search
  const q = searchTaskName.value.trim().toLowerCase();
  if (q) {
    list = list.filter((row) => {
      const task = taskStore.tasks.find((t) => t.id === row.task_id);
      const taskNameMatch = task?.name ? task.name.toLowerCase().includes(q) : false;
      const taskIdMatch = row.task_id.toLowerCase().includes(q);
      const execIdMatch = row.id.toLowerCase().includes(q);
      return taskNameMatch || taskIdMatch || execIdMatch;
    });
  }

  // Filter by status
  if (statusFilter.value && statusFilter.value !== 'all') {
    list = list.filter((row) => row.status === statusFilter.value);
  }

  // Filter by start time range
  if (timeRange.value && timeRange.value.length === 2) {
    const [startMs, endMs] = timeRange.value;
    list = list.filter((row) => {
      const time = Date.parse(row.started_at);
      if (isNaN(time)) return false;
      return time >= startMs && time <= endMs;
    });
  }

  return list;
});

const hasActiveFilter = computed(() => {
  return (
    searchTaskName.value.trim() !== '' ||
    statusFilter.value !== 'all' ||
    timeRange.value !== null
  );
});

function handleResetFilters() {
  searchTaskName.value = '';
  statusFilter.value = 'all';
  timeRange.value = null;
}

const columns: DataTableColumns<Execution> = [
  {
    title: '关联任务',
    key: 'task_id',
    ellipsis: true,
    minWidth: 160,
    render(row: Execution) {
      const task = taskStore.tasks.find((t) => t.id === row.task_id);
      if (task) {
        return h('div', { class: 'flex flex-col min-w-0' }, [
          h('span', { class: 'font-medium text-slate-800 dark:text-zinc-200 truncate' }, task.name),
          h('span', { class: 'text-[11px] text-slate-400 dark:text-zinc-500 font-mono truncate' }, row.task_id),
        ]);
      }
      return h('span', { class: 'font-mono text-xs text-slate-600 dark:text-zinc-400 truncate' }, row.task_id);
    },
  },
  {
    title: '状态',
    key: 'status',
    width: 90,
    align: 'center',
    render(row: Execution) {
      return h(
        NTag,
        { type: getStatusTagType(row.status), size: 'small', bordered: false },
        { default: () => getStatusLabel(row.status) }
      );
    },
  },
  {
    title: '耗时 (ms)',
    key: 'duration_ms',
    width: 95,
    align: 'center',
    render(row: Execution) {
      return row.duration_ms != null ? `${row.duration_ms} ms` : '-';
    },
  },
  {
    title: '开始时间',
    key: 'started_at',
    width: 175,
    align: 'center',
    render(row: Execution) {
      return h('span', { class: 'font-mono text-xs text-slate-600 dark:text-zinc-400' }, formatTimestamp(row.started_at));
    },
  },
  {
    title: '操作',
    key: 'actions',
    width: 90,
    align: 'center',
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
  <div class="space-y-4">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-xl font-bold">执行日志</h1>
        <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">查看所有自动化任务的历史运行状态与输出流</p>
      </div>
      <NButton size="small" secondary @click="executionStore.loadExecutions()">
        <template #icon>
          <RefreshCw class="w-3.5 h-3.5" />
        </template>
        刷新日志
      </NButton>
    </div>

    <!-- Filter Toolbar -->
    <div class="p-3 bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-xl flex flex-wrap items-center gap-3 text-xs">
      <!-- Search task name / ID -->
      <div class="w-56">
        <NInput
          v-model:value="searchTaskName"
          placeholder="搜索任务名称 / ID..."
          size="small"
          clearable
        >
          <template #prefix>
            <Search class="w-3.5 h-3.5 text-slate-400" />
          </template>
        </NInput>
      </div>

      <!-- Filter by status -->
      <div class="w-32">
        <NSelect
          v-model:value="statusFilter"
          :options="statusOptions"
          size="small"
          placeholder="状态筛选"
        />
      </div>

      <!-- Filter by date range -->
      <div class="w-72">
        <NDatePicker
          v-model:value="timeRange"
          type="datetimerange"
          size="small"
          clearable
          :start-placeholder="'开始时间'"
          :end-placeholder="'结束时间'"
        />
      </div>

      <!-- Reset button -->
      <NButton
        v-if="hasActiveFilter"
        size="small"
        quaternary
        type="warning"
        @click="handleResetFilters"
      >
        <template #icon>
          <RotateCcw class="w-3.5 h-3.5" />
        </template>
        重置
      </NButton>

      <!-- Record count -->
      <div class="ml-auto text-slate-400 dark:text-zinc-500 font-mono text-[11px]">
        共 {{ filteredExecutions.length }} 条记录
      </div>
    </div>

    <!-- Data Table -->
    <NDataTable
      :columns="columns"
      :data="filteredExecutions"
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
