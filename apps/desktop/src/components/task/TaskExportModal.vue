<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import {
  NModal,
  NInput,
  NCheckbox,
  NButton,
  NTag,
  NEmpty,
  useMessage,
} from 'naive-ui';
import { Search, Download, CheckSquare, Square } from 'lucide-vue-next';
import type { Task } from '../../types/task';

const props = defineProps<{
  show: boolean;
  tasks: Task[];
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
}>();

const message = useMessage();
const searchQuery = ref('');
const selectedTaskIds = ref<string[]>([]);

// 每次打开弹窗默认全选
watch(
  () => props.show,
  (show) => {
    if (show) {
      selectedTaskIds.value = props.tasks.map((t) => t.id);
      searchQuery.value = '';
    }
  }
);

const filteredTasks = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return props.tasks;
  return props.tasks.filter(
    (t) =>
      t.name.toLowerCase().includes(q) ||
      (t.description && t.description.toLowerCase().includes(q))
  );
});

const isAllSelected = computed(() => {
  return (
    filteredTasks.value.length > 0 &&
    filteredTasks.value.every((t) => selectedTaskIds.value.includes(t.id))
  );
});

function handleToggleSelectAll() {
  if (isAllSelected.value) {
    const currentFilteredSet = new Set(filteredTasks.value.map((t) => t.id));
    selectedTaskIds.value = selectedTaskIds.value.filter((id) => !currentFilteredSet.has(id));
  } else {
    const set = new Set(selectedTaskIds.value);
    filteredTasks.value.forEach((t) => set.add(t.id));
    selectedTaskIds.value = Array.from(set);
  }
}

function handleExport() {
  if (selectedTaskIds.value.length === 0) {
    message.warning('请至少选择一个需要导出的任务');
    return;
  }

  const selectedTasks = props.tasks.filter((t) => selectedTaskIds.value.includes(t.id));
  const payload = {
    easyjob_version: '0.1.0',
    exported_at: new Date().toISOString(),
    tasks: selectedTasks,
  };

  const jsonStr = JSON.stringify(payload, null, 2);
  const blob = new Blob([jsonStr], { type: 'application/json' });
  const url = URL.createObjectURL(blob);

  const timestamp = new Date().toISOString().replace(/[-:T]/g, '').slice(0, 14);
  const a = document.createElement('a');
  a.href = url;
  a.download = `easyjob-tasks-${timestamp}.json`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);

  message.success(`成功导出 ${selectedTasks.length} 个任务`);
  emit('update:show', false);
}
</script>

<template>
  <NModal
    :show="show"
    @update:show="$emit('update:show', $event)"
    preset="card"
    title="导出任务配置"
    style="width: 580px; max-width: 95vw;"
    size="small"
  >
    <div class="space-y-4">
      <div class="flex items-center justify-between gap-3">
        <NInput
          v-model:value="searchQuery"
          placeholder="搜索任务名称或描述..."
          size="small"
          clearable
          class="flex-1"
        >
          <template #prefix>
            <Search class="w-3.5 h-3.5 text-slate-400" />
          </template>
        </NInput>

        <NButton size="small" secondary @click="handleToggleSelectAll">
          <template #icon>
            <component :is="isAllSelected ? Square : CheckSquare" class="w-3.5 h-3.5" />
          </template>
          {{ isAllSelected ? '取消全选' : '全选' }}
        </NButton>
      </div>

      <div class="text-xs text-slate-500 dark:text-zinc-400 flex items-center justify-between px-1">
        <span>勾选需要导出的任务：</span>
        <span>已选中 {{ selectedTaskIds.length }} / {{ tasks.length }} 个任务</span>
      </div>

      <!-- Task Selection List -->
      <div class="max-h-80 overflow-y-auto border border-slate-200 dark:border-zinc-800 rounded-lg p-2 space-y-1">
        <div v-if="filteredTasks.length === 0" class="py-8">
          <NEmpty description="未找到匹配的任务" />
        </div>
        <div
          v-for="task in filteredTasks"
          :key="task.id"
          class="flex items-center justify-between p-2 rounded hover:bg-slate-50 dark:hover:bg-zinc-800/60 transition cursor-pointer"
          @click="
            selectedTaskIds.includes(task.id)
              ? (selectedTaskIds = selectedTaskIds.filter((id) => id !== task.id))
              : selectedTaskIds.push(task.id)
          "
        >
          <div class="flex items-center gap-3 overflow-hidden">
            <NCheckbox
              :checked="selectedTaskIds.includes(task.id)"
              @click.stop
              @update:checked="
                $event
                  ? selectedTaskIds.push(task.id)
                  : (selectedTaskIds = selectedTaskIds.filter((id) => id !== task.id))
              "
            />
            <div class="min-w-0">
              <div class="font-medium text-xs truncate text-slate-800 dark:text-zinc-200">
                {{ task.name }}
              </div>
              <div class="text-[11px] text-slate-400 dark:text-zinc-500 truncate mt-0.5">
                {{ task.description || '暂无描述' }}
              </div>
            </div>
          </div>

          <div class="flex items-center gap-1.5 shrink-0">
            <NTag size="tiny" :bordered="false" type="info">
              {{ task.triggers.length }} 触发器
            </NTag>
            <NTag size="tiny" :bordered="false" type="default">
              {{ task.actions.length }} 动作
            </NTag>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex justify-end gap-2">
        <NButton size="small" @click="$emit('update:show', false)">取消</NButton>
        <NButton
          size="small"
          type="primary"
          class="bg-emerald-600 hover:bg-emerald-500"
          :disabled="selectedTaskIds.length === 0"
          @click="handleExport"
        >
          <template #icon>
            <Download class="w-3.5 h-3.5" />
          </template>
          确认导出 ({{ selectedTaskIds.length }})
        </NButton>
      </div>
    </template>
  </NModal>
</template>
