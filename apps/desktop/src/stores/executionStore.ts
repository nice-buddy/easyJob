import { defineStore } from 'pinia';
import { ref } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
  listExecutions,
  cancelExecution as apiCancelExecution,
  getExecutionOutput,
} from '../services/tauri';
import {
  onExecutionStarted,
  onExecutionOutput,
  onExecutionFinished,
} from '../services/events';
import type { Execution, ExecutionId } from '../types/execution';

const MAX_LINES_PER_EXECUTION = 2000;
const MAX_CACHED_EXECUTIONS = 50;

export const useExecutionStore = defineStore('executions', () => {
  const executions = ref<Execution[]>([]);
  const logs = ref<Record<ExecutionId, string[]>>({});
  const activeExecutionId = ref<ExecutionId | null>(null);
  const loading = ref(false);

  let isListening = false;
  let unlistenFns: UnlistenFn[] = [];

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
      // Evict oldest executions if cache exceeds limit
      const keys = Object.keys(logs.value);
      if (keys.length >= MAX_CACHED_EXECUTIONS) {
        delete logs.value[keys[0]];
      }
      logs.value[id] = [];
    }
    logs.value[id].push(content);
    if (logs.value[id].length > MAX_LINES_PER_EXECUTION) {
      logs.value[id].splice(0, logs.value[id].length - MAX_LINES_PER_EXECUTION);
    }
  }

  async function fetchExecutionLogs(id: ExecutionId) {
    if (logs.value[id] && logs.value[id].length > 0) return;
    try {
      const records = await getExecutionOutput(id);
      if (records && records.length > 0) {
        logs.value[id] = records.map((r) => r.content);
      }
    } catch (e) {
      console.warn(`Failed to fetch historical output for execution ${id}:`, e);
    }
  }

  // Setup event listeners
  async function initListeners() {
    if (isListening) return;
    isListening = true;

    const unlistenStarted = await onExecutionStarted(() => {
      loadExecutions();
    });

    const unlistenOutput = await onExecutionOutput((payload) => {
      appendLog(payload.execution_id, payload.content);
    });

    const unlistenFinished = await onExecutionFinished(() => {
      loadExecutions();
    });

    unlistenFns.push(unlistenStarted, unlistenOutput, unlistenFinished);
  }

  async function cleanupListeners() {
    for (const unlisten of unlistenFns) {
      if (typeof unlisten === 'function') {
        unlisten();
      }
    }
    unlistenFns = [];
    isListening = false;
  }

  return {
    executions,
    logs,
    activeExecutionId,
    loading,
    loadExecutions,
    cancelExecution,
    appendLog,
    fetchExecutionLogs,
    initListeners,
    cleanupListeners,
  };
});
