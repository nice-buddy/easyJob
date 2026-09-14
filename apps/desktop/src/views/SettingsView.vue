<script setup lang="ts">
import { useAgentStore } from '../stores/agentStore';
import { NCard, NDescriptions, NDescriptionsItem, NButton } from 'naive-ui';

const agentStore = useAgentStore();
</script>

<template>
  <div class="max-w-3xl space-y-6">
    <div>
      <h1 class="text-xl font-bold">系统设置与状态</h1>
      <p class="text-xs text-slate-500 dark:text-zinc-400 mt-1">查看 easyJob 守护进程健康状态与偏好设定</p>
    </div>

    <NCard title="Agent 守护进程状态" size="small">
      <NDescriptions :column="2" bordered size="small">
        <NDescriptionsItem label="连接状态">
          <span :class="agentStore.isConnected ? 'text-emerald-500 font-semibold' : 'text-rose-500 font-semibold'">
            {{ agentStore.isConnected ? '运行中 (Connected)' : '未连接 (Disconnected)' }}
          </span>
        </NDescriptionsItem>
        <NDescriptionsItem label="Agent 版本">
          {{ agentStore.status?.version || '-' }}
        </NDescriptionsItem>
        <NDescriptionsItem label="运行时长 (秒)">
          {{ agentStore.status?.uptime_secs != null ? `${agentStore.status.uptime_secs} s` : '0 s' }}
        </NDescriptionsItem>
        <NDescriptionsItem label="当前活跃任务">
          {{ agentStore.status?.active_tasks ?? 0 }}
        </NDescriptionsItem>
        <NDescriptionsItem label="并发执行中">
          {{ agentStore.status?.running_executions ?? 0 }}
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
