import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { ExecutionOutputPayload } from '../types/execution';

export async function onExecutionStarted(
  cb: (payload: { execution_id: string; task_id: string }) => void
): Promise<UnlistenFn> {
  return await listen<{ execution_id: string; task_id: string }>('execution.started', (e) => cb(e.payload));
}

export async function onExecutionOutput(
  cb: (payload: ExecutionOutputPayload) => void
): Promise<UnlistenFn> {
  return await listen<ExecutionOutputPayload>('execution.output', (e) => cb(e.payload));
}

export async function onExecutionFinished(
  cb: (payload: { execution_id: string; task_id: string; status: string; exit_code: number | null }) => void
): Promise<UnlistenFn> {
  return await listen<{ execution_id: string; task_id: string; status: string; exit_code: number | null }>(
    'execution.finished',
    (e) => cb(e.payload)
  );
}
