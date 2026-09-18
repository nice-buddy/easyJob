<script setup lang="ts">
import { ref, watch } from 'vue';
import {
  NCollapse,
  NCollapseItem,
  NTag,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NSwitch,
  NButton,
} from 'naive-ui';
import TriggerEditor from './TriggerEditor.vue';
import ActionEditor from './ActionEditor.vue';
import type {
  Task,
  ConcurrencyPolicy,
  MissedRunPolicy,
  TaskNotificationPolicy,
  LogRetentionPolicy,
} from '../../types/task';
import { getTriggerType, getActionType } from '../../types/task';

const props = defineProps<{
  task: Task | null;
  editable: boolean;
  diffFields?: Set<string>;
  columnTitle: string;
}>();

const isDiff = (field: string) => props.diffFields?.has(field) ?? false;

const concurrencyOptions: { label: string; value: ConcurrencyPolicy }[] = [
  { label: '单例跳过 (SkipIfRunning)', value: 'SkipIfRunning' },
  { label: '允许并行 (AllowParallel)', value: 'AllowParallel' },
  { label: '至多排队一个 (QueueOne)', value: 'QueueOne' },
];

const missedRunOptions: { label: string; value: MissedRunPolicy }[] = [
  { label: '补跑一次 (RunOnce)', value: 'RunOnce' },
  { label: '直接跳过 (Skip)', value: 'Skip' },
];

const notificationOptions: { label: string; value: TaskNotificationPolicy }[] = [
  { label: '不通知 (默认)', value: 'None' },
  { label: '仅成功时通知 (OnlySuccess)', value: 'OnlySuccess' },
  { label: '仅失败时通知 (OnlyFailure)', value: 'OnlyFailure' },
  { label: '全部通知 (成功与失败均通知)', value: 'All' },
];

function formatNotification(policy?: string) {
  switch (policy) {
    case 'OnlySuccess': return '仅成功时通知';
    case 'OnlyFailure': return '仅失败时通知';
    case 'All': return '全部通知 (成功与失败均通知)';
    default: return '不通知';
  }
}

const retentionModeOptions = [
  { label: '跟随系统设置 (默认)', value: 'SystemDefault' },
  { label: '自定义保留天数', value: 'KeepDays' },
  { label: '永久保留 (从不清理)', value: 'Permanent' },
];

function formatLogRetention(policy?: LogRetentionPolicy) {
  if (!policy || policy.mode === 'SystemDefault') {
    return '跟随系统设置';
  }
  if (policy.mode === 'Permanent') {
    return '永久保留 (从不清理)';
  }
  return `保留 ${policy.days} 天`;
}

function onRetentionModeChange(mode: 'SystemDefault' | 'KeepDays' | 'Permanent') {
  if (!props.task) return;
  if (mode === 'KeepDays') {
    const prevDays =
      props.task.execution_policy.log_retention?.mode === 'KeepDays'
        ? props.task.execution_policy.log_retention.days
        : 7;
    props.task.execution_policy.log_retention = { mode: 'KeepDays', days: prevDays || 7 };
  } else if (mode === 'Permanent') {
    props.task.execution_policy.log_retention = { mode: 'Permanent' };
  } else {
    props.task.execution_policy.log_retention = { mode: 'SystemDefault' };
  }
}

const envList = ref<{ key: string; value: string }[]>([]);

watch(
  () => props.task?.environment,
  (env) => {
    envList.value = Object.entries(env || {}).map(([key, value]) => ({ key, value }));
  },
  { immediate: true, deep: true }
);

function syncEnv() {
  if (!props.task) return;
  const obj: Record<string, string> = {};
  envList.value.forEach(({ key, value }) => {
    if (key.trim()) obj[key.trim()] = value;
  });
  props.task.environment = obj;
}

function addEnv() {
  envList.value.push({ key: '', value: '' });
  syncEnv();
}

function removeEnv(idx: number) {
  envList.value.splice(idx, 1);
  syncEnv();
}
</script>

<template>
  <div v-if="task" class="h-full flex flex-col">
    <NCollapse default-expanded-names="['basic', 'policy', 'triggers', 'actions', 'env']">
      <!-- 1. 基本配置 -->
      <NCollapseItem title="基本配置" name="basic">
        <template #header-extra>
          <NTag
            v-if="!editable && (isDiff('basic.description') || isDiff('basic.enabled') || isDiff('basic.working_directory'))"
            size="tiny"
            type="warning"
          >
            有差异
          </NTag>
        </template>

        <div v-if="!editable" class="space-y-2 text-xs">
          <div :class="['p-2 rounded', isDiff('basic.description') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">描述：</span>
            <span class="font-medium text-slate-800 dark:text-zinc-200">{{ task.description || '无' }}</span>
          </div>
          <div :class="['p-2 rounded flex items-center justify-between', isDiff('basic.enabled') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">启用状态：</span>
            <NTag size="tiny" :type="task.enabled ? 'success' : 'default'">{{ task.enabled ? '启用' : '停用' }}</NTag>
          </div>
          <div :class="['p-2 rounded', isDiff('basic.working_directory') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">工作目录：</span>
            <span class="font-medium text-slate-800 dark:text-zinc-200">{{ task.working_directory || '默认' }}</span>
          </div>
        </div>

        <div v-else class="space-y-3 pt-1">
          <NForm label-placement="top" size="small">
            <NFormItem label="任务描述">
              <NInput v-model:value="task.description" type="textarea" :autosize="{ minRows: 2, maxRows: 3 }" />
            </NFormItem>
            <div class="flex items-center justify-between p-2 bg-slate-50 dark:bg-zinc-800/40 rounded mb-3">
              <span class="text-xs">任务启用状态</span>
              <NSwitch v-model:value="task.enabled" size="small" />
            </div>
            <NFormItem label="工作目录">
              <NInput v-model:value="task.working_directory" placeholder="默认运行路径" />
            </NFormItem>
          </NForm>
        </div>
      </NCollapseItem>

      <!-- 2. 执行策略 -->
      <NCollapseItem title="执行策略" name="policy">
        <template #header-extra>
          <NTag
            v-if="!editable && (isDiff('policy.concurrency_policy') || isDiff('policy.missed_run_policy') || isDiff('policy.timeout_secs') || isDiff('policy.retry_max_retries') || isDiff('policy.notification') || isDiff('policy.log_retention'))"
            size="tiny"
            type="warning"
          >
            有差异
          </NTag>
        </template>

        <div v-if="!editable" class="space-y-2 text-xs">
          <div :class="['p-2 rounded', isDiff('policy.concurrency_policy') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">并发策略：</span>
            <span class="font-medium">{{ task.execution_policy.concurrency_policy }}</span>
          </div>
          <div :class="['p-2 rounded', isDiff('policy.missed_run_policy') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">错失策略：</span>
            <span class="font-medium">{{ task.execution_policy.missed_run_policy }}</span>
          </div>
          <div :class="['p-2 rounded', isDiff('policy.timeout_secs') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">超时限制：</span>
            <span class="font-medium">{{ task.execution_policy.timeout_secs ? `${task.execution_policy.timeout_secs} 秒` : '无限制' }}</span>
          </div>
          <div :class="['p-2 rounded', isDiff('policy.retry_max_retries') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">重试策略：</span>
            <span class="font-medium">重试 {{ task.execution_policy.retry_policy.max_retries }} 次 (延迟 {{ task.execution_policy.retry_policy.delay_secs }} 秒)</span>
          </div>
          <div :class="['p-2 rounded', isDiff('policy.notification') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">结果通知：</span>
            <span class="font-medium">{{ formatNotification(task.execution_policy.notification) }}</span>
          </div>
          <div :class="['p-2 rounded', isDiff('policy.log_retention') ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']">
            <span class="text-slate-400">日志保留：</span>
            <span class="font-medium">{{ formatLogRetention(task.execution_policy.log_retention) }}</span>
          </div>
        </div>

        <div v-else class="space-y-3 pt-1">
          <NForm label-placement="top" size="small">
            <NFormItem label="并发策略">
              <NSelect v-model:value="task.execution_policy.concurrency_policy" :options="concurrencyOptions" />
            </NFormItem>
            <NFormItem label="错失触发策略">
              <NSelect v-model:value="task.execution_policy.missed_run_policy" :options="missedRunOptions" />
            </NFormItem>
            <div class="grid grid-cols-2 gap-2">
              <NFormItem label="超时 (秒)">
                <NInputNumber v-model:value="task.execution_policy.timeout_secs" :min="1" placeholder="不限" class="w-full" />
              </NFormItem>
              <NFormItem label="重试次数">
                <NInputNumber v-model:value="task.execution_policy.retry_policy.max_retries" :min="0" class="w-full" />
              </NFormItem>
            </div>
            <NFormItem label="执行结果通知">
              <NSelect v-model:value="task.execution_policy.notification" :options="notificationOptions" />
            </NFormItem>
            <NFormItem label="日志保留策略">
              <div class="flex items-center gap-2 w-full">
                <NSelect
                  :value="task.execution_policy.log_retention?.mode || 'SystemDefault'"
                  :options="retentionModeOptions"
                  @update:value="onRetentionModeChange"
                />
                <NInputNumber
                  v-if="task.execution_policy.log_retention?.mode === 'KeepDays'"
                  v-model:value="(task.execution_policy.log_retention as any).days"
                  :min="1"
                  style="width: 120px; flex-shrink: 0;"
                >
                  <template #suffix>天</template>
                </NInputNumber>
              </div>
            </NFormItem>
          </NForm>
        </div>
      </NCollapseItem>

      <!-- 3. 触发规则 -->
      <NCollapseItem title="触发规则" name="triggers">
        <template #header-extra>
          <NTag v-if="!editable && isDiff('triggers')" size="tiny" type="warning">有差异</NTag>
          <span class="text-xs text-slate-400 ml-1.5">({{ task.triggers.length }} 个)</span>
        </template>

        <div v-if="!editable" class="space-y-1.5 text-xs">
          <div
            v-for="(tr, idx) in task.triggers"
            :key="idx"
            :class="['p-2 rounded border', isDiff('triggers') ? 'border-amber-300 dark:border-amber-800/60 bg-amber-50/40 dark:bg-amber-950/20' : 'border-slate-200 dark:border-zinc-800 bg-slate-50 dark:bg-zinc-800/40']"
          >
            <div class="flex items-center justify-between mb-1">
              <NTag size="tiny" type="info">{{ getTriggerType(tr.kind) }}</NTag>
              <span :class="tr.enabled ? 'text-emerald-500' : 'text-slate-400'">{{ tr.enabled ? '启用' : '停用' }}</span>
            </div>
            <div class="text-[11px] text-slate-600 dark:text-zinc-300 font-mono truncate">
              {{ JSON.stringify(tr.kind) }}
            </div>
          </div>
          <div v-if="task.triggers.length === 0" class="text-slate-400 text-xs py-2 text-center">无触发器</div>
        </div>

        <div v-else>
          <TriggerEditor v-model:triggers="task.triggers" :task-id="task.id" />
        </div>
      </NCollapseItem>

      <!-- 4. 执行动作 -->
      <NCollapseItem title="执行动作" name="actions">
        <template #header-extra>
          <NTag v-if="!editable && isDiff('actions')" size="tiny" type="warning">有差异</NTag>
          <span class="text-xs text-slate-400 ml-1.5">({{ task.actions.length }} 个)</span>
        </template>

        <div v-if="!editable" class="space-y-1.5 text-xs">
          <div
            v-for="(act, idx) in task.actions"
            :key="idx"
            :class="['p-2 rounded border', isDiff('actions') ? 'border-amber-300 dark:border-amber-800/60 bg-amber-50/40 dark:bg-amber-950/20' : 'border-slate-200 dark:border-zinc-800 bg-slate-50 dark:bg-zinc-800/40']"
          >
            <div class="flex items-center justify-between mb-1">
              <NTag size="tiny" type="default">步骤 {{ idx + 1 }}: {{ getActionType(act.kind) }}</NTag>
              <span :class="act.enabled ? 'text-emerald-500' : 'text-slate-400'">{{ act.enabled ? '启用' : '停用' }}</span>
            </div>
            <div class="text-[11px] text-slate-600 dark:text-zinc-300 font-mono truncate">
              {{ JSON.stringify(act.kind) }}
            </div>
          </div>
          <div v-if="task.actions.length === 0" class="text-slate-400 text-xs py-2 text-center">无执行动作</div>
        </div>

        <div v-else>
          <ActionEditor v-model:actions="task.actions" :task-id="task.id" />
        </div>
      </NCollapseItem>

      <!-- 5. 环境变量 -->
      <NCollapseItem title="环境变量" name="env">
        <template #header-extra>
          <span class="text-xs text-slate-400 ml-1.5">({{ Object.keys(task.environment || {}).length }} 个)</span>
        </template>

        <div v-if="!editable" class="space-y-1 text-xs">
          <div
            v-for="(val, key) in task.environment || {}"
            :key="key"
            :class="['p-1.5 rounded flex justify-between font-mono text-[11px]', isDiff(`env.${key}`) ? 'bg-amber-50 dark:bg-amber-950/40 border border-amber-300 dark:border-amber-700' : 'bg-slate-50 dark:bg-zinc-800/40']"
          >
            <span class="text-slate-500">{{ key }}</span>
            <span class="text-slate-800 dark:text-zinc-200">{{ val }}</span>
          </div>
          <div v-if="Object.keys(task.environment || {}).length === 0" class="text-slate-400 text-xs py-2 text-center">无环境变量</div>
        </div>

        <div v-else class="space-y-2">
          <div
            v-for="(item, idx) in envList"
            :key="idx"
            class="flex items-center gap-2"
          >
            <NInput v-model:value="item.key" size="small" placeholder="KEY" @update:value="syncEnv" />
            <NInput v-model:value="item.value" size="small" placeholder="VALUE" @update:value="syncEnv" />
            <NButton size="tiny" secondary type="error" @click="removeEnv(idx)">×</NButton>
          </div>
          <NButton size="tiny" secondary @click="addEnv">+ 添加环境变量</NButton>
        </div>
      </NCollapseItem>
    </NCollapse>
  </div>
</template>
