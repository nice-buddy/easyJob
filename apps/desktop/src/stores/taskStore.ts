import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import {
  listTasks,
  saveTask as apiSaveTask,
  deleteTask as apiDeleteTask,
  triggerTask as apiTriggerTask,
} from '../services/tauri';
import type { Task, TaskId } from '../types/task';

export const useTaskStore = defineStore('tasks', () => {
  const tasks = ref<Task[]>([]);
  const searchQuery = ref('');
  const loading = ref(false);

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
    filteredTasks,
    loadTasks,
    saveTask,
    deleteTask,
    triggerTask,
  };
});
