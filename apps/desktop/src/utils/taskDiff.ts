import type { Task, Trigger, Action } from '../types/task';

export interface TaskDiffResult {
  hasDiff: boolean;
  basicDiff: boolean;
  policyDiff: boolean;
  triggersDiff: boolean;
  actionsDiff: boolean;
  envDiff: boolean;
  diffFields: Set<string>;
}

function normalizeTrigger(tr: Trigger) {
  return {
    enabled: tr.enabled,
    kind: tr.kind,
  };
}

function normalizeAction(act: Action) {
  return {
    enabled: act.enabled,
    sequence: act.sequence,
    kind: act.kind,
  };
}

export function compareTasks(existingTask: Task, importedTask: Task): TaskDiffResult {
  const diffFields = new Set<string>();

  // 1. Basic configuration
  if ((existingTask.description || '') !== (importedTask.description || '')) {
    diffFields.add('basic.description');
  }
  if (existingTask.enabled !== importedTask.enabled) {
    diffFields.add('basic.enabled');
  }
  if ((existingTask.working_directory || '') !== (importedTask.working_directory || '')) {
    diffFields.add('basic.working_directory');
  }
  const basicDiff =
    diffFields.has('basic.description') ||
    diffFields.has('basic.enabled') ||
    diffFields.has('basic.working_directory');

  // 2. Execution policy
  const ep1 = existingTask.execution_policy;
  const ep2 = importedTask.execution_policy;

  if (ep1.concurrency_policy !== ep2.concurrency_policy) {
    diffFields.add('policy.concurrency_policy');
  }
  if (ep1.missed_run_policy !== ep2.missed_run_policy) {
    diffFields.add('policy.missed_run_policy');
  }
  if (ep1.timeout_secs !== ep2.timeout_secs) {
    diffFields.add('policy.timeout_secs');
  }
  if (ep1.retry_policy.max_retries !== ep2.retry_policy.max_retries) {
    diffFields.add('policy.retry_max_retries');
  }
  if (ep1.retry_policy.delay_secs !== ep2.retry_policy.delay_secs) {
    diffFields.add('policy.retry_delay_secs');
  }

  const policyDiff =
    diffFields.has('policy.concurrency_policy') ||
    diffFields.has('policy.missed_run_policy') ||
    diffFields.has('policy.timeout_secs') ||
    diffFields.has('policy.retry_max_retries') ||
    diffFields.has('policy.retry_delay_secs');

  // 3. Triggers
  const normTriggers1 = existingTask.triggers.map(normalizeTrigger);
  const normTriggers2 = importedTask.triggers.map(normalizeTrigger);
  const triggersDiff = JSON.stringify(normTriggers1) !== JSON.stringify(normTriggers2);
  if (triggersDiff) {
    diffFields.add('triggers');
  }

  // 4. Actions
  const normActions1 = existingTask.actions.map(normalizeAction);
  const normActions2 = importedTask.actions.map(normalizeAction);
  const actionsDiff = JSON.stringify(normActions1) !== JSON.stringify(normActions2);
  if (actionsDiff) {
    diffFields.add('actions');
  }

  // 5. Environment variables
  const env1 = existingTask.environment || {};
  const env2 = importedTask.environment || {};
  const allEnvKeys = new Set([...Object.keys(env1), ...Object.keys(env2)]);
  let envDiff = false;

  for (const key of allEnvKeys) {
    if (env1[key] !== env2[key]) {
      diffFields.add(`env.${key}`);
      envDiff = true;
    }
  }

  const hasDiff = basicDiff || policyDiff || triggersDiff || actionsDiff || envDiff;

  return {
    hasDiff,
    basicDiff,
    policyDiff,
    triggersDiff,
    actionsDiff,
    envDiff,
    diffFields,
  };
}
