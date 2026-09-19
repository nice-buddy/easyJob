import { describe, it, expect } from 'vitest';
import {
  describeTriggerShort,
  formatDateTimeShort,
  formatDateTimeFull,
  formatRelativeTime,
  formatDateTimeTitle,
  type TriggerKind,
} from '../src/types/task';

function localIso(y: number, mo: number, d: number, h: number, mi: number): string {
  return new Date(y, mo - 1, d, h, mi, 0).toISOString();
}

describe('describeTriggerShort', () => {
  it('covers every row of the design table', () => {
    expect(describeTriggerShort({ Daily: { time: '09:00:00', timezone: 'UTC' } })).toBe('每天 09:00');
    expect(
      describeTriggerShort({
        Weekly: { days_of_week: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'], time: '09:00:00', timezone: 'UTC' },
      })
    ).toBe('每周一至周五 09:00');
    expect(describeTriggerShort({ Interval: { interval_secs: 300 } })).toBe('每 5 分钟');
    expect(describeTriggerShort({ Cron: { expression: '0 9 * * *', timezone: 'UTC' } })).toBe('每天 09:00');
    expect(describeTriggerShort({ Cron: { expression: '*/5 * * * *', timezone: 'UTC' } })).toBe('每 5 分钟');
    expect(describeTriggerShort({ Cron: { expression: '0 9 * * 1-5', timezone: 'UTC' } })).toBe('工作日 09:00');
    expect(describeTriggerShort({ Cron: { expression: '0 9 1 * *', timezone: 'UTC' } })).toBe('0 9 1 * *');
    expect(describeTriggerShort({ Cron: { expression: 'L * * * *', timezone: 'UTC' } })).toBe('L * * * *');
    expect(
      describeTriggerShort({ Fuzzy: { period: 'Daily', window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } })
    ).toBe('每天 09:00-10:00 之间随机');
    expect(
      describeTriggerShort({
        Fuzzy: {
          period: { Weekly: { days_of_week: ['Mon', 'Fri'] } },
          window_start: '09:00:00',
          window_end: '10:00:00',
          timezone: 'UTC',
        },
      })
    ).toBe('每周一、周五 09:00-10:00 之间随机');
    expect(describeTriggerShort({ Network: { events: ['Connect'], network_name: null } })).toBe('连接网络时');
    expect(describeTriggerShort('AgentStarted')).toBe('启动时');

    // Once 用本机本地时区渲染；用当前自然年构造以命中 MM-DD HH:mm 分支
    const thisYear = new Date().getFullYear();
    const onceIso = new Date(thisYear, 8, 19, 14, 30, 0).toISOString();
    expect(describeTriggerShort({ Once: { fire_at: onceIso } })).toBe('单次 09-19 14:30');
  });

  it('covers interval unit fallbacks and weekday separators', () => {
    expect(describeTriggerShort({ Interval: { interval_secs: 86400 } })).toBe('每 1 天');
    expect(describeTriggerShort({ Interval: { interval_secs: 7200 } })).toBe('每 2 小时');
    expect(describeTriggerShort({ Interval: { interval_secs: 45 } })).toBe('每 45 秒');
    // Weekly 非连续日用 、连接（规则明细）
    expect(
      describeTriggerShort({
        Weekly: { days_of_week: ['Mon', 'Wed'], time: '18:00:00', timezone: 'UTC' },
      })
    ).toBe('每周一、周三 18:00');
    // 数字 cron 周几同样可识别
    expect(describeTriggerShort({ Cron: { expression: '0 9 * * 1,3', timezone: 'UTC' } })).toBe('每周一、周三 09:00');
  });

  it('does not throw and never leaks undefined for malformed input', () => {
    const malformed: unknown[] = [
      null,
      undefined,
      {},
      { Cron: {} },
      { Cron: { expression: 123 } },
      { Once: {} },
      { Daily: {} },
      { Weekly: {} },
      { Fuzzy: {} },
      { Fuzzy: { period: 'Bogus', window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } },
      { Network: { events: ['Bogus'], network_name: null } },
      { Network: {} },
      { Interval: { interval_secs: 0 } },
      { Interval: {} },
      'Whatever',
    ];
    for (const kind of malformed) {
      expect(() => describeTriggerShort(kind as TriggerKind)).not.toThrow();
      const text = describeTriggerShort(kind as TriggerKind);
      expect(typeof text).toBe('string');
      expect(text.length).toBeGreaterThan(0);
      expect(text).not.toContain('undefined');
      expect(text).not.toContain('[object');
    }
  });
});

describe('time formatting helpers', () => {
  const now = new Date(2026, 8, 19, 12, 0, 0); // 本地 2026-09-19 12:00

  it('uses MM-DD HH:mm within the same calendar year', () => {
    expect(formatDateTimeShort(localIso(2026, 9, 19, 14, 30), now)).toBe('09-19 14:30');
  });

  it('uses YYYY-MM-DD HH:mm across calendar years', () => {
    expect(formatDateTimeShort(localIso(2027, 1, 2, 3, 4), now)).toBe('2027-01-02 03:04');
  });

  it('falls back to an em dash for missing or invalid timestamps', () => {
    expect(formatDateTimeShort(null, now)).toBe('—');
    expect(formatDateTimeShort('not-a-date', now)).toBe('—');
  });

  it('formats full local time and relative time for the title attribute', () => {
    const future = localIso(2026, 9, 19, 15, 0);
    expect(formatDateTimeFull(future)).toBe('2026-09-19 15:00:00');
    expect(formatRelativeTime(future, now)).toBe('3 小时后');
    expect(formatRelativeTime(localIso(2026, 9, 19, 11, 0), now)).toBe('1 小时前');
    expect(formatRelativeTime(localIso(2026, 9, 19, 12, 0), now)).toBe('刚刚');
    expect(formatDateTimeTitle(future, now)).toBe('2026-09-19 15:00:00（3 小时后）');
  });
});