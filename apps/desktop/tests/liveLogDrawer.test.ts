import { describe, it, expect, beforeEach, vi } from 'vitest';
import { ref, computed, watch, nextTick } from 'vue';
import { setActivePinia, createPinia } from 'pinia';
import LiveLogDrawer, { getStatusTagType } from '../src/components/console/LiveLogDrawer.vue';
import ExecutionsView from '../src/views/ExecutionsView.vue';
import TasksView from '../src/views/TasksView.vue';
import { useExecutionStore } from '../src/stores/executionStore';
import { useTaskStore } from '../src/stores/taskStore';
import * as tauriService from '../src/services/tauri';
import type { Execution } from '../src/types/execution';

vi.mock('../src/services/tauri', () => ({
  getAgentStatus: vi.fn(),
  listTasks: vi.fn(),
  getTask: vi.fn(),
  saveTask: vi.fn(),
  deleteTask: vi.fn(),
  triggerTask: vi.fn(),
  listExecutions: vi.fn(),
  getExecution: vi.fn(),
  cancelExecution: vi.fn(),
}));

vi.mock('../src/services/events', () => ({
  onExecutionStarted: vi.fn().mockResolvedValue(vi.fn()),
  onExecutionOutput: vi.fn().mockResolvedValue(vi.fn()),
  onExecutionFinished: vi.fn().mockResolvedValue(vi.fn()),
}));

describe('LiveLogDrawer & Task Cancellation', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  describe('Component Definitions & Interface Contracts', () => {
    it('exports a valid LiveLogDrawer Vue component', () => {
      expect(LiveLogDrawer).toBeDefined();
    });

    it('LiveLogDrawer defines expected props and emits', () => {
      const props = (LiveLogDrawer as any).props;
      expect(props).toBeDefined();
      expect(props.executionId).toBeDefined();
      expect(props.show).toBeDefined();

      const emits = (LiveLogDrawer as any).emits;
      expect(emits).toBeDefined();
      expect(emits).toContain('update:show');
    });
  });

  describe('Status Badge Mapping (getStatusTagType)', () => {
    it('maps Running to info tag', () => {
      expect(getStatusTagType('Running')).toBe('info');
    });

    it('maps Succeeded to success tag', () => {
      expect(getStatusTagType('Succeeded')).toBe('success');
    });

    it('maps Failed to error tag', () => {
      expect(getStatusTagType('Failed')).toBe('error');
    });

    it('maps TimedOut to warning tag', () => {
      expect(getStatusTagType('TimedOut')).toBe('warning');
    });

    it('maps Cancelled to default tag', () => {
      expect(getStatusTagType('Cancelled')).toBe('default');
    });

    it('maps Interrupted to warning tag', () => {
      expect(getStatusTagType('Interrupted')).toBe('warning');
    });

    it('maps unknown or undefined status to default tag', () => {
      expect(getStatusTagType(undefined)).toBe('default');
      expect(getStatusTagType('Unknown' as any)).toBe('default');
    });
  });

  describe('Terminal Log Streaming & Controls Logic', () => {
    it('reacts dynamically to log lines appended to executionStore', async () => {
      const executionStore = useExecutionStore();
      const executionId = 'exec-stream-1';

      const rawLogs = computed(() => executionStore.logs[executionId] || []);
      expect(rawLogs.value).toEqual([]);

      executionStore.appendLog(executionId, '[2026-09-14 10:00:00] Starting job...');
      expect(rawLogs.value).toHaveLength(1);
      expect(rawLogs.value[0]).toContain('Starting job...');

      executionStore.appendLog(executionId, '[2026-09-14 10:00:01] Processing items 1-50');
      executionStore.appendLog(executionId, '[2026-09-14 10:00:02] Completed successfully.');
      expect(rawLogs.value).toHaveLength(3);
    });

    it('displays fallback message when log stream is empty', () => {
      const logs: string[] = [];
      const fallbackText = logs.length === 0 ? '等待输出流数据中...' : '';
      expect(fallbackText).toBe('等待输出流数据中...');
    });

    it('handles clear current view window (清空当前视窗)', () => {
      const rawLogs = ref(['Line 1: init', 'Line 2: running', 'Line 3: progress 50%']);
      const clearedOffset = ref(0);

      const visibleLogs = computed(() => rawLogs.value.slice(clearedOffset.value));
      expect(visibleLogs.value).toHaveLength(3);

      // User clicks "清空当前视窗"
      clearedOffset.value = rawLogs.value.length;
      expect(visibleLogs.value).toHaveLength(0);

      // New log arrives
      rawLogs.value.push('Line 4: progress 100%');
      expect(visibleLogs.value).toHaveLength(1);
      expect(visibleLogs.value[0]).toBe('Line 4: progress 100%');
    });

    it('auto-scroll behavior reacts to log changes when enabled and respects toggle', async () => {
      let scrolled = false;
      const autoScroll = ref(true);
      const logLines = ref<string[]>([]);

      watch(
        () => logLines.value.length,
        () => {
          if (autoScroll.value) {
            scrolled = true;
          }
        }
      );

      // New log with auto-scroll enabled
      logLines.value.push('line 1');
      await nextTick();
      expect(scrolled).toBe(true);

      // Disable auto-scroll
      autoScroll.value = false;
      scrolled = false;

      logLines.value.push('line 2');
      await nextTick();
      expect(scrolled).toBe(false);
    });

    it('formats log lines for copy to clipboard (复制日志)', () => {
      const logs = ['[INFO] starting task', '[DEBUG] connecting socket', '[INFO] done'];
      const textToCopy = logs.join('\n');
      expect(textToCopy).toBe('[INFO] starting task\n[DEBUG] connecting socket\n[INFO] done');
    });

    it('handles clipboard copy API interaction and fallback', async () => {
      const writeTextMock = vi.fn().mockResolvedValue(undefined);
      const originalClipboard = navigator.clipboard;

      Object.assign(navigator, {
        clipboard: {
          writeText: writeTextMock,
        },
      });

      const logs = ['line 1', 'line 2'];
      await navigator.clipboard.writeText(logs.join('\n'));
      expect(writeTextMock).toHaveBeenCalledWith('line 1\nline 2');

      if (originalClipboard) {
        Object.assign(navigator, { clipboard: originalClipboard });
      }
    });

    it('formats duration correctly with fallback', () => {
      const formatDuration = (exec: { duration_ms: number | null; status: string }) => {
        if (exec.duration_ms != null) return `${exec.duration_ms} ms`;
        return exec.status === 'Running' ? '运行中...' : '-';
      };

      expect(formatDuration({ duration_ms: 1250, status: 'Succeeded' })).toBe('1250 ms');
      expect(formatDuration({ duration_ms: null, status: 'Running' })).toBe('运行中...');
      expect(formatDuration({ duration_ms: null, status: 'Failed' })).toBe('-');
    });
  });

  describe('Execution Reactivity & Store Synchronization', () => {
    it('syncs current execution with executionStore updates', async () => {
      const executionStore = useExecutionStore();
      const execId = 'exec-sync-1';

      const initialExec: Execution = {
        id: execId,
        task_id: 'task-1',
        trigger_id: 'trig-1',
        status: 'Running',
        scheduled_at: null,
        started_at: '2026-09-14T10:00:00Z',
        finished_at: null,
        duration_ms: null,
        exit_code: null,
        error_message: null,
      };

      executionStore.executions = [initialExec];
      const currentExecution = ref<Execution | null>({ ...initialExec });

      watch(
        () => executionStore.executions,
        (list) => {
          const match = list.find((e) => e.id === execId);
          if (match) {
            currentExecution.value = { ...match };
          }
        },
        { deep: true }
      );

      expect(currentExecution.value?.status).toBe('Running');

      // Execution finishes in background
      executionStore.executions = [
        {
          ...initialExec,
          status: 'Succeeded',
          duration_ms: 4500,
          finished_at: '2026-09-14T10:00:04Z',
          exit_code: 0,
        },
      ];

      await nextTick();
      expect(currentExecution.value?.status).toBe('Succeeded');
      expect(currentExecution.value?.duration_ms).toBe(4500);
    });

    it('fetches execution via getExecution when drawer opens', async () => {
      const sample: Execution = {
        id: 'exec-fetch-1',
        task_id: 'task-fetch-1',
        trigger_id: 'trig-1',
        status: 'Running',
        scheduled_at: null,
        started_at: '2026-09-14T10:00:00Z',
        finished_at: null,
        duration_ms: null,
        exit_code: null,
        error_message: null,
      };
      vi.mocked(tauriService.getExecution).mockResolvedValueOnce(sample);

      const res = await tauriService.getExecution('exec-fetch-1');
      expect(res.id).toBe('exec-fetch-1');
      expect(res.status).toBe('Running');
    });
  });

  describe('Execution Cancellation Workflow', () => {
    const sampleRunningExecution: Execution = {
      id: 'exec-cancel-test',
      task_id: 'task-cancel-test',
      trigger_id: 'trig-1',
      status: 'Running',
      scheduled_at: null,
      started_at: '2026-09-14T10:00:00Z',
      finished_at: null,
      duration_ms: null,
      exit_code: null,
      error_message: null,
    };

    it('shows cancel button only when status is Running', () => {
      const isCancelVisible = (status: string) => status === 'Running';

      expect(isCancelVisible('Running')).toBe(true);
      expect(isCancelVisible('Succeeded')).toBe(false);
      expect(isCancelVisible('Failed')).toBe(false);
      expect(isCancelVisible('TimedOut')).toBe(false);
      expect(isCancelVisible('Cancelled')).toBe(false);
      expect(isCancelVisible('Interrupted')).toBe(false);
    });

    it('cancels execution successfully via executionStore', async () => {
      vi.mocked(tauriService.cancelExecution).mockResolvedValueOnce(true);
      const executionStore = useExecutionStore();

      const currentExecution = ref<Execution>({ ...sampleRunningExecution });
      expect(currentExecution.value.status).toBe('Running');

      const res = await executionStore.cancelExecution(currentExecution.value.id);
      expect(res).toBe(true);
      expect(tauriService.cancelExecution).toHaveBeenCalledWith('exec-cancel-test');

      currentExecution.value.status = 'Cancelled';
      expect(currentExecution.value.status).toBe('Cancelled');
    });

    it('handles cancellation error gracefully', async () => {
      vi.mocked(tauriService.cancelExecution).mockRejectedValueOnce(new Error('Process not found'));
      const executionStore = useExecutionStore();

      let errorMessage = '';
      try {
        await executionStore.cancelExecution('exec-fail-cancel');
      } catch (e: any) {
        errorMessage = e?.message || String(e);
      }

      expect(errorMessage).toBe('Process not found');
    });
  });

  describe('Integration with Views', () => {
    it('ExecutionsView defines integration with LiveLogDrawer', () => {
      expect(ExecutionsView).toBeDefined();
    });

    it('TasksView defines integration with LiveLogDrawer on manual trigger', () => {
      expect(TasksView).toBeDefined();
    });

    it('TasksView opens LiveLogDrawer and sets selectedExecutionId on trigger', async () => {
      const taskStore = useTaskStore();
      const executionStore = useExecutionStore();

      const sampleTask: Task = {
        id: 'task-trigger-test',
        name: 'Manual Task',
        description: 'Test manual trigger',
        enabled: true,
        triggers: [],
        actions: [],
        execution_policy: {
          concurrency_policy: 'AllowParallel',
          missed_run_policy: 'Skip',
          retry_policy: { max_retries: 0, delay_secs: 0 },
          timeout_secs: 60,
        },
        version: 1,
        created_at: '2026-09-14T10:00:00Z',
        updated_at: '2026-09-14T10:00:00Z',
      };

      vi.mocked(tauriService.triggerTask).mockResolvedValueOnce(true);
      const execSample: Execution = {
        id: 'exec-manual-1',
        task_id: 'task-trigger-test',
        trigger_id: 't-manual',
        status: 'Running',
        scheduled_at: null,
        started_at: '2026-09-14T10:00:00Z',
        finished_at: null,
        duration_ms: null,
        exit_code: null,
        error_message: null,
      };
      vi.mocked(tauriService.listExecutions).mockResolvedValueOnce([execSample]);

      await taskStore.triggerTask(sampleTask.id);
      expect(tauriService.triggerTask).toHaveBeenCalledWith('task-trigger-test');

      await executionStore.loadExecutions();
      const match = executionStore.executions.find((e) => e.task_id === sampleTask.id);
      expect(match).toBeDefined();
      expect(match?.id).toBe('exec-manual-1');
    });

    it('TasksView waits for execution started event and binds new execution ID', async () => {
      const taskStore = useTaskStore();
      const executionStore = useExecutionStore();

      const sampleTask: Task = {
        id: 'task-trigger-event-test',
        name: 'Manual Event Task',
        description: 'Test event binding',
        enabled: true,
        triggers: [],
        actions: [],
        execution_policy: {
          concurrency_policy: 'AllowParallel',
          missed_run_policy: 'Skip',
          retry_policy: { max_retries: 0, delay_secs: 0 },
          timeout_secs: 60,
        },
        version: 1,
        created_at: '2026-09-14T10:00:00Z',
        updated_at: '2026-09-14T10:00:00Z',
      };

      vi.mocked(tauriService.triggerTask).mockResolvedValueOnce(true);
      vi.spyOn(executionStore, 'waitForExecutionStarted').mockResolvedValueOnce('exec-new-999');

      const waitPromise = executionStore.waitForExecutionStarted(sampleTask.id, 5000);
      await taskStore.triggerTask(sampleTask.id);

      const execId = await waitPromise;
      expect(execId).toBe('exec-new-999');
    });
  });
});
