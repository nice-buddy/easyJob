import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-autostart', () => ({
  enable: vi.fn(),
  disable: vi.fn(),
  isEnabled: vi.fn(),
}));

import { deriveMode } from '../src/services/startupMode';

describe('deriveMode', () => {
  it('service running wins boot', () => {
    expect(deriveMode({ running: true }, true)).toBe('boot');
  });
  it('login item only maps login', () => {
    expect(deriveMode({ running: false }, true)).toBe('login');
  });
  it('neither maps disabled', () => {
    expect(deriveMode({ running: false }, false)).toBe('disabled');
  });
});
