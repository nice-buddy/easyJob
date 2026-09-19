import { describe, it, expect } from 'vitest';
import { cloneTaskForDuplicate, getEmptyTask, type Task } from '../src/types/task';

function sampleTask(): Task {
  return {
    ...getEmptyTask(),
    id: 'task-original',
    name: '每日备份',
    description: '备份数据库',
    enabled: true,
    triggers: [
      {
        id: 'trig-1',
        task_id: 'task-original',
        enabled: true,
        kind: { Daily: { time: '09:00:00', timezone: 'UTC' } },
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
      },
      {
        id: 'trig-2',
        task_id: 'task-original',
        enabled: false,
        kind: 'AgentStarted',
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
      },
    ],
    actions: [
      {
        id: 'act-1',
        task_id: 'task-original',
        sequence: 1,
        enabled: true,
        kind: { ExecuteShell: { command: 'echo hi' } },
      },
    ],
    version: 4,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-02-01T00:00:00Z',
  };
}

describe('cloneTaskForDuplicate', () => {
  it('rebuilds ids, renames, disables and resets version and timestamps', () => {
    const original = sampleTask();
    const snapshot = JSON.stringify(original);
    const now = new Date(2026, 8, 19, 10, 0, 0);

    const copy = cloneTaskForDuplicate(original, now);

    expect(copy.id).not.toBe(original.id);
    expect(copy.name).toBe('每日备份 - 副本');
    expect(copy.enabled).toBe(false);
    expect(copy.version).toBe(1);
    expect(copy.created_at).toBe(now.toISOString());
    expect(copy.updated_at).toBe(now.toISOString());

    expect(copy.triggers).toHaveLength(2);
    expect(copy.triggers.every((trigger) => trigger.task_id === copy.id)).toBe(true);
    expect(copy.triggers.map((trigger) => trigger.id)).not.toContain('trig-1');
    expect(copy.triggers.map((trigger) => trigger.id)).not.toContain('trig-2');
    // 触发器自身的启用状态沿用原任务
    expect(copy.triggers[0].enabled).toBe(true);
    expect(copy.triggers[1].enabled).toBe(false);

    expect(copy.actions).toHaveLength(1);
    expect(copy.actions[0].task_id).toBe(copy.id);
    expect(copy.actions[0].id).not.toBe('act-1');
    expect(copy.actions[0].kind).toEqual({ ExecuteShell: { command: 'echo hi' } });

    // 其余字段与原任务一致
    expect(copy.description).toBe(original.description);
    expect(copy.working_directory).toBe(original.working_directory);
    expect(copy.environment).toEqual(original.environment);
    expect(copy.execution_policy).toEqual(original.execution_policy);

    // 原任务对象未被修改
    expect(JSON.stringify(original)).toBe(snapshot);
  });

  it('always disables the copy even when the original task is enabled', () => {
    const original = { ...sampleTask(), enabled: true };
    expect(cloneTaskForDuplicate(original).enabled).toBe(false);
  });
});
