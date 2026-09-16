import { describe, it, expect, beforeEach, vi } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { getEmptyTask } from '../src/types/task';
import type { Task } from '../src/types/task';

describe('Task Export payload builder', () => {
  it('generates standard export payload format with metadata', () => {
    const task1 = { ...getEmptyTask(), id: 'task-1', name: 'Backup' };
    const task2 = { ...getEmptyTask(), id: 'task-2', name: 'Cleanup' };
    const selected = [task1, task2];

    const payload = {
      easyjob_version: '0.1.0',
      exported_at: new Date().toISOString(),
      tasks: selected,
    };

    expect(payload.easyjob_version).toBe('0.1.0');
    expect(payload.tasks).toHaveLength(2);
    expect(payload.tasks[0].name).toBe('Backup');
    expect(payload.tasks[1].name).toBe('Cleanup');
  });

  it('filters selected tasks correctly from ID set', () => {
    const task1 = { ...getEmptyTask(), id: 'task-1', name: 'T1' };
    const task2 = { ...getEmptyTask(), id: 'task-2', name: 'T2' };
    const all = [task1, task2];
    const selectedIds = new Set(['task-2']);

    const exported = all.filter((t) => selectedIds.has(t.id));
    expect(exported).toHaveLength(1);
    expect(exported[0].id).toBe('task-2');
  });
});
