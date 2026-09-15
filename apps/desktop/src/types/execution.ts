import type { TaskId, TriggerId } from './task';

export type ExecutionId = string;

export type ExecutionStatus =
  | 'Queued'
  | 'Running'
  | 'Succeeded'
  | 'Failed'
  | 'TimedOut'
  | 'Cancelled'
  | 'Skipped'
  | 'Interrupted';

export function getStatusLabel(status?: string): string {
  switch (status) {
    case 'Running':
      return '运行中';
    case 'Succeeded':
      return '成功';
    case 'Failed':
      return '失败';
    case 'TimedOut':
      return '超时';
    case 'Cancelled':
      return '已取消';
    case 'Queued':
      return '排队中';
    case 'Skipped':
      return '已跳过';
    case 'Interrupted':
      return '异常中断';
    default:
      return status || '-';
  }
}

export function getStatusTagType(
  status?: string
): 'info' | 'success' | 'error' | 'warning' | 'default' {
  switch (status) {
    case 'Running':
      return 'info';
    case 'Succeeded':
      return 'success';
    case 'Failed':
      return 'error';
    case 'TimedOut':
      return 'warning';
    case 'Cancelled':
      return 'default';
    case 'Interrupted':
      return 'warning';
    default:
      return 'default';
  }
}

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
