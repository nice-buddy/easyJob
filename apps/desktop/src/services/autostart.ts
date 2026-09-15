import { enable, isEnabled, disable } from '@tauri-apps/plugin-autostart';

const AUTOSTART_INITIALIZED_KEY = 'easyjob_autostart_initialized';

export async function isAutostartEnabled(): Promise<boolean> {
  try {
    return await isEnabled();
  } catch (e) {
    console.warn('Failed to check autostart status:', e);
    return false;
  }
}

export async function setAutostart(shouldEnable: boolean): Promise<boolean> {
  try {
    if (shouldEnable) {
      await enable();
    } else {
      await disable();
    }
    return true;
  } catch (e) {
    console.error('Failed to update autostart status:', e);
    throw e;
  }
}

export async function initAutostartDefault(): Promise<boolean> {
  try {
    const isInit = typeof localStorage !== 'undefined' ? localStorage.getItem(AUTOSTART_INITIALIZED_KEY) : null;
    if (!isInit) {
      await enable();
      if (typeof localStorage !== 'undefined') {
        localStorage.setItem(AUTOSTART_INITIALIZED_KEY, 'true');
      }
      return true;
    }
    return await isEnabled();
  } catch (e) {
    console.warn('Autostart default initialization bypassed:', e);
    return false;
  }
}

