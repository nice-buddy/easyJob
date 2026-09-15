<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useAgentStore } from '../stores/agentStore';
import { NCard, NDescriptions, NDescriptionsItem, NButton, NSwitch, useMessage } from 'naive-ui';
import { setAutostart, initAutostartDefault } from '../services/autostart';

const agentStore = useAgentStore();
const message = useMessage();

const autostartLoading = ref(false);
const autostartActive = ref(true);

onMounted(async () => {
  try {
    autostartActive.value = await initAutostartDefault();
  } catch (e) {
    autostartActive.value = false;
  }
});

async function handleToggleAutostart(value: boolean) {
  autostartLoading.value = true;
  try {
    await setAutostart(value);
    autostartActive.value = value;
    message.success(value ? '已开启开机自启动' : '已关闭开机自启动');
  } catch (e: any) {
    message.error(`设置开机自启失败: ${e?.message || e}`);
    autostartActive.value = !value;
  } finally {
    autostartLoading.value = false;
  }
}
</script>

<template>
  <div class="max-w-3xl space-y-6">
    <div>
      <h1 class="text-xl font-bold">系统设置与状态</h1>
      <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">管理系统启动偏好及查看 easyJob 守护进程健康指标</p>
    </div>

    <!-- 启动偏好配置 -->
    <NCard title="系统与启动偏好" size="small">
      <div class="flex items-center justify-between py-1">
        <div class="space-y-1">
          <div class="text-sm font-medium text-slate-800 dark:text-zinc-200">开机自启动</div>
          <div class="text-xs text-slate-500 dark:text-zinc-400">
            开机时在后台静默运行 easyJob 并最小化到系统托盘，自动守护并按时调度各项定时任务
          </div>
        </div>
        <NSwitch
          v-model:value="autostartActive"
          :loading="autostartLoading"
          @update:value="handleToggleAutostart"
        />
      </div>
    </NCard>

    <!-- Agent 守护进程状态（横向展示） -->
    <NCard title="Agent 守护进程状态" size="small">
      <NDescriptions label-placement="left" :column="2" bordered size="small">
        <NDescriptionsItem label="连接状态">
          <span :class="agentStore.isConnected ? 'text-emerald-500 font-semibold' : 'text-rose-500 font-semibold'">
            {{ agentStore.isConnected ? '运行中 (Connected)' : '未连接 (Disconnected)' }}
          </span>
        </NDescriptionsItem>
        <NDescriptionsItem label="Agent 版本">
          <span class="font-medium">{{ agentStore.status?.version || '-' }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="运行时长">
          <span class="font-medium">{{ agentStore.status?.uptime_secs != null ? `${agentStore.status.uptime_secs} s` : '0 s' }}</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="当前活跃任务">
          <span class="font-medium">{{ agentStore.status?.active_tasks ?? 0 }} 个</span>
        </NDescriptionsItem>
        <NDescriptionsItem label="并发执行中">
          <span class="font-medium">{{ agentStore.status?.running_executions ?? 0 }} 个</span>
        </NDescriptionsItem>
      </NDescriptions>

      <template #action>
        <NButton size="small" secondary @click="agentStore.fetchStatus()">
          重新检测连接
        </NButton>
      </template>
    </NCard>
  </div>
</template>
