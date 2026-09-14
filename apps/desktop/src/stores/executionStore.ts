import { defineStore } from 'pinia';
import { ref } from 'vue';
import {
  listExecutions,
  cancelExecution as apiCancelExecution,
} from '../services/tauri';
import {
  onExecutionStarted,
  onExecutionOutput,
  onExecutionFinished,
} from '../services/events';
import type { Execution, ExecutionId } from '../types/execution';

export const useExecutionStore = defineStore('executions', () => {
  const executions = ref<Execution[]>([]);
  const logs = ref<Record<ExecutionId, string[]>>({});
  const activeExecutionId = ref<ExecutionId | null>(null);
  const loading = ref(false);

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
      logs.value[id] = [];
    }
    logs.value[id].push(content);
  }

  // Setup event listeners
  function initListeners() {
    onExecutionStarted(() => {
      loadExecutions();
    });

    onExecutionOutput((payload) => {
      appendLog(payload.execution_id, payload.content);
    });

    onExecutionFinished(() => {
      loadExecutions();
    });
  }

  return {
    executions,
    logs,
    activeExecutionId,
    loading,
    loadExecutions,
    cancelExecution,
    appendLog,
    initListeners,
  };
});
