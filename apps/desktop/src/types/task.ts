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

// serde: 无字段变体序列化为字符串，带字段变体序列化为对象
export type FuzzyPeriod =
  | 'Daily'
  | 'Weekdays'
  | 'Weekends'
  | { Weekly: { days_of_week: Weekday[] } };

export type NetworkEventKind = 'Connect' | 'Disconnect' | 'Online';

export type TriggerType =
  | 'Once' | 'Interval' | 'Daily' | 'Weekly' | 'AgentStarted'
  | 'Cron' | 'Fuzzy' | 'Network';

export type TriggerKind =
  | { Once: { fire_at: string } }
  | { Interval: { seconds: number; interval_secs?: number; start_at?: string | null } | { interval_secs: number; seconds?: number; start_at?: string | null } }
  | { Daily: { time: string; timezone: string } }
  | { Weekly: { days_of_week: Weekday[]; time: string; timezone: string } }
  | 'AgentStarted'
  | { Cron: { expression: string; timezone: string } }
  | { Fuzzy: { period: FuzzyPeriod; window_start: string; window_end: string; timezone: string } }
  | { Network: { events: NetworkEventKind[]; network_name: string | null } };

export function getTriggerType(kind: TriggerKind): TriggerType {
  if (typeof kind === 'string') {
    return kind;
  }
  if ('Once' in kind) return 'Once';
  if ('Interval' in kind) return 'Interval';
  if ('Daily' in kind) return 'Daily';
  if ('Weekly' in kind) return 'Weekly';
  if ('Cron' in kind) return 'Cron';
  if ('Fuzzy' in kind) return 'Fuzzy';
  if ('Network' in kind) return 'Network';
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

const CRON_FIELD_RANGES: [number, number][] = [
  [0, 59], // minute
  [0, 23], // hour
  [1, 31], // day of month
  [1, 12], // month
  [0, 7],  // day of week (7 = Sunday)
];

// croner 只识别月与周几字段的三字母别名；其余字段出现字母一律非法
const CRON_MONTH_NAMES: Record<string, number> = {
  jan: 1, feb: 2, mar: 3, apr: 4, may: 5, jun: 6,
  jul: 7, aug: 8, sep: 9, oct: 10, nov: 11, dec: 12,
};
const CRON_WEEKDAY_NAMES: Record<string, number> = {
  sun: 0, mon: 1, tue: 2, wed: 3, thu: 4, fri: 5, sat: 6,
};

const CRON_TOKEN_RE =
  /^(\*|\?|\d+|[A-Za-z]{3})(?:-(\d+|[A-Za-z]{3}))?(?:\/(\d+))?$/;

export function validateCronExpression(expr: string): boolean {
  const fields = expr.trim().split(/\s+/);
  if (fields.length !== 5) return false;
  return fields.every((field, i) => {
    if (field === '*' || field === '?') return true;
    const [lo, hi] = CRON_FIELD_RANGES[i];
    return field.split(',').every((token) => {
      const step = token.includes('/');
      if (step) {
        const [base, stepStr] = token.split('/');
        if (!/^\d+$/.test(stepStr) || parseInt(stepStr, 10) < 1) return false;
        if (base === '*' || base === '?') return true;
        return checkCronToken(base, lo, hi, i);
      }
      return checkCronToken(token, lo, hi, i);
    });
  });
}

function checkCronToken(token: string, lo: number, hi: number, fieldIdx: number): boolean {
  if (!CRON_TOKEN_RE.test(token)) return false;
  const [start, end] = token.split('-');
  const startVal = cronFieldValue(start, fieldIdx, false);
  if (startVal === null || startVal < lo || startVal > hi) return false;
  if (end === undefined) return true;
  const endVal = cronFieldValue(end, fieldIdx, true);
  if (endVal === null || endVal < lo || endVal > hi) return false;
  // croner 对反向区间报错（如 5-1、FRI-MON），这里必须一致
  return startVal <= endVal;
}

// 名称别名或数字 → 数值；未知别名返回 null
function cronFieldValue(part: string, fieldIdx: number, isRangeEnd: boolean): number | null {
  if (/^\d+$/.test(part)) return parseInt(part, 10);
  const name = part.toLowerCase();
  const table = fieldIdx === 3 ? CRON_MONTH_NAMES : fieldIdx === 4 ? CRON_WEEKDAY_NAMES : undefined;
  const value = table?.[name];
  if (value === undefined) return null;
  // croner 会把范围末端的 sun 归一化为 7（替换表 "-sun" → "-7"），使 FRI-SUN 合法
  return fieldIdx === 4 && isRangeEnd && name === 'sun' ? 7 : value;
}

export function describeTrigger(kind: TriggerKind): string {
  if (typeof kind === 'string') return '启动即运行';
  if ('Once' in kind) return `单次：${kind.Once.fire_at.replace('T', ' ').slice(0, 19)} UTC`;
  if ('Interval' in kind) {
    const s = kind.Interval.interval_secs ?? kind.Interval.seconds ?? 60;
    return `间隔：每 ${s} 秒`;
  }
  if ('Daily' in kind) return `每天 ${kind.Daily.time} (${kind.Daily.timezone})`;
  if ('Weekly' in kind) return `每周 ${kind.Weekly.days_of_week.join(',')} ${kind.Weekly.time}`;
  if ('Cron' in kind) return `Cron: ${kind.Cron.expression}`;
  if ('Fuzzy' in kind) {
    const f = kind.Fuzzy;
    const periodLabel =
      typeof f.period === 'string'
        ? { Daily: '每天', Weekdays: '工作日', Weekends: '周末' }[f.period]
        : `每周 ${f.period.Weekly.days_of_week.join(',')}`;
    return `模糊时间：${periodLabel} ${f.window_start}-${f.window_end} 随机`;
  }
  if ('Network' in kind) {
    const evLabels = kind.Network.events
      .map((e) => ({ Connect: '连接时', Disconnect: '断开时', Online: '可上网时' }[e]))
      .join('/');
    const name = kind.Network.network_name ? ` (${kind.Network.network_name})` : '（任意网络）';
    return `网络变动：${evLabels}${name}`;
  }
  return JSON.stringify(kind);
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
