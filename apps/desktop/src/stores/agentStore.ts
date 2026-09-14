import { defineStore } from 'pinia';
import { ref } from 'vue';
import { getAgentStatus } from '../services/tauri';
import type { AgentStatus } from '../types/agent';

export const useAgentStore = defineStore('agent', () => {
  const status = ref<AgentStatus | null>(null);
  const isConnected = ref(false);
  const lastError = ref<string | null>(null);

  async function fetchStatus() {
    try {
      status.value = await getAgentStatus();
      isConnected.value = true;
      lastError.value = null;
    } catch (e: any) {
      isConnected.value = false;
      lastError.value = String(e);
    }
  }

  return { status, isConnected, lastError, fetchStatus };
});
