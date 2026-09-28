import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import { exportTasksAndReveal, exportTasksJson, revealInFileManager } from '../src/services/tauri';

describe('export and reveal service wrappers', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('calls the export command with filename and contents', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('/Users/me/Downloads/easyjob-tasks.json');
    const result = await exportTasksJson('easyjob-tasks.json', '{"tasks":[]}');
    expect(result).toBe('/Users/me/Downloads/easyjob-tasks.json');
    expect(invoke).toHaveBeenCalledWith('export_tasks_json', {
      filename: 'easyjob-tasks.json',
      contents: '{"tasks":[]}',
    });
  });

  it('calls the reveal command with the exported path', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await revealInFileManager('/Users/me/Downloads/easyjob-tasks.json');
    expect(invoke).toHaveBeenCalledWith('reveal_in_file_manager', {
      path: '/Users/me/Downloads/easyjob-tasks.json',
    });
  });

  it('keeps export success when reveal fails', async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce('/Users/me/Downloads/easyjob-tasks.json')
      .mockRejectedValueOnce(new Error('no file manager'));
    const result = await exportTasksAndReveal('easyjob-tasks.json', '{"tasks":[]}');
    expect(result.path).toBe('/Users/me/Downloads/easyjob-tasks.json');
    expect(result.revealed).toBe(false);
    expect(result.revealError).toContain('no file manager');
  });

  it('reports reveal success when both steps succeed', async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce('/Users/me/Downloads/easyjob-tasks.json')
      .mockResolvedValueOnce(undefined);
    const result = await exportTasksAndReveal('easyjob-tasks.json', '{"tasks":[]}');
    expect(result).toEqual({
      path: '/Users/me/Downloads/easyjob-tasks.json',
      revealed: true,
    });
  });
});
