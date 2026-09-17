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
  const startedListeners = new Set<(payload: { execution_id: string; task_id: string }) => void>();

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
    logs.value[id] = [...logs.value[id], content];
    if (logs.value[id].length > MAX_LINES_PER_EXECUTION) {
      logs.value[id] = logs.value[id].slice(logs.value[id].length - MAX_LINES_PER_EXECUTION);
    }
  }

  async function fetchExecutionLogs(id: ExecutionId, force = false) {
    if (!force && logs.value[id] && logs.value[id].length > 0) return;
    try {
      const records = await getExecutionOutput(id);
      if (records && records.length > 0) {
        logs.value[id] = records.map((r) => r.content);
      }
    } catch (e) {
      console.warn(`Failed to fetch historical output for execution ${id}:`, e);
    }
  }

  function waitForExecutionStarted(taskId: string, timeout = 5000): Promise<string | null> {
    if (!isListening) {
      initListeners();
    }
    return new Promise((resolve) => {
      let timer: ReturnType<typeof setTimeout> | null = null;
      const listener = (payload: { execution_id: string; task_id: string }) => {
        if (payload.task_id === taskId) {
          if (timer) clearTimeout(timer);
          startedListeners.delete(listener);
          resolve(payload.execution_id);
        }
      };
      startedListeners.add(listener);
      timer = setTimeout(() => {
        startedListeners.delete(listener);
        resolve(null);
      }, timeout);
    });
  }

  // Setup event listeners
  async function initListeners() {
    if (isListening) return;
    isListening = true;

    const unlistenStarted = await onExecutionStarted((payload) => {
      for (const listener of startedListeners) {
        try {
          listener(payload);
        } catch (err) {
          console.error(err);
        }
      }
      loadExecutions();
    });

    const unlistenOutput = await onExecutionOutput((payload) => {
      appendLog(payload.execution_id, payload.content);
    });

    const unlistenFinished = await onExecutionFinished(async (payload) => {
      // Direct in-place reactive update for instant UI feedback
      const target = executions.value.find((e) => e.id === payload.execution_id);
      if (target) {
        target.status = payload.status as any;
        if ((payload as any).duration_ms != null) {
          target.duration_ms = (payload as any).duration_ms;
        }
        if (payload.exit_code != null) {
          target.exit_code = payload.exit_code;
        }
        if ((payload as any).error_message != null) {
          target.error_message = (payload as any).error_message;
        }
      }
      const loadPromise = loadExecutions();
      try {
        await fetchExecutionLogs(payload.execution_id, true);
      } catch {
        // Continue to sync list
      }
      await loadPromise;
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
    startedListeners.clear();
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
    waitForExecutionStarted,
    initListeners,
    cleanupListeners,
  };
});
