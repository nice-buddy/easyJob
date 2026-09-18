<script lang="ts">
export { parseDate } from '../../types/task';
</script>

<script setup lang="ts">
import { ref } from 'vue';
import { NSelect, NInputNumber, NInput, NDatePicker, NCheckboxGroup, NCheckbox, NButton, NSwitch } from 'naive-ui';
import { Plus, Trash2 } from 'lucide-vue-next';
import type { Trigger, Weekday, FuzzyPeriod } from '../../types/task';
import { getTriggerType, parseDate, validateCronExpression } from '../../types/task';

const props = defineProps<{
  triggers: Trigger[];
  taskId: string;
}>();

const emit = defineEmits<{
  (e: 'update:triggers', triggers: Trigger[]): void;
}>();

const triggerTypeOptions = [
  { label: '间隔触发 (Interval)', value: 'Interval' },
  { label: '每日定时 (Daily)', value: 'Daily' },
  { label: '每周定时 (Weekly)', value: 'Weekly' },
  { label: '单次执行 (Once)', value: 'Once' },
  { label: '启动即运行 (AgentStarted)', value: 'AgentStarted' },
  { label: 'Cron 表达式 (Cron)', value: 'Cron' },
  { label: '模糊时间 (Fuzzy)', value: 'Fuzzy' },
  { label: '网络变动 (Network)', value: 'Network' },
];

const dayOptions: { label: string; value: Weekday }[] = [
  { label: '周一', value: 'Mon' },
  { label: '周二', value: 'Tue' },
  { label: '周三', value: 'Wed' },
  { label: '周四', value: 'Thu' },
  { label: '周五', value: 'Fri' },
  { label: '周六', value: 'Sat' },
  { label: '周日', value: 'Sun' },
];

const intervalUnitOptions = [
  { label: '秒', value: 1 },
  { label: '分钟', value: 60 },
  { label: '小时', value: 3600 },
];

const intervalUnits = ref<Record<string, number>>({});

function getIntervalSecs(trigger: Trigger): number {
  if (typeof trigger.kind === 'object' && 'Interval' in trigger.kind) {
    return trigger.kind.Interval.interval_secs ?? trigger.kind.Interval.seconds ?? 60;
  }
  return 60;
}

function getIntervalUnit(trigger: Trigger): number {
  if (intervalUnits.value[trigger.id]) {
    return intervalUnits.value[trigger.id];
  }
  const totalSecs = getIntervalSecs(trigger);
  if (totalSecs >= 3600 && totalSecs % 3600 === 0) {
    return 3600;
  }
  if (totalSecs >= 60 && totalSecs % 60 === 0) {
    return 60;
  }
  return 1;
}

function getDisplayIntervalValue(trigger: Trigger): number {
  const totalSecs = getIntervalSecs(trigger);
  const unit = getIntervalUnit(trigger);
  return Math.max(1, Math.round(totalSecs / unit));
}

function setIntervalSecs(trigger: Trigger, secs: number) {
  if (typeof trigger.kind === 'object' && 'Interval' in trigger.kind) {
    trigger.kind.Interval.interval_secs = secs;
    if ('seconds' in trigger.kind.Interval) {
      trigger.kind.Interval.seconds = secs;
    }
  }
}

function onIntervalValueChange(trigger: Trigger, val: number | null) {
  const unit = getIntervalUnit(trigger);
  const inputVal = val && val > 0 ? val : 1;
  const totalSecs = inputVal * unit;
  setIntervalSecs(trigger, totalSecs);
}

function onIntervalUnitChange(trigger: Trigger, newUnit: number) {
  const currentDisplayVal = getDisplayIntervalValue(trigger);
  intervalUnits.value[trigger.id] = newUnit;
  const totalSecs = currentDisplayVal * newUnit;
  setIntervalSecs(trigger, totalSecs);
}

function addTrigger() {
  const newTrigger: Trigger = {
    id: crypto.randomUUID(),
    task_id: props.taskId,
    enabled: true,
    kind: { Interval: { interval_secs: 60 } },
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  };
  emit('update:triggers', [...props.triggers, newTrigger]);
}

function removeTrigger(index: number) {
  const next = [...props.triggers];
  next.splice(index, 1);
  emit('update:triggers', next);
}

function normalizeTime(val: string): string {
  if (!val) return '00:00:00';
  return val.length === 5 ? `${val}:00` : val;
}

function changeKindType(trigger: Trigger, type: string) {
  const defaultTz =
    typeof Intl !== 'undefined' && Intl.DateTimeFormat
      ? Intl.DateTimeFormat().resolvedOptions().timeZone || 'Asia/Shanghai'
      : 'Asia/Shanghai';

  if (type === 'Interval') {
    trigger.kind = { Interval: { interval_secs: 60 } };
  } else if (type === 'Daily') {
    trigger.kind = {
      Daily: {
        time: '09:00:00',
        timezone: defaultTz,
      },
    };
  } else if (type === 'Weekly') {
    trigger.kind = {
      Weekly: {
        days_of_week: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'],
        time: '09:00:00',
        timezone: defaultTz,
      },
    };
  } else if (type === 'Once') {
    const future = new Date(Date.now() + 3600000);
    trigger.kind = {
      Once: {
        fire_at: future.toISOString(),
      },
    };
  } else if (type === 'AgentStarted') {
    trigger.kind = 'AgentStarted';
  } else if (type === 'Cron') {
    trigger.kind = {
      Cron: {
        expression: '*/5 * * * *',
        timezone: defaultTz,
      },
    };
  } else if (type === 'Fuzzy') {
    trigger.kind = {
      Fuzzy: {
        period: 'Daily',
        window_start: '09:00:00',
        window_end: '10:00:00',
        timezone: defaultTz,
      },
    };
  } else if (type === 'Network') {
    trigger.kind = {
      Network: {
        events: ['Connect'],
        network_name: null,
      },
    };
  }
}

const fuzzyPeriodOptions = [
  { label: '每天', value: 'Daily' },
  { label: '工作日 (周一至周五)', value: 'Weekdays' },
  { label: '周末', value: 'Weekends' },
  { label: '按周几', value: 'Weekly' },
];

const networkEventOptions = [
  { label: '连接网络时', value: 'Connect' },
  { label: '断开网络时', value: 'Disconnect' },
  { label: '检测到可上网时', value: 'Online' },
];

function getFuzzyPeriod(tr: Trigger): string {
  if (typeof tr.kind === 'object' && 'Fuzzy' in tr.kind) {
    return typeof tr.kind.Fuzzy.period === 'string' ? tr.kind.Fuzzy.period : 'Weekly';
  }
  return 'Daily';
}

function setFuzzyPeriod(tr: Trigger, period: string) {
  if (typeof tr.kind === 'object' && 'Fuzzy' in tr.kind) {
    tr.kind.Fuzzy.period =
      period === 'Weekly' ? { Weekly: { days_of_week: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'] } } : (period as FuzzyPeriod);
  }
}

function getFuzzyWeeklyDays(tr: Trigger): Weekday[] {
  if (typeof tr.kind === 'object' && 'Fuzzy' in tr.kind) {
    const p = tr.kind.Fuzzy.period;
    if (typeof p !== 'string') return p.Weekly.days_of_week;
  }
  return [];
}

function setFuzzyWeeklyDays(tr: Trigger, days: Weekday[]) {
  if (typeof tr.kind === 'object' && 'Fuzzy' in tr.kind) {
    const p = tr.kind.Fuzzy.period;
    if (typeof p !== 'string') p.Weekly.days_of_week = days;
  }
}

function cronInvalid(tr: Trigger): boolean {
  if (typeof tr.kind === 'object' && 'Cron' in tr.kind) {
    return !validateCronExpression(tr.kind.Cron.expression);
  }
  return false;
}

function fuzzyWindowInvalid(tr: Trigger): boolean {
  if (typeof tr.kind === 'object' && 'Fuzzy' in tr.kind) {
    return tr.kind.Fuzzy.window_end <= tr.kind.Fuzzy.window_start;
  }
  return false;
}

function networkEventsInvalid(tr: Trigger): boolean {
  if (typeof tr.kind === 'object' && 'Network' in tr.kind) {
    return tr.kind.Network.events.length === 0;
  }
  return false;
}

function formatIntervalPreview(sec: number): string {
  if (sec >= 86400 && sec % 86400 === 0) {
    return `${sec / 86400} 天`;
  }
  if (sec >= 3600 && sec % 3600 === 0) {
    return `${sec / 3600} 小时`;
  }
  if (sec >= 60 && sec % 60 === 0) {
    return `${sec / 60} 分钟`;
  }
  return `${sec} 秒`;
}
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <div>
        <h3 class="text-sm font-semibold text-slate-800 dark:text-zinc-200">触发器列表 (Triggers)</h3>
        <p class="text-xs text-slate-400 dark:text-zinc-500">配置任务的启动条件与调度计划</p>
      </div>
      <NButton size="tiny" secondary @click="addTrigger">
        <template #icon><Plus class="w-3 h-3" /></template>
        添加触发器
      </NButton>
    </div>

    <div v-if="triggers.length === 0" class="text-xs text-slate-400 dark:text-zinc-500 py-4 text-center border border-dashed border-slate-200 dark:border-zinc-800 rounded-lg">
      暂未配置触发器，任务将仅可通过手动点击“立即执行”触发。
    </div>

    <div
      v-for="(tr, idx) in triggers"
      :key="tr.id"
      class="p-3.5 border border-slate-200 dark:border-zinc-800 rounded-lg space-y-3 bg-slate-50/70 dark:bg-zinc-800/30 transition hover:border-slate-300 dark:hover:border-zinc-700"
    >
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-2 flex-1">
          <NSelect
            :value="getTriggerType(tr.kind)"
            :options="triggerTypeOptions"
            size="small"
            class="w-56"
            @update:value="changeKindType(tr, $event)"
          />
          <div class="flex items-center gap-1.5 text-xs text-slate-500">
            <span>启用</span>
            <NSwitch v-model:value="tr.enabled" size="small" />
          </div>
        </div>

        <NButton size="tiny" text type="error" @click="removeTrigger(idx)">
          <Trash2 class="w-4 h-4" />
        </NButton>
      </div>

      <!-- Interval Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Interval' in tr.kind" class="flex items-center gap-2 text-xs flex-wrap">
        <span class="text-slate-500">每隔</span>
        <NInputNumber
          :value="getDisplayIntervalValue(tr)"
          size="small"
          :min="1"
          class="w-24"
          @update:value="onIntervalValueChange(tr, $event)"
        />
        <NSelect
          :value="getIntervalUnit(tr)"
          :options="intervalUnitOptions"
          size="small"
          class="w-24"
          @update:value="onIntervalUnitChange(tr, $event)"
        />
        <span class="text-slate-500">执行一次</span>
        <span class="text-slate-400 dark:text-zinc-500">
          ({{ formatIntervalPreview(tr.kind.Interval.interval_secs ?? tr.kind.Interval.seconds ?? 60) }})
        </span>
      </div>

      <!-- Daily Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Daily' in tr.kind" class="grid grid-cols-2 gap-3 text-xs">
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">执行时间 (HH:mm:ss)</label>
          <input
            type="time"
            step="1"
            :value="tr.kind.Daily.time"
            @change="tr.kind.Daily.time = normalizeTime(($event.target as HTMLInputElement).value)"
            class="w-full px-2.5 py-1.5 rounded border border-slate-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-xs font-mono"
          />
        </div>
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">时区 (Timezone)</label>
          <NInput
            v-model:value="tr.kind.Daily.timezone"
            placeholder="例如 Asia/Shanghai, UTC"
            size="small"
          />
        </div>
      </div>

      <!-- Weekly Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Weekly' in tr.kind" class="space-y-2.5 text-xs">
        <div class="grid grid-cols-2 gap-3">
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">执行时间 (HH:mm:ss)</label>
            <input
              type="time"
              step="1"
              :value="tr.kind.Weekly.time"
              @change="tr.kind.Weekly.time = normalizeTime(($event.target as HTMLInputElement).value)"
              class="w-full px-2.5 py-1.5 rounded border border-slate-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-xs font-mono"
            />
          </div>
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">时区 (Timezone)</label>
            <NInput
              v-model:value="tr.kind.Weekly.timezone"
              placeholder="例如 Asia/Shanghai, UTC"
              size="small"
            />
          </div>
        </div>

        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">每周执行日</label>
          <NCheckboxGroup v-model:value="tr.kind.Weekly.days_of_week">
            <div class="flex flex-wrap gap-2">
              <NCheckbox v-for="d in dayOptions" :key="d.value" :value="d.value" :label="d.label" size="small" />
            </div>
          </NCheckboxGroup>
        </div>
      </div>

      <!-- Once Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Once' in tr.kind" class="text-xs space-y-1">
        <label class="block text-slate-500 dark:text-zinc-400">单次触发时间</label>
        <NDatePicker
          type="datetime"
          :value="parseDate(tr.kind.Once.fire_at)"
          @update:value="tr.kind.Once.fire_at = $event ? new Date($event).toISOString() : ''"
          clearable
        />
      </div>

      <!-- AgentStarted Editor -->
      <div v-if="tr.kind === 'AgentStarted'" class="text-xs text-slate-600 dark:text-zinc-400 bg-emerald-50 dark:bg-emerald-950/20 border border-emerald-200 dark:border-emerald-800/30 p-2.5 rounded">
        当 easyJob 调度服务启动或开机后台运行瞬间，将自动执行一次该任务。
      </div>

      <!-- Cron Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Cron' in tr.kind" class="grid grid-cols-2 gap-3 text-xs">
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">Cron 表达式（分 时 日 月 周）</label>
          <NInput
            v-model:value="tr.kind.Cron.expression"
            :status="cronInvalid(tr) ? 'error' : undefined"
            placeholder="例如 */5 * * * *"
            size="small"
          />
          <p v-if="cronInvalid(tr)" class="mt-1 text-red-500 dark:text-red-400">表达式无效，需为 5 字段 cron（如 */5 * * * *）</p>
        </div>
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">时区 (Timezone)</label>
          <NInput
            v-model:value="tr.kind.Cron.timezone"
            placeholder="例如 Asia/Shanghai, UTC"
            size="small"
          />
        </div>
      </div>

      <!-- Fuzzy Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Fuzzy' in tr.kind" class="space-y-2.5 text-xs">
        <div class="grid grid-cols-2 gap-3">
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">周期</label>
            <NSelect
              :value="getFuzzyPeriod(tr)"
              :options="fuzzyPeriodOptions"
              size="small"
              @update:value="setFuzzyPeriod(tr, $event)"
            />
          </div>
          <div v-if="getFuzzyPeriod(tr) === 'Weekly'">
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">每周执行日</label>
            <NCheckboxGroup :value="getFuzzyWeeklyDays(tr)" @update:value="setFuzzyWeeklyDays(tr, $event as Weekday[])">
              <div class="flex flex-wrap gap-2">
                <NCheckbox v-for="d in dayOptions" :key="d.value" :value="d.value" :label="d.label" size="small" />
              </div>
            </NCheckboxGroup>
          </div>
        </div>

        <div class="grid grid-cols-3 gap-3">
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">窗口开始 (HH:mm:ss)</label>
            <input
              type="time"
              step="1"
              :value="tr.kind.Fuzzy.window_start"
              @change="tr.kind.Fuzzy.window_start = normalizeTime(($event.target as HTMLInputElement).value)"
              class="w-full px-2.5 py-1.5 rounded border border-slate-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-xs font-mono"
            />
          </div>
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">窗口结束 (HH:mm:ss)</label>
            <input
              type="time"
              step="1"
              :value="tr.kind.Fuzzy.window_end"
              @change="tr.kind.Fuzzy.window_end = normalizeTime(($event.target as HTMLInputElement).value)"
              class="w-full px-2.5 py-1.5 rounded border border-slate-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-xs font-mono"
            />
          </div>
          <div>
            <label class="block text-slate-500 dark:text-zinc-400 mb-1">时区 (Timezone)</label>
            <NInput
              v-model:value="tr.kind.Fuzzy.timezone"
              placeholder="例如 Asia/Shanghai, UTC"
              size="small"
            />
          </div>
        </div>

        <p v-if="fuzzyWindowInvalid(tr)" class="text-red-500 dark:text-red-400">窗口结束时间必须晚于开始时间</p>
      </div>

      <!-- Network Editor -->
      <div v-if="typeof tr.kind === 'object' && 'Network' in tr.kind" class="space-y-2.5 text-xs">
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">触发事件（至少选一个）</label>
          <NCheckboxGroup v-model:value="tr.kind.Network.events">
            <div class="flex flex-wrap gap-2">
              <NCheckbox v-for="e in networkEventOptions" :key="e.value" :value="e.value" :label="e.label" size="small" />
            </div>
          </NCheckboxGroup>
          <p v-if="networkEventsInvalid(tr)" class="text-red-500 dark:text-red-400">请至少选择一个触发事件</p>
        </div>

        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">网络名称（可选）</label>
          <NInput
            :value="tr.kind.Network.network_name ?? ''"
            @update:value="tr.kind.Network.network_name = $event === '' ? null : $event"
            placeholder="留空 = 任意网络；填写则精确匹配 WiFi 名称（区分大小写）"
            size="small"
            clearable
          />
        </div>
      </div>
    </div>
  </div>
</template>
