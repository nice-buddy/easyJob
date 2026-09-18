import { describe, it, expect } from 'vitest';
import { compareTasks } from '../src/utils/taskDiff';
import { getEmptyTask } from '../src/types/task';
import type { Task } from '../src/types/task';

describe('taskDiff utility', () => {
  it('returns hasDiff=false when tasks are identical in configuration', () => {
    const t1 = getEmptyTask();
    t1.name = 'Test Task';
    t1.description = 'Identical description';
    const t2 = JSON.parse(JSON.stringify(t1));

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(false);
    expect(diff.basicDiff).toBe(false);
    expect(diff.policyDiff).toBe(false);
    expect(diff.triggersDiff).toBe(false);
    expect(diff.actionsDiff).toBe(false);
    expect(diff.envDiff).toBe(false);
    expect(diff.diffFields.size).toBe(0);
  });

  it('detects basic info differences', () => {
    const t1 = getEmptyTask();
    t1.description = 'Old description';
    const t2 = JSON.parse(JSON.stringify(t1));
    t2.description = 'New description';
    t2.working_directory = '/tmp/new';

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.basicDiff).toBe(true);
    expect(diff.diffFields.has('basic.description')).toBe(true);
    expect(diff.diffFields.has('basic.working_directory')).toBe(true);
    expect(diff.policyDiff).toBe(false);
  });

  it('detects execution policy differences', () => {
    const t1 = getEmptyTask();
    t1.execution_policy.timeout_secs = 3600;
    const t2 = JSON.parse(JSON.stringify(t1));
    t2.execution_policy.timeout_secs = 7200;
    t2.execution_policy.concurrency_policy = 'AllowParallel';

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.policyDiff).toBe(true);
    expect(diff.diffFields.has('policy.timeout_secs')).toBe(true);
    expect(diff.diffFields.has('policy.concurrency_policy')).toBe(true);
  });

  it('detects triggers and actions differences', () => {
    const t1 = getEmptyTask();
    t1.triggers = [
      {
        id: 'tr-1',
        task_id: t1.id,
        enabled: true,
        kind: { Interval: { seconds: 60 } },
        created_at: '',
        updated_at: '',
      },
    ];
    t1.actions = [
      {
        id: 'act-1',
        task_id: t1.id,
        sequence: 0,
        enabled: true,
        kind: { ExecuteShell: { command: 'echo 1' } },
      },
    ];

    const t2 = JSON.parse(JSON.stringify(t1));
    t2.triggers[0].kind = { Interval: { seconds: 120 } };
    t2.actions.push({
      id: 'act-2',
      task_id: t2.id,
      sequence: 1,
      enabled: true,
      kind: { ExecuteShell: { command: 'echo 2' } },
    });

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.triggersDiff).toBe(true);
    expect(diff.actionsDiff).toBe(true);
    expect(diff.diffFields.has('triggers')).toBe(true);
    expect(diff.diffFields.has('actions')).toBe(true);
  });

  it('detects environment variable differences', () => {
    const t1 = getEmptyTask();
    t1.environment = { FOO: 'bar' };
    const t2 = JSON.parse(JSON.stringify(t1));
    t2.environment = { FOO: 'baz', EXTRA: '123' };

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.envDiff).toBe(true);
    expect(diff.diffFields.has('env.FOO')).toBe(true);
    expect(diff.diffFields.has('env.EXTRA')).toBe(true);
  });

  it('detects notification policy differences', () => {
    const t1 = getEmptyTask();
    t1.execution_policy.notification = 'None';
    const t2 = JSON.parse(JSON.stringify(t1));
    t2.execution_policy.notification = 'OnlyFailure';

    const diff = compareTasks(t1, t2);
    expect(diff.hasDiff).toBe(true);
    expect(diff.policyDiff).toBe(true);
    expect(diff.diffFields.has('policy.notification')).toBe(true);
  });

  it('should detect differences in policy.log_retention', () => {
    const taskA = getEmptyTask();
    taskA.execution_policy.log_retention = { mode: 'SystemDefault' };

    const taskB = getEmptyTask();
    taskB.execution_policy.log_retention = { mode: 'KeepDays', days: 14 };

    const res1 = compareTasks(taskA, taskB);
    expect(res1.hasDiff).toBe(true);
    expect(res1.policyDiff).toBe(true);
    expect(res1.diffFields.has('policy.log_retention')).toBe(true);

    // 相同 KeepDays 但天数不同
    const taskC = getEmptyTask();
    taskC.execution_policy.log_retention = { mode: 'KeepDays', days: 30 };
    const res2 = compareTasks(taskB, taskC);
    expect(res2.diffFields.has('policy.log_retention')).toBe(true);

    // 缺失 log_retention 时的 fallback 兼容
    const legacyTask = getEmptyTask();
    delete (legacyTask.execution_policy as any).log_retention;
    const res3 = compareTasks(taskA, legacyTask);
    expect(res3.diffFields.has('policy.log_retention')).toBe(false);
  });
});
