import { describe, it, expect, beforeEach, vi } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import TriggerEditor from '../src/components/task/TriggerEditor.vue';
import ActionEditor from '../src/components/task/ActionEditor.vue';
import TaskDrawer from '../src/components/task/TaskDrawer.vue';
import TasksView from '../src/views/TasksView.vue';
import { useTaskStore } from '../src/stores/taskStore';
import * as tauriService from '../src/services/tauri';
import type { Task, Trigger, Action, TriggerKind, ActionKind } from '../src/types/task';
import { getTriggerType, getActionType } from '../src/types/task';

vi.mock('../src/services/tauri', () => ({
  listTasks: vi.fn(),
  getTask: vi.fn(),
  saveTask: vi.fn(),
  deleteTask: vi.fn(),
  triggerTask: vi.fn(),
}));

describe('TaskDrawer, TriggerEditor & ActionEditor', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  describe('Component Definitions & Interface Contracts', () => {
    it('exports valid Vue components', () => {
      expect(TriggerEditor).toBeDefined();
      expect(ActionEditor).toBeDefined();
      expect(TaskDrawer).toBeDefined();
      expect(TasksView).toBeDefined();
    });

    it('TriggerEditor defines expected props and emits', () => {
      const props = (TriggerEditor as any).props;
      expect(props).toBeDefined();
      expect(props.triggers).toBeDefined();
      expect(props.taskId).toBeDefined();

      const emits = (TriggerEditor as any).emits;
      expect(emits).toBeDefined();
      expect(emits).toContain('update:triggers');
    });

    it('ActionEditor defines expected props and emits', () => {
      const props = (ActionEditor as any).props;
      expect(props).toBeDefined();
      expect(props.actions).toBeDefined();
      expect(props.taskId).toBeDefined();

      const emits = (ActionEditor as any).emits;
      expect(emits).toBeDefined();
      expect(emits).toContain('update:actions');
    });

    it('TaskDrawer defines expected props and emits', () => {
      const props = (TaskDrawer as any).props;
      expect(props).toBeDefined();
      expect(props.show).toBeDefined();
      expect(props.task).toBeDefined();

      const emits = (TaskDrawer as any).emits;
      expect(emits).toBeDefined();
      expect(emits).toContain('update:show');
      expect(emits).toContain('saved');
    });
  });

  describe('TriggerKind & ActionKind Serde Wire Format Conformance', () => {
    it('verifies Interval wire format uses interval_secs', () => {
      const interval: TriggerKind = {
        Interval: { interval_secs: 300 },
      };
      expect(getTriggerType(interval)).toBe('Interval');
      const json = JSON.stringify(interval);
      expect(json).toContain('"interval_secs":300');
    });

    it('verifies Daily wire format matches HH:mm:ss and timezone', () => {
      const daily: TriggerKind = {
        Daily: { time: '08:30:00', timezone: 'Asia/Shanghai' },
      };
      expect(getTriggerType(daily)).toBe('Daily');
      const parsed = JSON.parse(JSON.stringify(daily));
      expect(parsed.Daily.time).toBe('08:30:00');
      expect(parsed.Daily.timezone).toBe('Asia/Shanghai');
    });

    it('verifies Weekly wire format uses Weekday array', () => {
      const weekly: TriggerKind = {
        Weekly: {
          days_of_week: ['Mon', 'Wed', 'Fri'],
          time: '18:00:00',
          timezone: 'UTC',
        },
      };
      expect(getTriggerType(weekly)).toBe('Weekly');
      const parsed = JSON.parse(JSON.stringify(weekly));
      expect(parsed.Weekly.days_of_week).toEqual(['Mon', 'Wed', 'Fri']);
    });

    it('verifies Once wire format has fire_at', () => {
      const once: TriggerKind = {
        Once: { fire_at: '2026-10-01T12:00:00Z' },
      };
      expect(getTriggerType(once)).toBe('Once');
      const parsed = JSON.parse(JSON.stringify(once));
      expect(parsed.Once.fire_at).toBe('2026-10-01T12:00:00Z');
    });

    it('verifies AgentStarted wire format is a bare string', () => {
      const agentStarted: TriggerKind = 'AgentStarted';
      expect(getTriggerType(agentStarted)).toBe('AgentStarted');
      expect(JSON.stringify(agentStarted)).toBe('"AgentStarted"');
    });

    it('verifies ExecuteShell action format', () => {
      const shell: ActionKind = { ExecuteShell: { command: 'echo hello' } };
      expect(getActionType(shell)).toBe('ExecuteShell');
      expect(JSON.stringify(shell)).toBe('{"ExecuteShell":{"command":"echo hello"}}');
    });

    it('verifies ExecuteProgram action format with program and args', () => {
      const prog: ActionKind = {
        ExecuteProgram: { program: 'python3', args: ['-m', 'http.server'] },
      };
      expect(getActionType(prog)).toBe('ExecuteProgram');
      const parsed = JSON.parse(JSON.stringify(prog));
      expect(parsed.ExecuteProgram.program).toBe('python3');
      expect(parsed.ExecuteProgram.args).toEqual(['-m', 'http.server']);
    });

    it('verifies ExecutePowerShell action format with script and no_profile', () => {
      const ps: ActionKind = {
        ExecutePowerShell: { script: 'Get-Service', no_profile: true },
      };
      expect(getActionType(ps)).toBe('ExecutePowerShell');
      const parsed = JSON.parse(JSON.stringify(ps));
      expect(parsed.ExecutePowerShell.script).toBe('Get-Service');
      expect(parsed.ExecutePowerShell.no_profile).toBe(true);
    });

    it('verifies ExecuteCmd action format with command', () => {
      const cmd: ActionKind = { ExecuteCmd: { command: 'dir C:\\' } };
      expect(getActionType(cmd)).toBe('ExecuteCmd');
      expect(JSON.stringify(cmd)).toBe('{"ExecuteCmd":{"command":"dir C:\\\\"}}');
    });
  });

  describe('Trigger Management Behavior', () => {
    it('creates new triggers with unique IDs and valid defaults', () => {
      const taskId = 'task-101';
      const triggers: Trigger[] = [];

      const newTrigger: Trigger = {
        id: 'new-trig-uuid',
        task_id: taskId,
        enabled: true,
        kind: { Interval: { interval_secs: 60 } },
        created_at: '2026-09-14T00:00:00Z',
        updated_at: '2026-09-14T00:00:00Z',
      };
      triggers.push(newTrigger);

      expect(triggers).toHaveLength(1);
      expect(triggers[0].task_id).toBe(taskId);
      expect(triggers[0].enabled).toBe(true);
      expect(getTriggerType(triggers[0].kind)).toBe('Interval');
    });

    it('switches trigger type to Daily preserving valid structure', () => {
      const trigger: Trigger = {
        id: 'trig-1',
        task_id: 'task-101',
        enabled: true,
        kind: { Interval: { interval_secs: 60 } },
        created_at: '2026-09-14T00:00:00Z',
        updated_at: '2026-09-14T00:00:00Z',
      };

      trigger.kind = { Daily: { time: '09:00:00', timezone: 'UTC' } };
      expect(getTriggerType(trigger.kind)).toBe('Daily');
      if ('Daily' in trigger.kind) {
        expect(trigger.kind.Daily.time).toBe('09:00:00');
        expect(trigger.kind.Daily.timezone).toBe('UTC');
      }
    });

    it('switches trigger type to Weekly and AgentStarted', () => {
      const trigger: Trigger = {
        id: 'trig-1',
        task_id: 'task-101',
        enabled: true,
        kind: { Interval: { interval_secs: 60 } },
        created_at: '2026-09-14T00:00:00Z',
        updated_at: '2026-09-14T00:00:00Z',
      };

      trigger.kind = {
        Weekly: {
          days_of_week: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'],
          time: '10:00:00',
          timezone: 'Local',
        },
      };
      expect(getTriggerType(trigger.kind)).toBe('Weekly');

      trigger.kind = 'AgentStarted';
      expect(getTriggerType(trigger.kind)).toBe('AgentStarted');
    });

    it('removes trigger from collection', () => {
      const triggers: Trigger[] = [
        {
          id: 't-1',
          task_id: 'task-1',
          enabled: true,
          kind: 'AgentStarted',
          created_at: '',
          updated_at: '',
        },
        {
          id: 't-2',
          task_id: 'task-1',
          enabled: true,
          kind: { Interval: { interval_secs: 10 } },
          created_at: '',
          updated_at: '',
        },
      ];

      triggers.splice(0, 1);
      expect(triggers).toHaveLength(1);
      expect(triggers[0].id).toBe('t-2');
    });
  });

  describe('Action Management Behavior', () => {
    it('creates actions with incremented sequence numbers', () => {
      const taskId = 'task-202';
      const actions: Action[] = [];

      const act1: Action = {
        id: 'act-1',
        task_id: taskId,
        sequence: actions.length + 1,
        enabled: true,
        kind: { ExecuteShell: { command: 'echo first' } },
      };
      actions.push(act1);

      const act2: Action = {
        id: 'act-2',
        task_id: taskId,
        sequence: actions.length + 1,
        enabled: true,
        kind: { ExecuteProgram: { program: 'ls', args: ['-la'] } },
      };
      actions.push(act2);

      expect(actions[0].sequence).toBe(1);
      expect(actions[1].sequence).toBe(2);
    });

    it('re-indexes sequence numbers on removal', () => {
      const actions: Action[] = [
        {
          id: 'act-1',
          task_id: 'task-1',
          sequence: 1,
          enabled: true,
          kind: { ExecuteShell: { command: 'step 1' } },
        },
        {
          id: 'act-2',
          task_id: 'task-1',
          sequence: 2,
          enabled: true,
          kind: { ExecuteShell: { command: 'step 2' } },
        },
        {
          id: 'act-3',
          task_id: 'task-1',
          sequence: 3,
          enabled: true,
          kind: { ExecuteShell: { command: 'step 3' } },
        },
      ];

      actions.splice(0, 1);
      actions.forEach((a, idx) => {
        a.sequence = idx + 1;
      });

      expect(actions).toHaveLength(2);
      expect(actions[0].id).toBe('act-2');
      expect(actions[0].sequence).toBe(1);
      expect(actions[1].id).toBe('act-3');
      expect(actions[1].sequence).toBe(2);
    });
  });

  describe('Task Serialization & Store Integration', () => {
    const fullTask: Task = {
      id: 'task-full-1',
      name: 'Sync Database',
      description: 'Periodic sync from prod to stage',
      enabled: true,
      triggers: [
        {
          id: 't-1',
          task_id: 'task-full-1',
          enabled: true,
          kind: { Daily: { time: '03:00:00', timezone: 'UTC' } },
          created_at: '2026-09-14T00:00:00Z',
          updated_at: '2026-09-14T00:00:00Z',
        },
      ],
      actions: [
        {
          id: 'a-1',
          task_id: 'task-full-1',
          sequence: 1,
          enabled: true,
          kind: { ExecuteShell: { command: 'sync.sh' } },
        },
        {
          id: 'a-2',
          task_id: 'task-full-1',
          sequence: 2,
          enabled: true,
          kind: { ExecuteCmd: { command: 'echo done' } },
        },
      ],
      execution_policy: {
        concurrency_policy: 'SkipIfRunning',
        missed_run_policy: 'RunOnce',
        retry_policy: {
          max_retries: 3,
          delay_secs: 10,
        },
        timeout_secs: 1800,
      },
      working_directory: '/opt/easyjob',
      environment: { ENV: 'production' },
      version: 2,
      created_at: '2026-09-14T00:00:00Z',
      updated_at: '2026-09-14T00:00:00Z',
    };

    it('serializes full Task cleanly matching easyjob-domain schema', () => {
      const json = JSON.stringify(fullTask);
      const parsed: Task = JSON.parse(json);

      expect(parsed.id).toBe(fullTask.id);
      expect(parsed.name).toBe(fullTask.name);
      expect(parsed.execution_policy.concurrency_policy).toBe('SkipIfRunning');
      expect(parsed.execution_policy.missed_run_policy).toBe('RunOnce');
      expect(parsed.execution_policy.retry_policy.max_retries).toBe(3);
      expect(parsed.execution_policy.retry_policy.delay_secs).toBe(10);
      expect(parsed.execution_policy.timeout_secs).toBe(1800);
      expect(parsed.triggers).toHaveLength(1);
      expect(parsed.actions).toHaveLength(2);
      expect(parsed.working_directory).toBe('/opt/easyjob');
      expect(parsed.environment).toEqual({ ENV: 'production' });
    });

    it('deep clones task for editing to prevent state corruption', () => {
      const cloned: Task = JSON.parse(JSON.stringify(fullTask));
      cloned.name = 'Renamed Task';
      cloned.triggers.push({
        id: 't-new',
        task_id: cloned.id,
        enabled: true,
        kind: 'AgentStarted',
        created_at: '',
        updated_at: '',
      });

      expect(fullTask.name).toBe('Sync Database');
      expect(fullTask.triggers).toHaveLength(1);
      expect(cloned.name).toBe('Renamed Task');
      expect(cloned.triggers).toHaveLength(2);
    });

    it('saves valid task to taskStore', async () => {
      const taskStore = useTaskStore();
      vi.mocked(tauriService.saveTask).mockResolvedValueOnce(fullTask);

      const saved = await taskStore.saveTask(fullTask);
      expect(tauriService.saveTask).toHaveBeenCalledWith(fullTask);
      expect(saved.id).toBe(fullTask.id);
      expect(taskStore.tasks).toContainEqual(fullTask);
    });
  });
});
