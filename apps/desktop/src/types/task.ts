export type TaskId = string;
export type TriggerId = string;
export type ActionId = string;

export type ConcurrencyPolicy = 'AllowParallel' | 'SkipIfRunning' | 'QueueOne';
export type MissedRunPolicy = 'RunOnce' | 'Skip';
export type TaskNotificationPolicy = 'None' | 'OnlySuccess' | 'OnlyFailure' | 'All';

export interface RetryPolicy {
  max_retries: number;
  delay_secs: number;
}

export type LogRetentionPolicy =
  | { mode: 'SystemDefault' }
  | { mode: 'KeepDays'; days: number }
  | { mode: 'Permanent' };

export type SystemLogRetention =
  | { mode: 'KeepDays'; days: number }
  | { mode: 'Permanent' };

export interface SystemSettings {
  default_log_retention: SystemLogRetention;
}

export interface ExecutionPolicy {
  concurrency_policy: ConcurrencyPolicy;
  missed_run_policy: MissedRunPolicy;
  retry_policy: RetryPolicy;
  timeout_secs: number | null;
  notification: TaskNotificationPolicy;
  log_retention: LogRetentionPolicy;
}

export type Weekday = 'Mon' | 'Tue' | 'Wed' | 'Thu' | 'Fri' | 'Sat' | 'Sun';

export type TriggerKind =
  | { Once: { fire_at: string } }
  | { Interval: { seconds: number; interval_secs?: number; start_at?: string | null } | { interval_secs: number; seconds?: number; start_at?: string | null } }
  | { Daily: { time: string; timezone: string } }
  | { Weekly: { days_of_week: Weekday[]; time: string; timezone: string } }
  | 'AgentStarted';

export function getTriggerType(
  kind: TriggerKind
): 'Once' | 'Interval' | 'Daily' | 'Weekly' | 'AgentStarted' {
  if (typeof kind === 'string') {
    return kind;
  }
  if ('Once' in kind) return 'Once';
  if ('Interval' in kind) return 'Interval';
  if ('Daily' in kind) return 'Daily';
  if ('Weekly' in kind) return 'Weekly';
  throw new Error(`Unknown trigger kind: ${JSON.stringify(kind)}`);
}

export interface Trigger {
  id: TriggerId;
  task_id: TaskId;
  enabled: boolean;
  kind: TriggerKind;
  created_at: string;
  updated_at: string;
}

export type ActionKind =
  | { ExecuteProgram: { program: string; args: string[] } }
  | { ExecuteShell: { command: string } }
  | { ExecutePowerShell: { script: string; no_profile: boolean } }
  | { ExecuteCmd: { command: string } };

export function getActionType(
  kind: ActionKind
): 'ExecuteProgram' | 'ExecuteShell' | 'ExecutePowerShell' | 'ExecuteCmd' {
  if ('ExecuteProgram' in kind) return 'ExecuteProgram';
  if ('ExecuteShell' in kind) return 'ExecuteShell';
  if ('ExecutePowerShell' in kind) return 'ExecutePowerShell';
  if ('ExecuteCmd' in kind) return 'ExecuteCmd';
  throw new Error(`Unknown action kind: ${JSON.stringify(kind)}`);
}

export function isWindowsPlatform(): boolean {
  if (typeof navigator !== 'undefined') {
    const userAgent = navigator.userAgent || '';
    const platform = (navigator as any).userAgentData?.platform || navigator.platform || '';
    return /win/i.test(userAgent) || /win/i.test(platform);
  }
  return false;
}

export interface Action {
  id: ActionId;
  task_id: TaskId;
  sequence: number;
  enabled: boolean;
  kind: ActionKind;
}

export function getEmptyTask(): Task {
  return {
    id: typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'task-' + Math.random().toString(36).substring(2, 9),
    name: '',
    description: '',
    enabled: true,
    triggers: [],
    actions: [],
    execution_policy: {
      concurrency_policy: 'SkipIfRunning',
      missed_run_policy: 'RunOnce',
      retry_policy: {
        max_retries: 0,
        delay_secs: 0,
      },
      timeout_secs: 3600,
      notification: 'None',
      log_retention: { mode: 'SystemDefault' },
    },
    working_directory: null,
    environment: {},
    version: 1,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  };
}

export function parseDate(iso?: string | null): number | null {
  if (!iso) return null;
  const time = Date.parse(iso);
  return isNaN(time) ? null : time;
}

export interface Task {
  id: TaskId;
  name: string;
  description: string | null;
  enabled: boolean;
  triggers: Trigger[];
  actions: Action[];
  execution_policy: ExecutionPolicy;
  working_directory: string | null;
  environment: Record<string, string>;
  version: number;
  created_at: string;
  updated_at: string;
}
