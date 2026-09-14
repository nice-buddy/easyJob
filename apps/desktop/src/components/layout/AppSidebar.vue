<script setup lang="ts">
import { useAgentStore } from '../../stores/agentStore';
import { Calendar, History, Settings, Sun, Moon } from 'lucide-vue-next';

defineProps<{
  currentView: 'tasks' | 'executions' | 'settings';
  isDark: boolean;
}>();

defineEmits<{
  (e: 'change-view', view: 'tasks' | 'executions' | 'settings'): void;
  (e: 'toggle-theme'): void;
}>();

const agentStore = useAgentStore();
</script>

<template>
  <aside class="w-60 border-r border-slate-200 dark:border-zinc-800 flex flex-col justify-between p-4 bg-white dark:bg-zinc-900 select-none">
    <div>
      <!-- Brand -->
      <div class="flex items-center gap-3 px-2 py-3 mb-6">
        <div class="w-8 h-8 rounded-lg bg-emerald-600 flex items-center justify-center text-white font-bold shadow-md">
          eJ
        </div>
        <div>
          <div class="font-bold text-base tracking-wide">easyJob</div>
          <div class="text-xs text-slate-400 dark:text-zinc-500">定时任务调度台</div>
        </div>
      </div>

      <!-- Navigation Links -->
      <nav class="space-y-1">
        <button
          @click="$emit('change-view', 'tasks')"
          :class="[
            'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all',
            currentView === 'tasks'
              ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 font-semibold'
              : 'text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800'
          ]"
        >
          <Calendar class="w-4 h-4" />
          任务管理
        </button>

        <button
          @click="$emit('change-view', 'executions')"
          :class="[
            'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all',
            currentView === 'executions'
              ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 font-semibold'
              : 'text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800'
          ]"
        >
          <History class="w-4 h-4" />
          执行记录
        </button>

        <button
          @click="$emit('change-view', 'settings')"
          :class="[
            'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all',
            currentView === 'settings'
              ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 font-semibold'
              : 'text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800'
          ]"
        >
          <Settings class="w-4 h-4" />
          系统设置
        </button>
      </nav>
    </div>

    <!-- Bottom Status & Actions -->
    <div class="pt-4 border-t border-slate-200 dark:border-zinc-800 space-y-3">
      <!-- Agent status badge -->
      <div class="flex items-center justify-between px-2 py-1.5 rounded-md bg-slate-100 dark:bg-zinc-800/60 text-xs">
        <span class="text-slate-500 dark:text-zinc-400">Agent 服务</span>
        <div class="flex items-center gap-1.5 font-medium">
          <span
            class="w-2 h-2 rounded-full"
            :class="agentStore.isConnected ? 'bg-emerald-500 animate-pulse' : 'bg-rose-500'"
          />
          <span :class="agentStore.isConnected ? 'text-emerald-600 dark:text-emerald-400' : 'text-rose-500'">
            {{ agentStore.isConnected ? '已连接' : '未连接' }}
          </span>
        </div>
      </div>

      <!-- Theme Switch -->
      <button
        @click="$emit('toggle-theme')"
        class="w-full flex items-center justify-center gap-2 px-3 py-1.5 rounded-lg border border-slate-200 dark:border-zinc-800 text-xs text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800 transition"
      >
        <component :is="isDark ? Sun : Moon" class="w-3.5 h-3.5" />
        <span>{{ isDark ? '切换浅色' : '切换深色' }}</span>
      </button>
    </div>
  </aside>
</template>
