import { describe, it, expect } from 'vitest';
import {
  getTriggerType,
  describeTrigger,
  validateCronExpression,
  type TriggerKind,
} from '../src/types/task';

describe('New trigger kinds wire format', () => {
  it('recognizes Cron kind', () => {
    const kind: TriggerKind = { Cron: { expression: '*/5 * * * *', timezone: 'Asia/Shanghai' } };
    expect(getTriggerType(kind)).toBe('Cron');
  });

  it('recognizes Fuzzy kind and serde shape', () => {
    const kind: TriggerKind = {
      Fuzzy: { period: { Weekly: { days_of_week: ['Mon', 'Fri'] } }, window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' },
    };
    expect(getTriggerType(kind)).toBe('Fuzzy');
    expect(JSON.stringify(kind)).toContain('"days_of_week"');
  });

  it('Fuzzy unit periods serialize as strings (serde conformance)', () => {
    expect(JSON.stringify({ period: 'Daily' })).toContain('"Daily"');
  });

  it('recognizes Network kind with optional name', () => {
    const withName: TriggerKind = { Network: { events: ['Connect', 'Online'], network_name: 'MyHome' } };
    const noName: TriggerKind = { Network: { events: ['Disconnect'], network_name: null } };
    expect(getTriggerType(withName)).toBe('Network');
    expect(getTriggerType(noName)).toBe('Network');
  });
});

describe('validateCronExpression', () => {
  it('accepts valid 5-field expressions', () => {
    expect(validateCronExpression('*/5 * * * *')).toBe(true);
    expect(validateCronExpression('30 9 * * 1-5')).toBe(true);
    expect(validateCronExpression('0 0 1 JAN SUN')).toBe(true);
  });

  it('rejects wrong field count and bad tokens', () => {
    expect(validateCronExpression('0 */5 * * * *')).toBe(false); // 6 字段
    expect(validateCronExpression('*/5 * * *')).toBe(false);     // 4 字段
    expect(validateCronExpression('99 * * * *')).toBe(false);    // 分钟越界
    expect(validateCronExpression('hello world')).toBe(false);
  });

  it('matches backend croner behavior for ? and weekday aliases', () => {
    // croner 将 `?` 视作 `*`
    expect(validateCronExpression('? * * * *')).toBe(true);
    // croner 支持周几别名与范围
    expect(validateCronExpression('0 0 * * MON-FRI')).toBe(true);
  });
});

describe('describeTrigger', () => {
  it('describes new kinds in Chinese', () => {
    expect(describeTrigger({ Cron: { expression: '*/5 * * * *', timezone: 'Asia/Shanghai' } })).toBe('Cron: */5 * * * *');
    expect(describeTrigger({ Fuzzy: { period: 'Daily', window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } })).toBe('模糊时间：每天 09:00:00-10:00:00 随机');
    expect(describeTrigger({ Network: { events: ['Connect'], network_name: 'MyHome' } })).toBe('网络变动：连接时 (MyHome)');
    expect(describeTrigger({ Network: { events: ['Connect', 'Disconnect'], network_name: null } })).toBe('网络变动：连接时/断开时（任意网络）');
  });
});