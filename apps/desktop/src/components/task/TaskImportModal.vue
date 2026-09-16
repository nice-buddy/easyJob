<script setup lang="ts">
import { ref } from 'vue';
import {
  NModal,
  NButton,
  NTag,
  NRadioGroup,
  NRadio,
  useMessage,
} from 'naive-ui';
import { UploadCloud, FileText } from 'lucide-vue-next';
import TaskMergeModal from './TaskMergeModal.vue';
import type { Task } from '../../types/task';
import { useTaskStore } from '../../stores/taskStore';

const props = defineProps<{
  show: boolean;
  existingTasks: Task[];
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
  (e: 'imported'): void;
}>();

const taskStore = useTaskStore();
const message = useMessage();

interface ImportItem {
  task: Task;
  isConflict: boolean;
  existingMatch?: Task;
  action: 'create' | 'overwrite' | 'skip' | 'merged';
  mergedTask?: Task;
}

const importItems = ref<ImportItem[]>([]);
const hasFile = ref(false);
const fileName = ref('');
const showMergeModal = ref(false);
const activeMergingItem = ref<ImportItem | null>(null);

function handleFileChange(event: Event) {
  const target = event.target as HTMLInputElement;
  const file = target.files?.[0];
  if (!file) return;

  fileName.value = file.name;
  const reader = new FileReader();
  reader.onload = (e) => {
    try {
      const content = e.target?.result as string;
      const parsed = JSON.parse(content);
      const rawTasks: Task[] = Array.isArray(parsed)
        ? parsed
        : Array.isArray(parsed?.tasks)
        ? parsed.tasks
        : [];

      if (rawTasks.length === 0) {
        message.warning('文件格式正确，但未找到可导入的任务数据');
        return;
      }

      const existingMap = new Map<string, Task>();
      props.existingTasks.forEach((t) => existingMap.set(t.name, t));

      importItems.value = rawTasks.map((t) => {
        const existing = existingMap.get(t.name);
        if (existing) {
          return {
            task: t,
            isConflict: true,
            existingMatch: existing,
            action: 'overwrite', // 默认推荐覆盖
          };
        }
        return {
          task: t,
          isConflict: false,
          action: 'create',
        };
      });

      hasFile.value = true;
    } catch (err: any) {
      message.error('解析 JSON 失败: ' + (err?.message || err));
    }
  };
  reader.readAsText(file);
}

function openManualMerge(item: ImportItem) {
  activeMergingItem.value = item;
  showMergeModal.value = true;
}

function handleMergeResolved(merged: Task) {
  if (activeMergingItem.value) {
    activeMergingItem.value.mergedTask = merged;
    activeMergingItem.value.action = 'merged';
  }
}

function setAllConflictAction(act: 'overwrite' | 'skip') {
  importItems.value.forEach((item) => {
    if (item.isConflict) {
      item.action = act;
    }
  });
}

async function handleConfirmImport() {
  const toSave: Task[] = [];

  for (const item of importItems.value) {
    if (item.action === 'skip') continue;

    if (item.action === 'create') {
      const newId = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'task-' + Math.random().toString(36).substring(2, 9);
      const cloned = JSON.parse(JSON.stringify(item.task)) as Task;
      cloned.id = newId;
      cloned.version = 1;
      cloned.triggers.forEach((tr) => {
        tr.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'tr-' + Math.random().toString(36).substring(2, 9);
        tr.task_id = newId;
      });
      cloned.actions.forEach((act) => {
        act.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'act-' + Math.random().toString(36).substring(2, 9);
        act.task_id = newId;
      });
      toSave.push(cloned);
    } else if (item.action === 'overwrite' && item.existingMatch) {
      const existingId = item.existingMatch.id;
      const cloned = JSON.parse(JSON.stringify(item.task)) as Task;
      cloned.id = existingId;
      cloned.version = (item.existingMatch.version || 1) + 1;
      cloned.triggers.forEach((tr) => {
        tr.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'tr-' + Math.random().toString(36).substring(2, 9);
        tr.task_id = existingId;
      });
      cloned.actions.forEach((act) => {
        act.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'act-' + Math.random().toString(36).substring(2, 9);
        act.task_id = existingId;
      });
      toSave.push(cloned);
    } else if (item.action === 'merged' && item.existingMatch && item.mergedTask) {
      const existingId = item.existingMatch.id;
      const cloned = JSON.parse(JSON.stringify(item.mergedTask)) as Task;
      cloned.id = existingId;
      cloned.version = (item.existingMatch.version || 1) + 1;
      cloned.triggers.forEach((tr) => {
        tr.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'tr-' + Math.random().toString(36).substring(2, 9);
        tr.task_id = existingId;
      });
      cloned.actions.forEach((act) => {
        act.id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'act-' + Math.random().toString(36).substring(2, 9);
        act.task_id = existingId;
      });
      toSave.push(cloned);
    }
  }

  if (toSave.length === 0) {
    message.info('未选择任何需导入的任务');
    emit('update:show', false);
    return;
  }

  try {
    for (const t of toSave) {
      await taskStore.saveTask(t);
    }
    message.success(`成功导入并更新 ${toSave.length} 个任务`);
    emit('imported');
    emit('update:show', false);
  } catch (e: any) {
    message.error('导入保存失败: ' + (e?.message || e));
  }
}
</script>

<template>
  <NModal
    :show="show"
    @update:show="$emit('update:show', $event)"
    preset="card"
    title="导入任务配置"
    style="width: 720px; max-width: 95vw;"
    size="small"
  >
    <!-- 步骤 1: 待选文件 -->
    <div v-if="!hasFile" class="py-8 flex flex-col items-center justify-center border-2 border-dashed border-slate-200 dark:border-zinc-800 rounded-xl hover:border-emerald-500/60 transition cursor-pointer relative">
      <input
        type="file"
        accept=".json"
        class="absolute inset-0 opacity-0 cursor-pointer"
        @change="handleFileChange"
      />
      <UploadCloud class="w-12 h-12 text-slate-400 mb-2" />
      <span class="text-sm font-semibold text-slate-700 dark:text-zinc-200">点击选择或拖拽 JSON 任务文件</span>
      <span class="text-xs text-slate-400 dark:text-zinc-500 mt-1">支持 easyJob 导出的任务配置包</span>
    </div>

    <!-- 步骤 2: 解析与冲突确认列表 -->
    <div v-else class="space-y-4">
      <div class="flex items-center justify-between text-xs px-1">
        <div class="flex items-center gap-2">
          <FileText class="w-4 h-4 text-emerald-600" />
          <span class="font-medium">{{ fileName }}</span>
          <span class="text-slate-400">({{ importItems.length }} 个任务)</span>
        </div>

        <div class="flex items-center gap-1.5">
          <span class="text-slate-400">同名批量:</span>
          <NButton size="tiny" secondary @click="setAllConflictAction('overwrite')">全部覆盖</NButton>
          <NButton size="tiny" secondary @click="setAllConflictAction('skip')">全部跳过</NButton>
        </div>
      </div>

      <div class="max-h-96 overflow-y-auto border border-slate-200 dark:border-zinc-800 rounded-lg divide-y divide-slate-100 dark:divide-zinc-800/80">
        <div
          v-for="(item, idx) in importItems"
          :key="idx"
          class="p-3 flex items-center justify-between gap-4 hover:bg-slate-50 dark:hover:bg-zinc-800/40 transition text-xs"
        >
          <div class="space-y-1 min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="font-semibold text-slate-800 dark:text-zinc-200 truncate">{{ item.task.name }}</span>
              <NTag v-if="!item.isConflict" size="tiny" type="success" :bordered="false">新增</NTag>
              <NTag v-else size="tiny" type="warning" :bordered="false">同名冲突</NTag>
            </div>
            <div class="text-slate-400 text-[11px] truncate">
              {{ item.task.description || '暂无描述' }} · {{ item.task.triggers.length }} 触发器 · {{ item.task.actions.length }} 动作
            </div>
          </div>

          <!-- 操作选项 -->
          <div class="shrink-0 flex items-center gap-2">
            <div v-if="item.isConflict" class="flex items-center gap-2">
              <NRadioGroup v-model:value="item.action" size="small">
                <NRadio value="overwrite">覆盖</NRadio>
                <NRadio value="skip">跳过</NRadio>
                <NRadio value="merged" :disabled="!item.mergedTask">已合并</NRadio>
              </NRadioGroup>

              <NButton size="tiny" secondary type="primary" @click="openManualMerge(item)">
                {{ item.mergedTask ? '重新合并' : '手动合并' }}
              </NButton>
            </div>
            <div v-else>
              <NTag size="tiny" type="info">就绪导入</NTag>
            </div>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex justify-between items-center">
        <NButton v-if="hasFile" size="small" text @click="hasFile = false; importItems = []">重新选择文件</NButton>
        <div v-else></div>

        <div class="flex gap-2">
          <NButton size="small" @click="$emit('update:show', false)">取消</NButton>
          <NButton
            size="small"
            type="primary"
            class="bg-emerald-600 hover:bg-emerald-500"
            :disabled="!hasFile || importItems.length === 0"
            @click="handleConfirmImport"
          >
            确认导入
          </NButton>
        </div>
      </div>
    </template>
  </NModal>

  <!-- 手动合并弹窗 -->
  <TaskMergeModal
    v-model:show="showMergeModal"
    :existing-task="activeMergingItem?.existingMatch || null"
    :imported-task="activeMergingItem?.task || null"
    @merged="handleMergeResolved"
  />
</template>
