<script lang="ts">
export { getEmptyTask } from '../../types/task';
</script>

<script setup lang="ts">
import { ref, watch } from 'vue';
import {
  NDrawer,
  NDrawerContent,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NSwitch,
  NButton,
  NTabs,
  NTabPane,
  useMessage,
} from 'naive-ui';
import TriggerEditor from './TriggerEditor.vue';
import ActionEditor from './ActionEditor.vue';
import type { Task, ConcurrencyPolicy, MissedRunPolicy, TaskNotificationPolicy } from '../../types/task';
import { getEmptyTask } from '../../types/task';
import { useTaskStore } from '../../stores/taskStore';

const props = defineProps<{
  show: boolean;
  task: Task | null;
}>();

const emit = defineEmits<{
  (e: 'update:show', val: boolean): void;
  (e: 'saved'): void;
}>();

const taskStore = useTaskStore();

let message: {
  success: (msg: string) => void;
  error: (msg: string) => void;
  warning: (msg: string) => void;
};
try {
  message = useMessage();
} catch {
  message = {
    success: (msg: string) => console.log(msg),
    error: (msg: string) => console.error(msg),
    warning: (msg: string) => console.warn(msg),
  };
}

const isSaving = ref(false);
const activeTab = ref('basic');
const currentTask = ref<Task>(getEmptyTask());

watch(
  () => props.task,
  (t) => {
    if (t) {
      currentTask.value = JSON.parse(JSON.stringify(t));
      if (!currentTask.value.execution_policy.notification) {
        currentTask.value.execution_policy.notification = 'None';
      }
    } else {
      currentTask.value = getEmptyTask();
    }
    activeTab.value = 'basic';
  },
  { immediate: true }
);

watch(
  () => props.show,
  (show) => {
    if (show) {
      if (props.task) {
        currentTask.value = JSON.parse(JSON.stringify(props.task));
        if (!currentTask.value.execution_policy.notification) {
          currentTask.value.execution_policy.notification = 'None';
        }
      } else {
        currentTask.value = getEmptyTask();
      }
      activeTab.value = 'basic';
    }
  }
);

const concurrencyOptions: { label: string; value: ConcurrencyPolicy }[] = [
  { label: '单例跳过 (SkipIfRunning - 默认推荐)', value: 'SkipIfRunning' },
  { label: '允许并行 (AllowParallel)', value: 'AllowParallel' },
  { label: '至多排队一个 (QueueOne)', value: 'QueueOne' },
];

const missedRunOptions: { label: string; value: MissedRunPolicy }[] = [
  { label: '补跑一次 (RunOnce - 默认)', value: 'RunOnce' },
  { label: '直接跳过 (Skip)', value: 'Skip' },
];

const notificationOptions: { label: string; value: TaskNotificationPolicy }[] = [
  { label: '不通知 (默认)', value: 'None' },
  { label: '仅成功时通知 (OnlySuccess)', value: 'OnlySuccess' },
  { label: '仅失败时通知 (OnlyFailure)', value: 'OnlyFailure' },
  { label: '全部通知 (成功与失败均通知)', value: 'All' },
];

async function handleSave() {
  if (!currentTask.value.name.trim()) {
    message.warning('请输入任务名称');
    activeTab.value = 'basic';
    return;
  }

  if (currentTask.value.actions.length === 0) {
    message.warning('建议至少添加一个执行动作');
    activeTab.value = 'actions';
    return;
  }

  if (currentTask.value.execution_policy.timeout_secs !== null) {
    const t = Number(currentTask.value.execution_policy.timeout_secs);
    currentTask.value.execution_policy.timeout_secs = isNaN(t) || t <= 0 ? null : t;
  }

  isSaving.value = true;
  try {
    await taskStore.saveTask(currentTask.value);
    message.success('任务保存成功');
    emit('update:show', false);
    emit('saved');
  } catch (e: any) {
    message.error('保存失败: ' + (e?.message || e));
  } finally {
    isSaving.value = false;
  }
}
</script>

<template>
  <NDrawer :show="show" width="620" @update:show="$emit('update:show', $event)">
    <NDrawerContent :title="task ? '编辑任务' : '新建任务'" closable>
      <div class="pb-8">
        <NTabs v-model:value="activeTab" type="line" animated>
          <!-- Tab 1: Basic Info & Policy -->
          <NTabPane name="basic" tab="基本配置与策略">
            <div class="space-y-4 pt-2">
              <NForm label-placement="top" size="small">
                <NFormItem label="任务名称" required>
                  <NInput
                    v-model:value="currentTask.name"
                    placeholder="例如：每日数据库定时备份与上传"
                  />
                </NFormItem>

                <NFormItem label="任务描述">
                  <NInput
                    v-model:value="currentTask.description"
                    type="textarea"
                    placeholder="任务的业务用途、依赖环境及注意事项说明..."
                    :autosize="{ minRows: 2, maxRows: 4 }"
                  />
                </NFormItem>

                <div class="flex items-center gap-3 p-3 bg-slate-50 dark:bg-zinc-800/40 rounded-lg border border-slate-200/80 dark:border-zinc-800 mb-4">
                  <NSwitch v-model:value="currentTask.enabled" size="medium" />
                  <div>
                    <span class="text-xs font-semibold">任务启用状态</span>
                    <p class="text-xs text-slate-400 dark:text-zinc-500">
                      {{ currentTask.enabled ? '当前已启用，触发器到达时将正常调度执行' : '已停用，触发器将不会触发该任务' }}
                    </p>
                  </div>
                </div>

                <NFormItem label="工作目录 (Working Directory)">
                  <NInput
                    v-model:value="currentTask.working_directory"
                    placeholder="留空表示跟随 easyJob 默认运行路径"
                  />
                </NFormItem>

                <div class="grid grid-cols-2 gap-4">
                  <NFormItem label="并发控制策略">
                    <NSelect
                      v-model:value="currentTask.execution_policy.concurrency_policy"
                      :options="concurrencyOptions"
                    />
                  </NFormItem>

                  <NFormItem label="错失触发策略">
                    <NSelect
                      v-model:value="currentTask.execution_policy.missed_run_policy"
                      :options="missedRunOptions"
                    />
                  </NFormItem>
                </div>

                <div class="grid grid-cols-3 gap-3">
                  <NFormItem label="超时时间 (秒)">
                    <NInputNumber
                      v-model:value="currentTask.execution_policy.timeout_secs"
                      :min="1"
                      placeholder="留空不限制"
                      class="w-full"
                    />
                  </NFormItem>

                  <NFormItem label="最大重试次数">
                    <NInputNumber
                      v-model:value="currentTask.execution_policy.retry_policy.max_retries"
                      :min="0"
                      class="w-full"
                    />
                  </NFormItem>

                  <NFormItem label="重试间隔 (秒)">
                    <NInputNumber
                      v-model:value="currentTask.execution_policy.retry_policy.delay_secs"
                      :min="0"
                      class="w-full"
                    />
                  </NFormItem>
                </div>

                <NFormItem label="执行结果通知">
                  <NSelect
                    v-model:value="currentTask.execution_policy.notification"
                    :options="notificationOptions"
                  />
                </NFormItem>
              </NForm>
            </div>
          </NTabPane>

          <!-- Tab 2: Triggers -->
          <NTabPane name="triggers" :tab="`触发器 (${currentTask.triggers.length})`">
            <div class="pt-2">
              <TriggerEditor
                :triggers="currentTask.triggers"
                :task-id="currentTask.id"
                @update:triggers="currentTask.triggers = $event"
              />
            </div>
          </NTabPane>

          <!-- Tab 3: Actions -->
          <NTabPane name="actions" :tab="`执行动作 (${currentTask.actions.length})`">
            <div class="pt-2">
              <ActionEditor
                :actions="currentTask.actions"
                :task-id="currentTask.id"
                @update:actions="currentTask.actions = $event"
              />
            </div>
          </NTabPane>
        </NTabs>
      </div>

      <template #footer>
        <div class="flex justify-between items-center w-full">
          <div class="text-xs text-slate-400">
            {{ currentTask.triggers.length }} 个触发器 · {{ currentTask.actions.length }} 个动作
          </div>
          <div class="flex gap-2.5">
            <NButton size="small" @click="$emit('update:show', false)">取消</NButton>
            <NButton
              size="small"
              type="primary"
              :loading="isSaving"
              class="bg-emerald-600 hover:bg-emerald-500"
              @click="handleSave"
            >
              保存任务
            </NButton>
          </div>
        </div>
      </template>
    </NDrawerContent>
  </NDrawer>
</template>
