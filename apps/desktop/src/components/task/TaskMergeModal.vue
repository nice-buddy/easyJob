<script setup lang="ts">
import { ref, watch, computed } from 'vue';
import { NModal, NButton, NTag, useMessage } from 'naive-ui';
import { Check } from 'lucide-vue-next';
import TaskAccordionContent from './TaskAccordionContent.vue';
import type { Task } from '../../types/task';
import { compareTasks } from '../../utils/taskDiff';

const props = defineProps<{
  show: boolean;
  existingTask: Task | null;
  importedTask: Task | null;
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
  (e: 'merged', task: Task): void;
}>();

const message = useMessage();
const mergedTask = ref<Task | null>(null);

watch(
  () => [props.show, props.importedTask],
  ([show]) => {
    if (show && props.importedTask) {
      mergedTask.value = JSON.parse(JSON.stringify(props.importedTask));
      if (mergedTask.value && !mergedTask.value.execution_policy.log_retention) {
        mergedTask.value.execution_policy.log_retention = { mode: 'SystemDefault' };
      }
    }
  },
  { immediate: true }
);

const diffResult = computed(() => {
  if (!props.existingTask || !props.importedTask) {
    return {
      hasDiff: false,
      basicDiff: false,
      policyDiff: false,
      triggersDiff: false,
      actionsDiff: false,
      envDiff: false,
      diffFields: new Set<string>(),
    };
  }
  return compareTasks(props.existingTask, props.importedTask);
});

// 块级一键采用已有配置
function copyExistingSection(section: 'basic' | 'policy' | 'triggers' | 'actions' | 'env') {
  if (!props.existingTask || !mergedTask.value) return;
  const src = props.existingTask;
  const dst = mergedTask.value;

  if (section === 'basic') {
    dst.description = src.description;
    dst.enabled = src.enabled;
    dst.working_directory = src.working_directory;
  } else if (section === 'policy') {
    dst.execution_policy = JSON.parse(JSON.stringify(src.execution_policy));
    if (!dst.execution_policy.log_retention) {
      dst.execution_policy.log_retention = { mode: 'SystemDefault' };
    }
  } else if (section === 'triggers') {
    dst.triggers = JSON.parse(JSON.stringify(src.triggers));
  } else if (section === 'actions') {
    dst.actions = JSON.parse(JSON.stringify(src.actions));
  } else if (section === 'env') {
    dst.environment = JSON.parse(JSON.stringify(src.environment || {}));
  }
  message.success('已应用已有配置到最终结果');
}

// 块级一键还原导入配置
function restoreImportedSection(section: 'basic' | 'policy' | 'triggers' | 'actions' | 'env') {
  if (!props.importedTask || !mergedTask.value) return;
  const src = props.importedTask;
  const dst = mergedTask.value;

  if (section === 'basic') {
    dst.description = src.description;
    dst.enabled = src.enabled;
    dst.working_directory = src.working_directory;
  } else if (section === 'policy') {
    dst.execution_policy = JSON.parse(JSON.stringify(src.execution_policy));
    if (!dst.execution_policy.log_retention) {
      dst.execution_policy.log_retention = { mode: 'SystemDefault' };
    }
  } else if (section === 'triggers') {
    dst.triggers = JSON.parse(JSON.stringify(src.triggers));
  } else if (section === 'actions') {
    dst.actions = JSON.parse(JSON.stringify(src.actions));
  } else if (section === 'env') {
    dst.environment = JSON.parse(JSON.stringify(src.environment || {}));
  }
  message.success('已还原导入配置到最终结果');
}

function handleAdoptAllExisting() {
  if (!props.existingTask) return;
  mergedTask.value = JSON.parse(JSON.stringify(props.existingTask));
  if (mergedTask.value && !mergedTask.value.execution_policy.log_retention) {
    mergedTask.value.execution_policy.log_retention = { mode: 'SystemDefault' };
  }
}

function handleRestoreAllImported() {
  if (!props.importedTask) return;
  mergedTask.value = JSON.parse(JSON.stringify(props.importedTask));
  if (mergedTask.value && !mergedTask.value.execution_policy.log_retention) {
    mergedTask.value.execution_policy.log_retention = { mode: 'SystemDefault' };
  }
}

function handleConfirmMerge() {
  if (!mergedTask.value) return;
  emit('merged', mergedTask.value);
  emit('update:show', false);
  message.success('任务合并配置已保存');
}
</script>

<template>
  <NModal
    :show="show"
    @update:show="$emit('update:show', $event)"
    preset="card"
    title="手动合并任务配置差异"
    style="width: 94vw; max-width: 1400px; height: 90vh;"
    size="small"
  >
    <template #header-extra>
      <span class="text-xs text-slate-500">
        任务名称: <strong class="text-slate-800 dark:text-zinc-200">{{ existingTask?.name }}</strong>
      </span>
    </template>

    <div v-if="existingTask && importedTask && mergedTask" class="grid grid-cols-3 gap-4 h-[75vh] overflow-hidden">
      <!-- Column 1: 已有配置 (只读) -->
      <div class="flex flex-col h-full border border-slate-200 dark:border-zinc-800 rounded-lg p-3 bg-slate-50/50 dark:bg-zinc-900/50 overflow-y-auto">
        <div class="flex items-center justify-between pb-2 mb-2 border-b border-slate-200 dark:border-zinc-800">
          <span class="font-bold text-xs text-slate-700 dark:text-zinc-300">第一栏：已有任务配置</span>
          <NTag size="tiny" type="default">只读</NTag>
        </div>
        <TaskAccordionContent
          :task="existingTask"
          :editable="false"
          column-title="已有任务"
        />
      </div>

      <!-- Column 2: 导入配置 (只读 + 高亮) -->
      <div class="flex flex-col h-full border border-amber-200 dark:border-amber-900/50 rounded-lg p-3 bg-amber-50/20 dark:bg-amber-950/10 overflow-y-auto">
        <div class="flex items-center justify-between pb-2 mb-2 border-b border-amber-200/60 dark:border-amber-900/50">
          <div class="flex items-center gap-1.5">
            <span class="font-bold text-xs text-slate-700 dark:text-zinc-300">第二栏：导入任务配置</span>
            <NTag v-if="diffResult.hasDiff" size="tiny" type="warning">含差异高亮</NTag>
          </div>
          <NTag size="tiny" type="default">只读</NTag>
        </div>
        <TaskAccordionContent
          :task="importedTask"
          :editable="false"
          :diff-fields="diffResult.diffFields"
          column-title="导入任务"
        />
      </div>

      <!-- Column 3: 最终合并结果 (可编辑) -->
      <div class="flex flex-col h-full border-2 border-emerald-500/40 dark:border-emerald-500/30 rounded-lg p-3 bg-white dark:bg-zinc-900 overflow-y-auto shadow-sm">
        <div class="flex items-center justify-between pb-2 mb-2 border-b border-slate-200 dark:border-zinc-800">
          <div class="flex items-center gap-1.5">
            <span class="font-bold text-xs text-emerald-600 dark:text-emerald-400">第三栏：最终合并结果</span>
            <NTag size="tiny" type="success">可编辑</NTag>
          </div>
          <div class="flex items-center gap-1">
            <NButton size="tiny" secondary @click="handleAdoptAllExisting">
              全部采用已有
            </NButton>
            <NButton size="tiny" secondary @click="handleRestoreAllImported">
              全部还原导入
            </NButton>
          </div>
        </div>

        <!-- 快捷操作工具条 -->
        <div class="bg-slate-100 dark:bg-zinc-800/80 p-1.5 rounded flex items-center justify-between text-[11px] mb-2">
          <span class="text-slate-500">块级快捷操作：</span>
          <div class="flex flex-col gap-1">
            <div class="flex gap-1 justify-end">
              <span class="text-slate-400 mr-2">采用已有:</span>
              <NButton size="tiny" text type="primary" @click="copyExistingSection('basic')">◀ 基本</NButton>
              <NButton size="tiny" text type="primary" @click="copyExistingSection('policy')">◀ 策略</NButton>
              <NButton size="tiny" text type="primary" @click="copyExistingSection('triggers')">◀ 触发器</NButton>
              <NButton size="tiny" text type="primary" @click="copyExistingSection('actions')">◀ 动作</NButton>
              <NButton size="tiny" text type="primary" @click="copyExistingSection('env')">◀ 环境</NButton>
            </div>
            <div class="flex gap-1 justify-end">
              <span class="text-slate-400 mr-2">还原导入:</span>
              <NButton size="tiny" text type="info" @click="restoreImportedSection('basic')">▶ 基本</NButton>
              <NButton size="tiny" text type="info" @click="restoreImportedSection('policy')">▶ 策略</NButton>
              <NButton size="tiny" text type="info" @click="restoreImportedSection('triggers')">▶ 触发器</NButton>
              <NButton size="tiny" text type="info" @click="restoreImportedSection('actions')">▶ 动作</NButton>
              <NButton size="tiny" text type="info" @click="restoreImportedSection('env')">▶ 环境</NButton>
            </div>
          </div>
        </div>

        <TaskAccordionContent
          :task="mergedTask"
          :editable="true"
          column-title="合并结果"
        />
      </div>
    </div>

    <template #footer>
      <div class="flex justify-end gap-2">
        <NButton size="small" @click="$emit('update:show', false)">取消</NButton>
        <NButton
          size="small"
          type="primary"
          class="bg-emerald-600 hover:bg-emerald-500"
          @click="handleConfirmMerge"
        >
          <template #icon>
            <Check class="w-3.5 h-3.5" />
          </template>
          确认合并此任务
        </NButton>
      </div>
    </template>
  </NModal>
</template>
