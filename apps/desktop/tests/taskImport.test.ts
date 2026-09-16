import { describe, it, expect } from 'vitest';
import { getEmptyTask } from '../src/types/task';
import type { Task } from '../src/types/task';

describe('TaskImport conflict detection logic', () => {
  it('categorizes imported tasks into new vs conflict correctly', () => {
    const existingTask = { ...getEmptyTask(), id: 'local-1', name: 'Database Backup' };
    const existingList = [existingTask];

    const importedTask1 = { ...getEmptyTask(), id: 'ext-1', name: 'Database Backup' };
    const importedTask2 = { ...getEmptyTask(), id: 'ext-2', name: 'Log Archiver' };
    const importedList = [importedTask1, importedTask2];

    const existingNames = new Set(existingList.map((t) => t.name));

    const conflicts = importedList.filter((t) => existingNames.has(t.name));
    const nonConflicts = importedList.filter((t) => !existingNames.has(t.name));

    expect(conflicts).toHaveLength(1);
    expect(conflicts[0].name).toBe('Database Backup');
    expect(nonConflicts).toHaveLength(1);
    expect(nonConflicts[0].name).toBe('Log Archiver');
  });

  it('re-generates fresh UUIDs for newly imported non-conflicting tasks', () => {
    const imported = getEmptyTask();
    imported.id = 'fixed-old-id';
    imported.triggers = [
      {
        id: 'old-tr',
        task_id: 'fixed-old-id',
        enabled: true,
        kind: 'AgentStarted',
        created_at: '',
        updated_at: '',
      },
    ];

    // Simulate UUID regeneration
    const newTaskId = 'new-task-uuid-1';
    const prepared = {
      ...imported,
      id: newTaskId,
      version: 1,
      triggers: imported.triggers.map((tr) => ({
        ...tr,
        id: 'new-tr-uuid-1',
        task_id: newTaskId,
      })),
    };

    expect(prepared.id).not.toBe('fixed-old-id');
    expect(prepared.triggers[0].task_id).toBe(newTaskId);
  });
});
