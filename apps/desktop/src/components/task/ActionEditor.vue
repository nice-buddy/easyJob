<script setup lang="ts">
import { NSelect, NInput, NButton, NSwitch } from 'naive-ui';
import { Plus, Trash2 } from 'lucide-vue-next';
import type { Action, ActionKind, ScriptEncoding } from '../../types/task';
import { getActionType, isWindowsPlatform } from '../../types/task';

const props = defineProps<{
  actions: Action[];
  taskId: string;
}>();

const emit = defineEmits<{
  (e: 'update:actions', actions: Action[]): void;
}>();

const isWindows = isWindowsPlatform();

const encodingOptions = [
  { label: 'UTF-8 (默认)', value: 'utf8' },
  { label: 'GBK (中文旧脚本)', value: 'gbk' },
];

function getEncoding(action: Action): ScriptEncoding {
  if ('ExecuteCmd' in action.kind) return action.kind.ExecuteCmd.encoding ?? 'utf8';
  if ('ExecutePowerShell' in action.kind) return action.kind.ExecutePowerShell.encoding ?? 'utf8';
  return 'utf8';
}

function setEncoding(action: Action, value: ScriptEncoding) {
  if ('ExecuteCmd' in action.kind) {
    action.kind.ExecuteCmd.encoding = value;
  } else if ('ExecutePowerShell' in action.kind) {
    action.kind.ExecutePowerShell.encoding = value;
  }
}

const platformDefaultOptions = isWindows
  ? [
      { label: 'Windows CMD (ExecuteCmd)', value: 'ExecuteCmd' },
      { label: 'PowerShell 脚本 (ExecutePowerShell)', value: 'ExecutePowerShell' },
      { label: '独立可执行程序 (ExecuteProgram)', value: 'ExecuteProgram' },
    ]
  : [
      { label: 'Shell 脚本 / 命令 (ExecuteShell)', value: 'ExecuteShell' },
      { label: '独立可执行程序 (ExecuteProgram)', value: 'ExecuteProgram' },
    ];

function getActionTypeOptions(action: Action) {
  const currentType = getActionType(action.kind);
  if (!platformDefaultOptions.some((opt) => opt.value === currentType)) {
    const allLabels: Record<string, string> = {
      ExecuteShell: 'Shell 脚本 / 命令 (ExecuteShell)',
      ExecuteProgram: '独立可执行程序 (ExecuteProgram)',
      ExecutePowerShell: 'PowerShell 脚本 (ExecutePowerShell)',
      ExecuteCmd: 'Windows CMD (ExecuteCmd)',
    };
    return [
      ...platformDefaultOptions,
      { label: allLabels[currentType] || currentType, value: currentType },
    ];
  }
  return platformDefaultOptions;
}

function addAction() {
  const defaultKind: ActionKind = isWindows
    ? { ExecuteCmd: { command: '' } }
    : { ExecuteShell: { command: '' } };
  const newAction: Action = {
    id: crypto.randomUUID(),
    task_id: props.taskId,
    sequence: props.actions.length + 1,
    enabled: true,
    kind: defaultKind,
  };
  emit('update:actions', [...props.actions, newAction]);
}

function removeAction(index: number) {
  const next = [...props.actions];
  next.splice(index, 1);
  next.forEach((act, idx) => {
    act.sequence = idx + 1;
  });
  emit('update:actions', next);
}

function changeKindType(action: Action, type: string) {
  if (type === 'ExecuteShell') {
    action.kind = { ExecuteShell: { command: '' } };
  } else if (type === 'ExecuteProgram') {
    action.kind = { ExecuteProgram: { program: '', args: [] } };
  } else if (type === 'ExecutePowerShell') {
    action.kind = { ExecutePowerShell: { script: '', no_profile: true } };
  } else if (type === 'ExecuteCmd') {
    action.kind = { ExecuteCmd: { command: '' } };
  }
}

function parseCliArgs(input: string): string[] {
  const matches = input.match(/(?:[^\s"']+|"[^"]*"|'[^']*')+/g);
  if (!matches) return [];
  return matches.map((arg) => {
    if ((arg.startsWith('"') && arg.endsWith('"')) || (arg.startsWith("'") && arg.endsWith("'"))) {
      return arg.slice(1, -1);
    }
    return arg;
  });
}

function getArgsString(args: string[]): string {
  return args.map((arg) => (arg.includes(' ') ? `"${arg}"` : arg)).join(' ');
}

function setArgsString(action: Action, val: string) {
  if ('ExecuteProgram' in action.kind) {
    action.kind.ExecuteProgram.args = parseCliArgs(val.trim());
  }
}
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <div>
        <h3 class="text-sm font-semibold text-slate-800 dark:text-zinc-200">执行动作 (Actions)</h3>
        <p class="text-xs text-slate-400 dark:text-zinc-500">按顺序执行的一组指令或程序</p>
      </div>
      <NButton size="tiny" secondary @click="addAction">
        <template #icon><Plus class="w-3 h-3" /></template>
        添加动作
      </NButton>
    </div>

    <div v-if="actions.length === 0" class="text-xs text-slate-400 dark:text-zinc-500 py-4 text-center border border-dashed border-slate-200 dark:border-zinc-800 rounded-lg">
      暂未配置执行动作，建议至少添加一个动作以使任务有意义。
    </div>

    <div
      v-for="(act, idx) in actions"
      :key="act.id"
      class="p-3.5 border border-slate-200 dark:border-zinc-800 rounded-lg space-y-3 bg-slate-50/70 dark:bg-zinc-800/30 transition hover:border-slate-300 dark:hover:border-zinc-700"
    >
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-2 flex-1">
          <span class="text-xs font-semibold text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-950/40 px-2 py-0.5 rounded border border-emerald-200/50 dark:border-emerald-800/30">
            步骤 {{ act.sequence }}
          </span>
          <NSelect
            :value="getActionType(act.kind)"
            :options="getActionTypeOptions(act)"
            size="small"
            class="w-64"
            @update:value="changeKindType(act, $event)"
          />
          <div class="flex items-center gap-1.5 text-xs text-slate-500">
            <span>启用</span>
            <NSwitch v-model:value="act.enabled" size="small" />
          </div>
        </div>

        <NButton size="tiny" text type="error" @click="removeAction(idx)">
          <Trash2 class="w-4 h-4" />
        </NButton>
      </div>

      <!-- ExecuteShell Editor -->
      <div v-if="'ExecuteShell' in act.kind" class="space-y-1 text-xs">
        <label class="block text-slate-500 dark:text-zinc-400">Shell 指令 / 脚本命令</label>
        <NInput
          v-model:value="act.kind.ExecuteShell.command"
          type="textarea"
          :autosize="{ minRows: 2, maxRows: 6 }"
          placeholder="例如：echo 'Hello easyJob' && date"
          size="small"
        />
      </div>

      <!-- ExecuteProgram Editor -->
      <div v-if="'ExecuteProgram' in act.kind" class="space-y-2 text-xs">
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">可执行程序路径或系统命令</label>
          <NInput
            v-model:value="act.kind.ExecuteProgram.program"
            placeholder="例如：curl 或 /usr/bin/python3 或 C:\Tools\sync.exe"
            size="small"
          />
        </div>
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">程序参数列表 (空格分隔)</label>
          <NInput
            :value="getArgsString(act.kind.ExecuteProgram.args)"
            placeholder="例如：-s https://example.com -o output.json"
            size="small"
            @update:value="setArgsString(act, $event)"
          />
        </div>
      </div>

      <!-- ExecutePowerShell Editor -->
      <div v-if="'ExecutePowerShell' in act.kind" class="space-y-2 text-xs">
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">PowerShell 脚本内容</label>
          <NInput
            v-model:value="act.kind.ExecutePowerShell.script"
            type="textarea"
            :autosize="{ minRows: 2, maxRows: 6 }"
            placeholder="例如：Get-Process | Where-Object { $_.CPU -gt 10 }"
            size="small"
          />
        </div>
        <div class="flex items-center gap-2">
          <NSwitch v-model:value="act.kind.ExecutePowerShell.no_profile" size="small" />
          <span class="text-slate-500 dark:text-zinc-400">使用 -NoProfile 模式 (推荐，启动更快更稳定)</span>
        </div>
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">输出编码</label>
          <NSelect
            :value="getEncoding(act)"
            :options="encodingOptions"
            size="small"
            class="w-48"
            @update:value="setEncoding(act, $event)"
          />
        </div>
      </div>

      <!-- ExecuteCmd Editor -->
      <div v-if="'ExecuteCmd' in act.kind" class="space-y-2 text-xs">
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">Windows CMD 命令 (cmd.exe /C)</label>
          <NInput
            v-model:value="act.kind.ExecuteCmd.command"
            type="textarea"
            :autosize="{ minRows: 2, maxRows: 6 }"
            placeholder="例如：dir /s /b C:\Logs"
            size="small"
          />
        </div>
        <div>
          <label class="block text-slate-500 dark:text-zinc-400 mb-1">输出编码</label>
          <NSelect
            :value="getEncoding(act)"
            :options="encodingOptions"
            size="small"
            class="w-48"
            @update:value="setEncoding(act, $event)"
          />
        </div>
      </div>
    </div>
  </div>
</template>
