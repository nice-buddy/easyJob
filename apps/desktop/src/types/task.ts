export type TaskId = string;
export type TriggerId = string;
export type ActionId = string;

export type ConcurrencyPolicy = 'AllowParallel' | 'SkipIfRunning' | 'QueueOne';
export type MissedRunPolicy = 'RunOnce' | 'Skip';

export interface RetryPolicy {
  max_retries: number;
  delay_secs: number;
}

export interface ExecutionPolicy {
  concurrency_policy: ConcurrencyPolicy;
  missed_run_policy: MissedRunPolicy;
  timeout_secs: number | null;
  max_retries: number;
  retry_policy?: RetryPolicy;
}

export type TriggerKind =
  | { type: 'Once'; datetime: string; fire_at?: string }
  | { type: 'Interval'; seconds: number; interval_secs?: number; start_at?: string | null }
  | { type: 'Daily'; time: string; timezone: string }
  | { type: 'Weekly'; days_of_week: number[]; time: string; timezone: string }
  | { type: 'AgentStarted' };

export interface Trigger {
  id: TriggerId;
  task_id: TaskId;
  enabled: boolean;
  kind: TriggerKind;
  created_at: string;
  updated_at: string;
}

export type ActionKind =
  | { type: 'ExecuteShell'; command: string }
  | { type: 'ExecuteProgram'; program: string; args: string[] };

export interface Action {
  id: ActionId;
  task_id: TaskId;
  sequence: number;
  enabled: boolean;
  kind: ActionKind;
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
