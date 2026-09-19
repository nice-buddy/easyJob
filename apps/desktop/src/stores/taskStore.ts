import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import {
  listTasks,
  saveTask as apiSaveTask,
  deleteTask as apiDeleteTask,
  triggerTask as apiTriggerTask,
  getTaskOverview,
  rerollTrigger as apiRerollTrigger,
} from '../services/tauri';
import { onExecutionFinished } from '../services/events';
import type { Task, TaskId, TriggerId, TaskOverview } from '../types/task';

export const useTaskStore = defineStore('tasks', () => {
  const tasks = ref<Task[]>([]);
  const searchQuery = ref('');
  const loading = ref(false);
  const scheduleOverview = ref<Record<TaskId, TaskOverview>>({});
  let unlistenOverview: (() => void) | null = null;
  let overviewListenerStarted = false;

  const filteredTasks = computed(() => {
    if (!searchQuery.value.trim()) return tasks.value;
    const q = searchQuery.value.toLowerCase();
    return tasks.value.filter(
      (t) =>
        t.name.toLowerCase().includes(q) ||
        (t.description && t.description.toLowerCase().includes(q))
    );
  });

  async function loadTasks() {
    loading.value = true;
    try {
      tasks.value = await listTasks();
    } finally {
      loading.value = false;
    }
    await loadOverview();
  }

  async function loadOverview() {
    try {
      const list = await getTaskOverview();
      const next: Record<TaskId, TaskOverview> = {};
      for (const entry of list ?? []) {
        next[entry.task_id] = entry;
      }
      scheduleOverview.value = next;
    } catch {
      // 静默降级：agent 未运行 / IPC 失败时列表照常渲染，只是不显示时间信息
    }
  }

  async function rerollTrigger(taskId: TaskId, triggerId: TriggerId) {
    const result = await apiRerollTrigger(taskId, triggerId);
    const entry = scheduleOverview.value[taskId];
    if (entry) {
      scheduleOverview.value = {
        ...scheduleOverview.value,
        [taskId]: {
          ...entry,
          triggers: entry.triggers.map((trigger) =>
            trigger.trigger_id === triggerId
              ? { ...trigger, next_fire_at: result.next_fire_at }
              : trigger
          ),
        },
      };
    }
    return result.next_fire_at;
  }

  async function initOverviewListener() {
    if (overviewListenerStarted) return;
    overviewListenerStarted = true;
    try {
      unlistenOverview = await onExecutionFinished(() => {
        void loadOverview();
      });
    } catch {
      overviewListenerStarted = false;
      unlistenOverview = null;
    }
  }

  function cleanupOverviewListener() {
    if (typeof unlistenOverview === 'function') unlistenOverview();
    unlistenOverview = null;
    overviewListenerStarted = false;
  }

  async function saveTask(task: Task) {
    const saved = await apiSaveTask(task);
    const idx = tasks.value.findIndex((t) => t.id === saved.id);
    if (idx >= 0) {
      tasks.value[idx] = saved;
    } else {
      tasks.value.unshift(saved);
    }
    return saved;
  }

  async function deleteTask(id: TaskId) {
    await apiDeleteTask(id);
    tasks.value = tasks.value.filter((t) => t.id !== id);
  }

  async function triggerTask(id: TaskId) {
    return await apiTriggerTask(id);
  }

  return {
    tasks,
    searchQuery,
    loading,
    scheduleOverview,
    filteredTasks,
    loadTasks,
    loadOverview,
    rerollTrigger,
    initOverviewListener,
    cleanupOverviewListener,
    saveTask,
    deleteTask,
    triggerTask,
  };
});
