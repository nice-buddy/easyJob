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
  <aside class="w-44 border-r border-slate-200 dark:border-zinc-800 flex flex-col justify-between p-3 bg-white dark:bg-zinc-900 select-none shrink-0">
    <div>
      <!-- Brand -->
      <div class="flex items-center gap-2.5 px-1 py-2 mb-4">
        <img src="/favicon.svg" class="w-7 h-7 rounded-lg shadow-sm shrink-0 object-contain" alt="easyJob Logo" />
        <div class="min-w-0">

          <div class="font-bold text-sm tracking-wide leading-none">easyJob</div>
          <div class="text-[11px] text-slate-400 dark:text-zinc-500 mt-1 leading-none truncate">任务调度台</div>
        </div>
      </div>

      <!-- Navigation Links -->
      <nav class="space-y-1">
        <button
          @click="$emit('change-view', 'tasks')"
          :class="[
            'w-full flex items-center gap-2.5 px-2.5 py-2 rounded-lg text-xs font-medium transition-all',
            currentView === 'tasks'
              ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 font-semibold'
              : 'text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800'
          ]"
        >
          <Calendar class="w-4 h-4 shrink-0" />
          <span>任务管理</span>
        </button>

        <button
          @click="$emit('change-view', 'executions')"
          :class="[
            'w-full flex items-center gap-2.5 px-2.5 py-2 rounded-lg text-xs font-medium transition-all',
            currentView === 'executions'
              ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 font-semibold'
              : 'text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800'
          ]"
        >
          <History class="w-4 h-4 shrink-0" />
          <span>执行日志</span>
        </button>

        <button
          @click="$emit('change-view', 'settings')"
          :class="[
            'w-full flex items-center gap-2.5 px-2.5 py-2 rounded-lg text-xs font-medium transition-all',
            currentView === 'settings'
              ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 font-semibold'
              : 'text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800'
          ]"
        >
          <Settings class="w-4 h-4 shrink-0" />
          <span>系统设置</span>
        </button>
      </nav>
    </div>

    <!-- Bottom Status & Actions -->
    <div class="pt-3 border-t border-slate-200 dark:border-zinc-800 space-y-2.5">
      <!-- Agent status badge -->
      <div class="flex items-center justify-between px-2 py-1.5 rounded-md bg-slate-100 dark:bg-zinc-800/60 text-[11px]">
        <span class="text-slate-500 dark:text-zinc-400">Agent</span>
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
        class="w-full flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-lg border border-slate-200 dark:border-zinc-800 text-[11px] text-slate-600 dark:text-zinc-400 hover:bg-slate-100 dark:hover:bg-zinc-800 transition"
      >
        <component :is="isDark ? Sun : Moon" class="w-3.5 h-3.5 shrink-0" />
        <span>{{ isDark ? '浅色模式' : '深色模式' }}</span>
      </button>
    </div>
  </aside>
</template>
