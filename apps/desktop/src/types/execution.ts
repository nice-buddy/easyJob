import type { TaskId, TriggerId } from './task';

export type ExecutionId = string;

export type ExecutionStatus =
  | 'Queued'
  | 'Pending'
  | 'Running'
  | 'Succeeded'
  | 'Failed'
  | 'TimedOut'
  | 'Cancelled'
  | 'Skipped'
  | 'Interrupted';

export interface Execution {
  id: ExecutionId;
  task_id: TaskId;
  trigger_id: TriggerId | null;
  status: ExecutionStatus;
  scheduled_at: string | null;
  started_at: string;
  finished_at: string | null;
  duration_ms: number | null;
  exit_code: number | null;
  error_message: string | null;
}

export interface ExecutionOutputPayload {
  execution_id: ExecutionId;
  task_id: TaskId;
  stream: 'stdout' | 'stderr';
  content: string;
}
