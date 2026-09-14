import { describe, it, expect, beforeEach, vi } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { useAgentStore } from '../src/stores/agentStore';
import { useTaskStore } from '../src/stores/taskStore';
import { useExecutionStore } from '../src/stores/executionStore';
import * as tauriService from '../src/services/tauri';
import * as eventsService from '../src/services/events';
import type { Task } from '../src/types/task';
import type { Execution, ExecutionOutputPayload } from '../src/types/execution';
import type { AgentStatus } from '../src/types/agent';

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
  onExecutionStarted: vi.fn(),
  onExecutionOutput: vi.fn(),
  onExecutionFinished: vi.fn(),
}));

describe('Pinia Stores', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  describe('useAgentStore', () => {
    it('initializes with default state', () => {
      const store = useAgentStore();
      expect(store.status).toBeNull();
      expect(store.isConnected).toBe(false);
      expect(store.lastError).toBeNull();
    });

    it('successfully fetches agent status', async () => {
      const mockStatus: AgentStatus = {
        version: '0.1.0',
        uptime_secs: 120,
        active_tasks: 3,
        running_executions: 1,
      };
      vi.mocked(tauriService.getAgentStatus).mockResolvedValueOnce(mockStatus);

      const store = useAgentStore();
      await store.fetchStatus();

      expect(store.status).toEqual(mockStatus);
      expect(store.isConnected).toBe(true);
      expect(store.lastError).toBeNull();
    });

    it('handles error when fetching agent status', async () => {
      vi.mocked(tauriService.getAgentStatus).mockRejectedValueOnce(new Error('IPC Connection refused'));

      const store = useAgentStore();
      await store.fetchStatus();

      expect(store.status).toBeNull();
      expect(store.isConnected).toBe(false);
      expect(store.lastError).toContain('IPC Connection refused');
    });
  });

  describe('useTaskStore', () => {
    const sampleTask1: Task = {
      id: 'task-1',
      name: 'Daily Database Backup',
      description: 'Backs up SQLite db daily',
      enabled: true,
      triggers: [],
      actions: [],
      execution_policy: {
        concurrency_policy: 'SkipIfRunning',
        missed_run_policy: 'RunOnce',
        timeout_secs: 300,
        max_retries: 1,
      },
      working_directory: null,
      environment: {},
      version: 1,
      created_at: '2026-09-14T00:00:00Z',
      updated_at: '2026-09-14T00:00:00Z',
    };

    const sampleTask2: Task = {
      id: 'task-2',
      name: 'Clean Log Files',
      description: 'Removes old log files',
      enabled: false,
      triggers: [],
      actions: [],
      execution_policy: {
        concurrency_policy: 'AllowParallel',
        missed_run_policy: 'Skip',
        timeout_secs: null,
        max_retries: 0,
      },
      working_directory: null,
      environment: {},
      version: 1,
      created_at: '2026-09-14T00:00:00Z',
      updated_at: '2026-09-14T00:00:00Z',
    };

    it('initializes with default state', () => {
      const store = useTaskStore();
      expect(store.tasks).toEqual([]);
      expect(store.searchQuery).toBe('');
      expect(store.loading).toBe(false);
      expect(store.filteredTasks).toEqual([]);
    });

    it('loads tasks and manages loading state', async () => {
      vi.mocked(tauriService.listTasks).mockResolvedValueOnce([sampleTask1, sampleTask2]);

      const store = useTaskStore();
      const loadPromise = store.loadTasks();
      expect(store.loading).toBe(true);

      await loadPromise;
      expect(store.loading).toBe(false);
      expect(store.tasks).toEqual([sampleTask1, sampleTask2]);
    });

    it('filters tasks by name and description case-insensitively', () => {
      const store = useTaskStore();
      store.tasks = [sampleTask1, sampleTask2];

      store.searchQuery = 'database';
      expect(store.filteredTasks).toEqual([sampleTask1]);

      store.searchQuery = 'old log';
      expect(store.filteredTasks).toEqual([sampleTask2]);

      store.searchQuery = '   ';
      expect(store.filteredTasks).toEqual([sampleTask1, sampleTask2]);

      store.searchQuery = 'non-existent';
      expect(store.filteredTasks).toEqual([]);
    });

    it('saves new task by unshifting into tasks array', async () => {
      const store = useTaskStore();
      store.tasks = [sampleTask2];

      vi.mocked(tauriService.saveTask).mockResolvedValueOnce(sampleTask1);

      const saved = await store.saveTask(sampleTask1);
      expect(saved).toEqual(sampleTask1);
      expect(store.tasks).toEqual([sampleTask1, sampleTask2]);
    });

    it('saves existing task by updating in place', async () => {
      const store = useTaskStore();
      store.tasks = [{ ...sampleTask1 }, { ...sampleTask2 }];

      const updatedTask1: Task = { ...sampleTask1, name: 'Renamed Task 1' };
      vi.mocked(tauriService.saveTask).mockResolvedValueOnce(updatedTask1);

      const saved = await store.saveTask(updatedTask1);
      expect(saved.name).toBe('Renamed Task 1');
      expect(store.tasks[0].name).toBe('Renamed Task 1');
      expect(store.tasks.length).toBe(2);
    });

    it('deletes task and removes from tasks array', async () => {
      const store = useTaskStore();
      store.tasks = [sampleTask1, sampleTask2];

      vi.mocked(tauriService.deleteTask).mockResolvedValueOnce(true);

      await store.deleteTask('task-1');
      expect(tauriService.deleteTask).toHaveBeenCalledWith('task-1');
      expect(store.tasks).toEqual([sampleTask2]);
    });

    it('triggers task execution via service', async () => {
      const store = useTaskStore();
      vi.mocked(tauriService.triggerTask).mockResolvedValueOnce(true);

      const result = await store.triggerTask('task-1');
      expect(tauriService.triggerTask).toHaveBeenCalledWith('task-1');
      expect(result).toBe(true);
    });
  });

  describe('useExecutionStore', () => {
    const sampleExecution: Execution = {
      id: 'exec-1',
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

    it('initializes with default state', () => {
      const store = useExecutionStore();
      expect(store.executions).toEqual([]);
      expect(store.logs).toEqual({});
      expect(store.activeExecutionId).toBeNull();
      expect(store.loading).toBe(false);
    });

    it('loads executions with default limit and toggles loading', async () => {
      vi.mocked(tauriService.listExecutions).mockResolvedValueOnce([sampleExecution]);

      const store = useExecutionStore();
      const loadPromise = store.loadExecutions();
      expect(store.loading).toBe(true);

      await loadPromise;
      expect(store.loading).toBe(false);
      expect(tauriService.listExecutions).toHaveBeenCalledWith(50);
      expect(store.executions).toEqual([sampleExecution]);
    });

    it('cancels execution via service', async () => {
      const store = useExecutionStore();
      vi.mocked(tauriService.cancelExecution).mockResolvedValueOnce(true);

      const res = await store.cancelExecution('exec-1');
      expect(tauriService.cancelExecution).toHaveBeenCalledWith('exec-1');
      expect(res).toBe(true);
    });

    it('appends logs per execution id and buffers content', () => {
      const store = useExecutionStore();

      store.appendLog('exec-1', 'line 1\n');
      expect(store.logs['exec-1']).toEqual(['line 1\n']);

      store.appendLog('exec-1', 'line 2\n');
      expect(store.logs['exec-1']).toEqual(['line 1\n', 'line 2\n']);

      store.appendLog('exec-2', 'other exec log\n');
      expect(store.logs['exec-2']).toEqual(['other exec log\n']);
    });

    it('initializes listeners and responds to events', async () => {
      let startedCallback: ((payload: any) => void) | undefined;
      let outputCallback: ((payload: ExecutionOutputPayload) => void) | undefined;
      let finishedCallback: ((payload: any) => void) | undefined;

      vi.mocked(eventsService.onExecutionStarted).mockImplementation(async (cb) => {
        startedCallback = cb;
        return () => {};
      });
      vi.mocked(eventsService.onExecutionOutput).mockImplementation(async (cb) => {
        outputCallback = cb;
        return () => {};
      });
      vi.mocked(eventsService.onExecutionFinished).mockImplementation(async (cb) => {
        finishedCallback = cb;
        return () => {};
      });

      vi.mocked(tauriService.listExecutions).mockResolvedValue([sampleExecution]);

      const store = useExecutionStore();
      store.initListeners();

      expect(eventsService.onExecutionStarted).toHaveBeenCalled();
      expect(eventsService.onExecutionOutput).toHaveBeenCalled();
      expect(eventsService.onExecutionFinished).toHaveBeenCalled();

      // Trigger started event
      expect(startedCallback).toBeDefined();
      startedCallback!({ execution_id: 'exec-1', task_id: 'task-1' });
      expect(tauriService.listExecutions).toHaveBeenCalledTimes(1);

      // Trigger output event
      expect(outputCallback).toBeDefined();
      outputCallback!({
        execution_id: 'exec-1',
        task_id: 'task-1',
        stream: 'stdout',
        content: 'hello from process',
      });
      expect(store.logs['exec-1']).toContain('hello from process');

      // Trigger finished event
      expect(finishedCallback).toBeDefined();
      finishedCallback!({
        execution_id: 'exec-1',
        task_id: 'task-1',
        status: 'Succeeded',
        exit_code: 0,
      });
      expect(tauriService.listExecutions).toHaveBeenCalledTimes(2);
    });
  });
});
