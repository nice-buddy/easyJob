import { invoke } from '@tauri-apps/api/core';
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';

export type StartupMode = 'boot' | 'login' | 'disabled';

export interface ServiceStatus {
  installed: boolean;
  running: boolean;
  data_dir: string | null;
}

/** 互斥推导：服务运行即开机模式；否则看登录项；都没有即禁用。 */
export function deriveMode(svc: { running: boolean }, login: boolean): StartupMode {
  if (svc.running) return 'boot';
  if (login) return 'login';
  return 'disabled';
}

export async function getServiceStatus(): Promise<ServiceStatus> {
  try {
    return await invoke<ServiceStatus>('service_status');
  } catch (e: any) {
    throw new Error(`查询系统服务状态失败: ${e?.message || e}`);
  }
}

export async function getStartupMode(): Promise<StartupMode> {
  const [svc, login] = await Promise.all([getServiceStatus(), isEnabled()]);
  return deriveMode(svc, login);
}

async function waitServiceRunning(timeoutMs = 15000): Promise<ServiceStatus> {
  const start = Date.now();
  for (;;) {
    const st = await getServiceStatus();
    if (st.running) return st;
    if (Date.now() - start > timeoutMs) {
      throw new Error('系统服务安装成功，但未能在 15 秒内进入运行状态，请检查系统服务管理器');
    }
    await new Promise((r) => setTimeout(r, 500));
  }
}

export async function setStartupMode(mode: StartupMode): Promise<StartupMode> {
  if (mode === 'boot') {
    try {
      await invoke('service_install');
    } catch (e: any) {
      throw new Error(`${e?.message || e}`);
    }
    try {
      await waitServiceRunning();
      await disable();
      return 'boot';
    } catch (e: any) {
      try {
        await invoke('service_uninstall');
      } catch {
        /* 回滚尽力而为 */
      }
      throw new Error(`${e?.message || e}`);
    }
  }
  if (mode === 'login') {
    try {
      await invoke('service_uninstall');
    } catch (e: any) {
      throw new Error(`${e?.message || e}`);
    }
    await enable();
    return 'login';
  }
  try {
    await invoke('service_uninstall');
  } catch (e: any) {
    throw new Error(`${e?.message || e}`);
  }
  await disable();
  return 'disabled';
}

/** 旧版一次性迁移：本地键不存在时，沿用“默认开启登录自启”语义。 */
const LEGACY_INIT_KEY = 'easyjob_autostart_initialized';

export async function migrateLegacyDefault(): Promise<StartupMode> {
  if (typeof localStorage !== 'undefined' && localStorage.getItem(LEGACY_INIT_KEY)) {
    return getStartupMode();
  }
  const current = await getStartupMode();
  if (current === 'disabled') {
    await setStartupMode('login');
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(LEGACY_INIT_KEY, 'true');
    }
    return 'login';
  }
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(LEGACY_INIT_KEY, 'true');
  }
  return current;
}
