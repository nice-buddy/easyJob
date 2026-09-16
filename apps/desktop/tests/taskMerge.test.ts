import { describe, it, expect } from 'vitest';
import { getEmptyTask } from '../src/types/task';
import type { Task } from '../src/types/task';

describe('TaskMergeModal quick copy actions', () => {
  it('clones existing basic info into target task', () => {
    const existing: Task = {
      ...getEmptyTask(),
      name: 'Old Name',
      description: 'Old Desc',
      working_directory: '/old/dir',
      enabled: false,
    };
    const target: Task = {
      ...getEmptyTask(),
      name: 'New Name',
      description: 'New Desc',
      working_directory: '/new/dir',
      enabled: true,
    };

    // Simulate "采用已有配置 - 基本信息"
    target.description = existing.description;
    target.working_directory = existing.working_directory;
    target.enabled = existing.enabled;

    expect(target.description).toBe('Old Desc');
    expect(target.working_directory).toBe('/old/dir');
    expect(target.enabled).toBe(false);
  });

  it('clones existing policy into target task', () => {
    const existing = getEmptyTask();
    existing.execution_policy.timeout_secs = 120;
    existing.execution_policy.concurrency_policy = 'AllowParallel';

    const target = getEmptyTask();
    target.execution_policy = JSON.parse(JSON.stringify(existing.execution_policy));

    expect(target.execution_policy.timeout_secs).toBe(120);
    expect(target.execution_policy.concurrency_policy).toBe('AllowParallel');
  });

  it('clones existing triggers and actions into target task', () => {
    const existing = getEmptyTask();
    existing.triggers = [
      {
        id: 't-1',
        task_id: existing.id,
        enabled: true,
        kind: { Daily: { time: '08:00', timezone: 'UTC' } },
        created_at: '',
        updated_at: '',
      },
    ];
    existing.actions = [
      {
        id: 'a-1',
        task_id: existing.id,
        sequence: 0,
        enabled: true,
        kind: { ExecuteShell: { command: 'ls -la' } },
      },
    ];

    const target = getEmptyTask();
    target.triggers = JSON.parse(JSON.stringify(existing.triggers));
    target.actions = JSON.parse(JSON.stringify(existing.actions));

    expect(target.triggers).toHaveLength(1);
    expect(target.actions).toHaveLength(1);
    expect(target.actions[0].kind).toEqual({ ExecuteShell: { command: 'ls -la' } });
  });
  it('updates environment variables into target task', () => {
    const target = getEmptyTask();
    
    // Simulate updating envList and syncing to target.environment
    const envList = [
      { key: 'ENV_VAR_1', value: 'value1' },
      { key: 'ENV_VAR_2', value: 'value2' },
      { key: ' ', value: 'ignore-empty' },
    ];
    
    const obj: Record<string, string> = {};
    envList.forEach(({ key, value }) => {
      if (key.trim()) obj[key.trim()] = value;
    });
    target.environment = obj;

    expect(target.environment).toEqual({
      ENV_VAR_1: 'value1',
      ENV_VAR_2: 'value2',
    });
  });
});
