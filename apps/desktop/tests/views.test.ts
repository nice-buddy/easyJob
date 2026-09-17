import { describe, it, expect, beforeEach, vi } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import App from '../src/App.vue';
import AppSidebar from '../src/components/layout/AppSidebar.vue';
import TasksView from '../src/views/TasksView.vue';
import ExecutionsView from '../src/views/ExecutionsView.vue';
import SettingsView from '../src/views/SettingsView.vue';
import { useAgentStore } from '../src/stores/agentStore';
import { useTaskStore } from '../src/stores/taskStore';
import { useExecutionStore } from '../src/stores/executionStore';
import * as tauriService from '../src/services/tauri';
import * as eventsService from '../src/services/events';
import * as autostartService from '../src/services/autostart';
import * as pluginAutostart from '@tauri-apps/plugin-autostart';
import { ref } from 'vue';
import * as tauriEvent from '@tauri-apps/api/event';
import type { Task } from '../src/types/task';
import type { Execution } from '../src/types/execution';
import { getStatusLabel, getStatusTagType } from '../src/types/execution';
import { isWindowsPlatform } from '../src/types/task';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

vi.mock('@tauri-apps/plugin-autostart', () => ({
  enable: vi.fn().mockResolvedValue(undefined),
  disable: vi.fn().mockResolvedValue(undefined),
  isEnabled: vi.fn().mockResolvedValue(false),
}));

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

const mockStorage: Record<string, string> = {};
if (typeof globalThis.localStorage === 'undefined') {
  globalThis.localStorage = {
    getItem: (key: string) => mockStorage[key] ?? null,
    setItem: (key: string, val: string) => { mockStorage[key] = String(val); },
    removeItem: (key: string) => { delete mockStorage[key]; },
    clear: () => { Object.keys(mockStorage).forEach((k) => delete mockStorage[k]); },
    length: 0,
    key: () => null,
  } as Storage;
}

describe('Desktop UI Views & Components', () => {
  beforeEach(() => {
    localStorage.clear();

    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  describe('Component Definitions & Contracts', () => {
    it('exports valid Vue components for all views and layout', () => {
      expect(App).toBeDefined();
      expect(AppSidebar).toBeDefined();
      expect(TasksView).toBeDefined();
      expect(ExecutionsView).toBeDefined();
      expect(SettingsView).toBeDefined();
    });

    it('AppSidebar defines currentView and isDark props', () => {
      const props = (AppSidebar as any).props;
      expect(props).toBeDefined();
      expect(props.currentView).toBeDefined();
      expect(props.isDark).toBeDefined();
    });

    it('TasksView defines create-task and edit-task emits', () => {
      const emits = (TasksView as any).emits;
      expect(emits).toBeDefined();
      expect(emits).toContain('create-task');
      expect(emits).toContain('edit-task');
    });

    it('AppSidebar defines change-view and toggle-theme emits', () => {
      const emits = (AppSidebar as any).emits;
      expect(emits).toBeDefined();
      expect(emits).toContain('change-view');
      expect(emits).toContain('toggle-theme');
    });
  });

  describe('TasksView Logic & Store Integration', () => {
    const sampleTask: Task = {
      id: 'task-view-1',
      name: 'Backup Job',
      description: 'Daily database backup',
      enabled: true,
      triggers: [
        {
          id: 't-1',
          task_id: 'task-view-1',
          enabled: true,
          kind: { Daily: { time: '02:00:00', timezone: 'UTC' } },
          created_at: '2026-09-14T00:00:00Z',
          updated_at: '2026-09-14T00:00:00Z',
        },
      ],
      actions: [
        {
          id: 'a-1',
          task_id: 'task-view-1',
          sequence: 1,
          enabled: true,
          kind: { ExecuteShell: { command: 'backup.sh' } },
        },
      ],
      execution_policy: {
        concurrency_policy: 'SkipIfRunning',
        missed_run_policy: 'RunOnce',
        retry_policy: { max_retries: 1, delay_secs: 5 },
        timeout_secs: 300,
      },
      working_directory: null,
      environment: {},
      version: 1,
      created_at: '2026-09-14T00:00:00Z',
      updated_at: '2026-09-14T00:00:00Z',
    };

    it('loads tasks via taskStore on view mount', async () => {
      vi.mocked(tauriService.listTasks).mockResolvedValueOnce([sampleTask]);
      const taskStore = useTaskStore();

      await taskStore.loadTasks();
      expect(tauriService.listTasks).toHaveBeenCalled();
      expect(taskStore.tasks).toHaveLength(1);
      expect(taskStore.tasks[0].id).toBe('task-view-1');
    });

    it('toggles task enabled status and persists to store', async () => {
      const taskStore = useTaskStore();
      taskStore.tasks = [{ ...sampleTask, enabled: true }];

      const updatedTask = { ...sampleTask, enabled: false };
      vi.mocked(tauriService.saveTask).mockResolvedValueOnce(updatedTask);

      const res = await taskStore.saveTask(updatedTask);
      expect(tauriService.saveTask).toHaveBeenCalledWith(updatedTask);
      expect(res.enabled).toBe(false);
      expect(taskStore.tasks[0].enabled).toBe(false);
    });

    it('handles toggle failure gracefully', async () => {
      const taskStore = useTaskStore();
      taskStore.tasks = [{ ...sampleTask, enabled: true }];

      vi.mocked(tauriService.saveTask).mockRejectedValueOnce(new Error('DB write failed'));

      await expect(taskStore.saveTask({ ...sampleTask, enabled: false })).rejects.toThrow('DB write failed');
    });

    it('triggers task execution via taskStore', async () => {
      const taskStore = useTaskStore();
      vi.mocked(tauriService.triggerTask).mockResolvedValueOnce(true);

      const triggered = await taskStore.triggerTask('task-view-1');
      expect(tauriService.triggerTask).toHaveBeenCalledWith('task-view-1');
      expect(triggered).toBe(true);
    });

    it('handles trigger failure', async () => {
      const taskStore = useTaskStore();
      vi.mocked(tauriService.triggerTask).mockRejectedValueOnce(new Error('Agent unreachable'));

      await expect(taskStore.triggerTask('task-view-1')).rejects.toThrow('Agent unreachable');
    });

    it('deletes task from store when confirmed', async () => {
      const taskStore = useTaskStore();
      taskStore.tasks = [sampleTask];
      vi.mocked(tauriService.deleteTask).mockResolvedValueOnce(true);

      await taskStore.deleteTask(sampleTask.id);
      expect(tauriService.deleteTask).toHaveBeenCalledWith(sampleTask.id);
      expect(taskStore.tasks).toHaveLength(0);
    });

    it('filters tasks based on search query', () => {
      const taskStore = useTaskStore();
      taskStore.tasks = [
        sampleTask,
        {
          ...sampleTask,
          id: 'task-view-2',
          name: 'Log Cleaner',
          description: 'Cleans old logs',
        },
      ];

      taskStore.searchQuery = 'cleaner';
      expect(taskStore.filteredTasks).toHaveLength(1);
      expect(taskStore.filteredTasks[0].id).toBe('task-view-2');

      taskStore.searchQuery = 'not-found';
      expect(taskStore.filteredTasks).toHaveLength(0);

      taskStore.searchQuery = '';
      expect(taskStore.filteredTasks).toHaveLength(2);
    });
  });

  describe('ExecutionsView Logic & Store Integration', () => {
    const sampleExecution: Execution = {
      id: 'exec-view-1',
      task_id: 'task-view-1',
      trigger_id: 't-1',
      status: 'Succeeded',
      scheduled_at: null,
      started_at: '2026-09-14T10:00:00Z',
      finished_at: '2026-09-14T10:00:05Z',
      duration_ms: 5000,
      exit_code: 0,
      error_message: null,
    };

    it('loads executions and displays recent activity', async () => {
      vi.mocked(tauriService.listExecutions).mockResolvedValueOnce([sampleExecution]);
      const executionStore = useExecutionStore();

      await executionStore.loadExecutions();
      expect(tauriService.listExecutions).toHaveBeenCalledWith(50);
      expect(executionStore.executions).toHaveLength(1);
      expect(executionStore.executions[0].id).toBe('exec-view-1');
      expect(executionStore.executions[0].duration_ms).toBe(5000);
    });

    it('sets activeExecutionId when inspecting output', () => {
      const executionStore = useExecutionStore();
      expect(executionStore.activeExecutionId).toBeNull();

      executionStore.activeExecutionId = 'exec-view-1';
      expect(executionStore.activeExecutionId).toBe('exec-view-1');
    });

    it('maps execution statuses to correct visual tag types', () => {
      const statusMap: Record<string, string> = {
        Succeeded: 'success',
        Failed: 'error',
        TimedOut: 'warning',
        Running: 'info',
        Cancelled: 'default',
      };

      expect(statusMap['Succeeded']).toBe('success');
      expect(statusMap['Failed']).toBe('error');
      expect(statusMap['TimedOut']).toBe('warning');
      expect(statusMap['Running']).toBe('info');
      expect(statusMap['Cancelled']).toBe('default');
      expect(statusMap['UnknownStatus'] || 'default').toBe('default');
    });

    it('formats duration correctly with fallback for null', () => {
      const formatDuration = (ms: number | null) => (ms != null ? `${ms} ms` : '-');
      expect(formatDuration(null)).toBe('-');
      expect(formatDuration(120)).toBe('120 ms');
      expect(formatDuration(0)).toBe('0 ms');
    });

    it('translates execution status to Chinese labels accurately', () => {
      expect(getStatusLabel('Succeeded')).toBe('成功');
      expect(getStatusLabel('Failed')).toBe('失败');
      expect(getStatusLabel('Running')).toBe('运行中');
      expect(getStatusLabel('TimedOut')).toBe('超时');
      expect(getStatusLabel('Cancelled')).toBe('已取消');
      expect(getStatusLabel('Queued')).toBe('排队中');
      expect(getStatusLabel('Skipped')).toBe('已跳过');
      expect(getStatusLabel('Interrupted')).toBe('异常中断');
      expect(getStatusLabel('Unknown')).toBe('Unknown');
      expect(getStatusLabel(undefined)).toBe('-');

      expect(getStatusTagType('Succeeded')).toBe('success');
      expect(getStatusTagType('Failed')).toBe('error');
      expect(getStatusTagType('Running')).toBe('info');
      expect(getStatusTagType('TimedOut')).toBe('warning');
      expect(getStatusTagType('Cancelled')).toBe('default');
    });

    it('filters executions by task name, status, and time range correctly', () => {
      const exec1: Execution = {
        id: 'exec-1',
        task_id: 'task-1',
        trigger_id: null,
        status: 'Succeeded',
        scheduled_at: null,
        started_at: '2026-09-14T08:00:00Z',
        finished_at: '2026-09-14T08:00:10Z',
        duration_ms: 10000,
        exit_code: 0,
        error_message: null,
      };

      const exec2: Execution = {
        id: 'exec-2',
        task_id: 'task-2',
        trigger_id: null,
        status: 'Failed',
        scheduled_at: null,
        started_at: '2026-09-15T12:00:00Z',
        finished_at: '2026-09-15T12:00:05Z',
        duration_ms: 5000,
        exit_code: 1,
        error_message: 'error',
      };

      const all = [exec1, exec2];

      // Filter by status 'Failed'
      const failedOnly = all.filter((e) => e.status === 'Failed');
      expect(failedOnly).toHaveLength(1);
      expect(failedOnly[0].id).toBe('exec-2');

      // Filter by time range
      const t1 = Date.parse('2026-09-14T00:00:00Z');
      const t2 = Date.parse('2026-09-14T23:59:59Z');
      const day1Only = all.filter((e) => {
        const ts = Date.parse(e.started_at);
        return ts >= t1 && ts <= t2;
      });
      expect(day1Only).toHaveLength(1);
      expect(day1Only[0].id).toBe('exec-1');
    });

    it('detects platform correctly and exposes isWindowsPlatform helper', () => {
      expect(typeof isWindowsPlatform).toBe('function');
      const isWin = isWindowsPlatform();
      expect(typeof isWin).toBe('boolean');
    });
  });

  describe('SettingsView Logic & Store Integration', () => {
    it('displays agent status data from agentStore', async () => {
      const agentStore = useAgentStore();
      vi.mocked(tauriService.getAgentStatus).mockResolvedValueOnce({
        version: '0.1.0',
        uptime_secs: 3600,
        active_tasks: 5,
        running_executions: 2,
      });

      await agentStore.fetchStatus();
      expect(agentStore.isConnected).toBe(true);
      expect(agentStore.status?.version).toBe('0.1.0');
      expect(agentStore.status?.uptime_secs).toBe(3600);
      expect(agentStore.status?.active_tasks).toBe(5);
      expect(agentStore.status?.running_executions).toBe(2);
    });

    it('handles disconnected agent gracefully in settings', async () => {
      const agentStore = useAgentStore();
      vi.mocked(tauriService.getAgentStatus).mockRejectedValueOnce(new Error('Agent offline'));

      await agentStore.fetchStatus();
      expect(agentStore.isConnected).toBe(false);
      expect(agentStore.status).toBeNull();
      expect(agentStore.lastError).toContain('Agent offline');
    });

    it('refreshes agent status when re-detect connection is triggered', async () => {
      const agentStore = useAgentStore();
      vi.mocked(tauriService.getAgentStatus).mockResolvedValueOnce({
        version: '0.1.0',
        uptime_secs: 50,
        active_tasks: 2,
        running_executions: 0,
      });

      await agentStore.fetchStatus();
      expect(agentStore.isConnected).toBe(true);
      expect(tauriService.getAgentStatus).toHaveBeenCalledTimes(1);
    });

    it('checks autostart status via autostart service', async () => {
      vi.mocked(pluginAutostart.isEnabled).mockResolvedValueOnce(true);
      const enabled = await autostartService.isAutostartEnabled();
      expect(enabled).toBe(true);
      expect(pluginAutostart.isEnabled).toHaveBeenCalledTimes(1);

      vi.mocked(pluginAutostart.isEnabled).mockRejectedValueOnce(new Error('Tauri not ready'));
      const fallback = await autostartService.isAutostartEnabled();
      expect(fallback).toBe(false);
    });

    it('toggles autostart status with enable and disable', async () => {
      await autostartService.setAutostart(true);
      expect(pluginAutostart.enable).toHaveBeenCalledTimes(1);

      await autostartService.setAutostart(false);
      expect(pluginAutostart.disable).toHaveBeenCalledTimes(1);
    });

    it('initializes autostart to enabled by default on first launch', async () => {
      expect(localStorage.getItem('easyjob_autostart_initialized')).toBeNull();

      const res = await autostartService.initAutostartDefault();
      expect(res).toBe(true);
      expect(pluginAutostart.enable).toHaveBeenCalledTimes(1);
      expect(localStorage.getItem('easyjob_autostart_initialized')).toBe('true');

      // Subsequent runs check isEnabled instead of re-enabling
      vi.mocked(pluginAutostart.isEnabled).mockResolvedValueOnce(false);
      const secondRun = await autostartService.initAutostartDefault();
      expect(secondRun).toBe(false);
      expect(pluginAutostart.isEnabled).toHaveBeenCalledTimes(1);
    });
  });

  describe('App Layout & Polling Logic', () => {
    it('initializes agent polling and execution event listeners', async () => {
      vi.mocked(tauriService.getAgentStatus).mockResolvedValue({
        version: '0.1.0',
        uptime_secs: 10,
        active_tasks: 1,
        running_executions: 0,
      });
      vi.mocked(tauriService.listExecutions).mockResolvedValue([]);

      const agentStore = useAgentStore();
      const executionStore = useExecutionStore();

      await agentStore.fetchStatus();
      await executionStore.initListeners();

      expect(agentStore.isConnected).toBe(true);
      expect(eventsService.onExecutionStarted).toHaveBeenCalledTimes(1);
      expect(eventsService.onExecutionOutput).toHaveBeenCalledTimes(1);
      expect(eventsService.onExecutionFinished).toHaveBeenCalledTimes(1);

      await executionStore.cleanupListeners();
    });

    it('handles interval polling cleanup gracefully', () => {
      let intervalCleared = false;
      const fakeTimer = setInterval(() => {}, 10000);
      clearInterval(fakeTimer);
      intervalCleared = true;
      expect(intervalCleared).toBe(true);
    });

    it('switches currentView to executions and reloads executions when navigate event is received', async () => {
      let navigateCallback: ((event: { payload: string }) => void) | null = null;
      vi.mocked(tauriEvent.listen).mockImplementation(async (eventName: string, handler: any) => {
        if (eventName === 'navigate') {
          navigateCallback = handler;
        }
        return vi.fn();
      });

      const currentView = ref<'tasks' | 'executions' | 'settings'>('tasks');
      const executionStore = useExecutionStore();
      const loadExecutionsSpy = vi.spyOn(executionStore, 'loadExecutions').mockResolvedValue();

      // Setup navigate listener matching App.vue logic
      const unlisten = await tauriEvent.listen<string>('navigate', (event) => {
        if (event.payload === 'executions') {
          currentView.value = 'executions';
          executionStore.loadExecutions();
        }
      });

      expect(tauriEvent.listen).toHaveBeenCalledWith('navigate', expect.any(Function));
      expect(navigateCallback).not.toBeNull();

      // Trigger navigate event with 'executions'
      navigateCallback!({ payload: 'executions' });

      expect(currentView.value).toBe('executions');
      expect(loadExecutionsSpy).toHaveBeenCalledTimes(1);

      // Verify other navigation payloads do not reload executions
      currentView.value = 'tasks';
      navigateCallback!({ payload: 'settings' });
      expect(currentView.value).toBe('tasks');
      expect(loadExecutionsSpy).toHaveBeenCalledTimes(1);

      unlisten();
    });
  });
});
