<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from 'vue';
import { NConfigProvider, NMessageProvider, NDialogProvider, darkTheme, zhCN, dateZhCN } from 'naive-ui';
import AppSidebar from './components/layout/AppSidebar.vue';
import TasksView from './views/TasksView.vue';
import ExecutionsView from './views/ExecutionsView.vue';
import SettingsView from './views/SettingsView.vue';
import { useAgentStore } from './stores/agentStore';
import { useExecutionStore } from './stores/executionStore';

const isDark = ref(true);
const currentView = ref<'tasks' | 'executions' | 'settings'>('tasks');

watch(
  isDark,
  (val) => {
    if (typeof document !== 'undefined') {
      if (val) {
        document.documentElement.classList.add('dark');
      } else {
        document.documentElement.classList.remove('dark');
      }
    }
  },
  { immediate: true }
);

const agentStore = useAgentStore();
const executionStore = useExecutionStore();
let pollInterval: ReturnType<typeof setInterval> | null = null;

onMounted(async () => {
  await agentStore.fetchStatus();
  executionStore.initListeners();
  pollInterval = setInterval(() => {
    agentStore.fetchStatus();
  }, 5000);
});

onUnmounted(() => {
  if (pollInterval) {
    clearInterval(pollInterval);
  }
  executionStore.cleanupListeners();
});
</script>

<template>
  <NConfigProvider :theme="isDark ? darkTheme : null" :locale="zhCN" :date-locale="dateZhCN">
    <NMessageProvider>
      <NDialogProvider>
        <div :class="{ dark: isDark }" class="flex h-screen w-screen overflow-hidden bg-white dark:bg-zinc-950 text-slate-800 dark:text-zinc-200">
          <AppSidebar
            :current-view="currentView"
            :is-dark="isDark"
            @change-view="currentView = $event"
            @toggle-theme="isDark = !isDark"
          />
          <main class="flex-1 overflow-y-auto p-6 bg-slate-50/50 dark:bg-zinc-900/40">
            <TasksView v-if="currentView === 'tasks'" />
            <ExecutionsView v-else-if="currentView === 'executions'" />
            <SettingsView v-else-if="currentView === 'settings'" />
          </main>
        </div>
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
