import { invoke } from '@tauri-apps/api/core';
import type { Task, TaskId } from '../types/task';
import type { Execution, ExecutionId } from '../types/execution';
import type { AgentStatus } from '../types/agent';

export async function getAgentStatus(): Promise<AgentStatus> {
  return await invoke<AgentStatus>('get_agent_status');
}

export async function listTasks(): Promise<Task[]> {
  return await invoke<Task[]>('list_tasks');
}

export async function getTask(id: TaskId): Promise<Task> {
  return await invoke<Task>('get_task', { id });
}

export async function saveTask(task: Task): Promise<Task> {
  return await invoke<Task>('save_task', { task });
}

export async function deleteTask(id: TaskId): Promise<boolean> {
  return await invoke<boolean>('delete_task', { id });
}

export async function triggerTask(id: TaskId): Promise<boolean> {
  return await invoke<boolean>('trigger_task', { id });
}

export async function listExecutions(limit?: number): Promise<Execution[]> {
  return await invoke<Execution[]>('list_executions', { limit });
}

export async function getExecution(id: ExecutionId): Promise<Execution> {
  return await invoke<Execution>('get_execution', { id });
}

export async function cancelExecution(id: ExecutionId): Promise<boolean> {
  return await invoke<boolean>('cancel_execution', { id });
}

export interface ExecutionOutputRecord {
  stream: string;
  content: string;
  created_at: string;
}

export async function getExecutionOutput(id: ExecutionId): Promise<ExecutionOutputRecord[]> {
  return await invoke<ExecutionOutputRecord[]>('get_execution_output', { id });
}

