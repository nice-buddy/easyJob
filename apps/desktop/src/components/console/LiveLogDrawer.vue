<script lang="ts">
export { getStatusTagType, getStatusLabel } from '../../types/execution';
</script>

<script setup lang="ts">
import { computed, ref, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { NDrawer, NDrawerContent, NButton, NTag, NSwitch, useMessage, useDialog } from 'naive-ui';
import { Square, Copy, Trash2, Terminal } from 'lucide-vue-next';
import { useExecutionStore } from '../../stores/executionStore';
import { getExecution } from '../../services/tauri';
import { onExecutionFinished, onExecutionStarted } from '../../services/events';
import type { Execution } from '../../types/execution';
import { getStatusTagType, getStatusLabel } from '../../types/execution';

const props = defineProps<{
  executionId: string | null;
  show: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
}>();

function handleUpdateShow(val: boolean) {
  emit('update:show', val);
}

const executionStore = useExecutionStore();

let message: {
  success: (msg: string) => void;
  error: (msg: string) => void;
  warning?: (msg: string) => void;
};
try {
  message = useMessage();
} catch {
  message = {
    success: (msg: string) => console.log(msg),
    error: (msg: string) => console.error(msg),
    warning: (msg: string) => console.warn(msg),
  };
}

let dialog: any = null;
try {
  dialog = useDialog();
} catch {
  dialog = null;
}

const currentExecution = ref<Execution | null>(null);
const logContainer = ref<HTMLElement | null>(null);
const autoScroll = ref(true);
const clearedOffset = ref(0);
const isCancelling = ref(false);

const rawLogs = computed(() => {
  if (!props.executionId) return [];
  return executionStore.logs[props.executionId] || [];
});

const logLines = computed(() => {
  return rawLogs.value.slice(clearedOffset.value);
});

let unlistenStarted: (() => void) | null = null;
let unlistenFinished: (() => void) | null = null;

onMounted(async () => {
  unlistenStarted = await onExecutionStarted(async (payload) => {
    if (props.executionId && payload.execution_id === props.executionId) {
      if (currentExecution.value) {
        currentExecution.value.status = 'Running';
      }
    }
  });

  unlistenFinished = await onExecutionFinished(async (payload) => {
    if (props.executionId && payload.execution_id === props.executionId) {
      if (currentExecution.value) {
        currentExecution.value.status = payload.status as any;
        if ((payload as any).duration_ms != null) {
          currentExecution.value.duration_ms = (payload as any).duration_ms;
        }
        if (payload.exit_code != null) {
          currentExecution.value.exit_code = payload.exit_code;
        }
        if ((payload as any).error_message != null) {
          currentExecution.value.error_message = (payload as any).error_message;
        }
      }
      await executionStore.fetchExecutionLogs(payload.execution_id, true);
    }
  });
});

onUnmounted(() => {
  if (unlistenStarted) {
    unlistenStarted();
  }
  if (unlistenFinished) {
    unlistenFinished();
  }
});

// Fetch execution details and historical output when executionId or show changes
watch(
  [() => props.executionId, () => props.show],
  async ([id, isShown]) => {
    if (id && isShown) {
      clearedOffset.value = 0;
      const cached = executionStore.executions.find((e) => e.id === id);
      if (cached) {
        currentExecution.value = { ...cached };
      }
      try {
        const fetched = await getExecution(id);
        if (fetched) {
          currentExecution.value = fetched;
        }
      } catch {
        // use cached execution
      }
      await executionStore.fetchExecutionLogs(id);
    } else if (!id && isShown) {
      clearedOffset.value = 0;
      currentExecution.value = {
        id: '',
        task_id: '',
        trigger_id: null,
        status: 'Queued',
        scheduled_at: null,
        started_at: new Date().toISOString(),
        finished_at: null,
        duration_ms: null,
        exit_code: null,
        error_message: null,
      };
    }
  },
  { immediate: true }
);

// Keep currentExecution in sync with executionStore.executions updates
watch(
  () => executionStore.executions,
  (list) => {
    if (props.executionId) {
      const match = list.find((e) => e.id === props.executionId);
      if (match) {
        currentExecution.value = { ...match };
      }
    }
  },
  { deep: true }
);

// Auto-scroll on new log entries
watch(
  () => logLines.value.length,
  async () => {
    if (!autoScroll.value) return;
    await nextTick();
    if (logContainer.value) {
      logContainer.value.scrollTop = logContainer.value.scrollHeight;
    }
  }
);

function handleClearView() {
  clearedOffset.value = rawLogs.value.length;
}

async function handleCopyLogs() {
  const text = logLines.value.join('\n');
  if (!text) {
    if (message.warning) {
      message.warning('暂无日志可复制');
    }
    return;
  }
  try {
    if (typeof navigator !== 'undefined' && navigator.clipboard && navigator.clipboard.writeText) {
      await navigator.clipboard.writeText(text);
    }
    message.success('日志已复制到剪贴板');
  } catch {
    message.error('复制失败');
  }
}

function handleCancel() {
  if (!props.executionId) return;
  if (dialog && dialog.warning) {
    dialog.warning({
      title: '确认终止执行',
      content: '确定要强制终止当前任务正在运行的进程吗？',
      positiveText: '终止执行',
      negativeText: '取消',
      onPositiveClick: () => {
        executeCancel();
      },
    });
  } else {
    executeCancel();
  }
}

async function executeCancel() {
  if (!props.executionId) return;
  isCancelling.value = true;
  try {
    await executionStore.cancelExecution(props.executionId);
    message.success('已下发终止指令');
    if (currentExecution.value) {
      currentExecution.value.status = 'Cancelled';
    }
  } catch (e: any) {
    message.error('终止失败: ' + (e?.message || e));
  } finally {
    isCancelling.value = false;
  }
}
</script>

<template>
  <NDrawer
    :show="show"
    :width="650"
    placement="right"
    @update:show="handleUpdateShow"
  >
    <NDrawerContent title="运行实时监控" closable>
      <div class="h-full flex flex-col space-y-3 pb-6">
        <!-- Status Header Card -->
        <div class="p-3.5 rounded-xl bg-slate-100 dark:bg-zinc-800/70 border border-slate-200 dark:border-zinc-700/50 space-y-2.5 text-xs">
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2">
              <span class="font-medium text-slate-600 dark:text-zinc-400">状态:</span>
              <NTag
                size="small"
                :bordered="false"
                :type="getStatusTagType(currentExecution?.status)"
              >
                {{ getStatusLabel(currentExecution?.status) }}
              </NTag>
              <span
                v-if="currentExecution?.duration_ms != null"
                class="text-slate-500 dark:text-zinc-400 ml-2"
              >
                耗时: {{ currentExecution.duration_ms }} ms
              </span>
            </div>

            <!-- Cancellation Button -->
            <NButton
              v-if="currentExecution?.status === 'Running'"
              size="tiny"
              type="error"
              secondary
              :loading="isCancelling"
              @click="handleCancel"
            >
              <template #icon>
                <Square class="w-3 h-3" />
              </template>
              终止执行
            </NButton>
          </div>

          <div class="grid grid-cols-2 gap-2 text-[11px] text-slate-500 dark:text-zinc-400 pt-1 border-t border-slate-200/60 dark:border-zinc-700/40">
            <div class="truncate">
              <span class="text-slate-400 dark:text-zinc-500">执行 ID: </span>
              <span class="font-mono text-slate-700 dark:text-zinc-300">
                {{ currentExecution?.id || props.executionId || '生成中...' }}
              </span>
            </div>
            <div class="truncate">
              <span class="text-slate-400 dark:text-zinc-500">任务 ID: </span>
              <span class="font-mono text-slate-700 dark:text-zinc-300">
                {{ currentExecution?.task_id || '-' }}
              </span>
            </div>
          </div>
        </div>

        <!-- Terminal Log Toolbar & Body -->
        <div class="flex-1 flex flex-col rounded-xl overflow-hidden border border-zinc-800 bg-zinc-950 shadow-inner">
          <!-- Terminal Toolbar -->
          <div class="flex items-center justify-between px-3 py-2 bg-zinc-900 border-b border-zinc-800 text-xs">
            <div class="flex items-center gap-2 text-zinc-400">
              <Terminal class="w-3.5 h-3.5 text-emerald-400" />
              <span class="font-mono font-medium text-[11px] text-zinc-300">终端输出流</span>
            </div>

            <div class="flex items-center gap-3">
              <div class="flex items-center gap-1.5">
                <NSwitch v-model:value="autoScroll" size="small">
                  <template #checked>自动滚动</template>
                  <template #unchecked>暂停滚动</template>
                </NSwitch>
              </div>

              <NButton
                size="tiny"
                quaternary
                class="text-zinc-400 hover:text-zinc-200"
                @click="handleClearView"
              >
                <template #icon>
                  <Trash2 class="w-3 h-3" />
                </template>
                清空当前视窗
              </NButton>

              <NButton
                size="tiny"
                quaternary
                class="text-zinc-400 hover:text-zinc-200"
                @click="handleCopyLogs"
              >
                <template #icon>
                  <Copy class="w-3 h-3" />
                </template>
                复制日志
              </NButton>
            </div>
          </div>

          <!-- Terminal Log View Container -->
          <div
            ref="logContainer"
            class="flex-1 p-4 text-zinc-200 font-mono text-xs overflow-y-auto select-text min-h-[300px]"
            style="max-height: calc(100vh - 240px);"
          >
            <div v-if="logLines.length === 0" class="text-zinc-500 italic py-8 text-center">
              等待输出流数据中...
            </div>
            <div
              v-for="(line, idx) in logLines"
              :key="idx"
              class="whitespace-pre-wrap break-all py-0.5 leading-relaxed flex items-start"
            >
              <span class="text-zinc-600 select-none mr-3 text-right w-6 flex-shrink-0 text-[11px]">
                {{ idx + 1 }}
              </span>
              <span class="flex-1">{{ line }}</span>
            </div>
          </div>
        </div>
      </div>
    </NDrawerContent>
  </NDrawer>
</template>
