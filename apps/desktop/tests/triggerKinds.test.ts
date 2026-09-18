import { describe, it, expect } from 'vitest';
import {
  getTriggerType,
  describeTrigger,
  validateCronExpression,
  type TriggerKind,
} from '../src/types/task';

// 以下字符串为后端 serde 实测输出的真实线格式
const FUZZY_WIRE =
  '{"Fuzzy":{"period":"Daily","window_start":"09:00:00","window_end":"10:00:00","timezone":"UTC"}}';
const FUZZY_WEEKLY_WIRE =
  '{"Fuzzy":{"period":{"Weekly":{"days_of_week":["Mon","Fri"]}},"window_start":"09:00:00","window_end":"10:00:00","timezone":"UTC"}}';
const NETWORK_WIRE = '{"Network":{"events":["Connect","Online"],"network_name":null}}';
const CRON_WIRE = '{"Cron":{"expression":"*/5 * * * *","timezone":"Asia/Shanghai"}}';

describe('New trigger kinds wire format', () => {
  it('recognizes Cron kind', () => {
    const kind = JSON.parse(CRON_WIRE) as TriggerKind;
    expect(getTriggerType(kind)).toBe('Cron');
  });

  it('recognizes Fuzzy kind and serde shape', () => {
    const kind = JSON.parse(FUZZY_WEEKLY_WIRE) as TriggerKind;
    expect(getTriggerType(kind)).toBe('Fuzzy');
    expect((kind as any).Fuzzy.period.Weekly.days_of_week).toEqual(['Mon', 'Fri']);
  });

  it('Fuzzy unit periods serialize as strings (serde conformance)', () => {
    const kind = JSON.parse(FUZZY_WIRE) as TriggerKind;
    expect(getTriggerType(kind)).toBe('Fuzzy');
    expect((kind as any).Fuzzy.period).toBe('Daily');
  });

  it('recognizes Network kind with optional name', () => {
    const withName: TriggerKind = { Network: { events: ['Connect', 'Online'], network_name: 'MyHome' } };
    const noName = JSON.parse(NETWORK_WIRE) as TriggerKind;
    expect(getTriggerType(withName)).toBe('Network');
    expect(getTriggerType(noName)).toBe('Network');
    expect((noName as any).Network.network_name).toBeNull();
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

  it('rejects reversed ranges and unknown aliases like croner', () => {
    // 反向区间：croner 报错
    expect(validateCronExpression('5-1 * * * *')).toBe(false);
    expect(validateCronExpression('50-40 * * * *')).toBe(false);
    expect(validateCronExpression('0 0 * 12-1 *')).toBe(false);
    expect(validateCronExpression('0 0 * * 5-1')).toBe(false);
    expect(validateCronExpression('0 0 * * FRI-MON')).toBe(false);
    // 未知别名：croner 只替换已知三字母别名
    expect(validateCronExpression('FOO * * * *')).toBe(false);
    expect(validateCronExpression('* * * * FOO')).toBe(false);
    // 合法回绕区间仍需接受（sun 作为范围末端归一化为 7）
    expect(validateCronExpression('0 0 * * SUN-MON')).toBe(true);
    expect(validateCronExpression('0 0 * * FRI-SUN')).toBe(true);
  });
});

describe('describeTrigger', () => {
  it('describes new kinds in Chinese', () => {
    expect(describeTrigger({ Cron: { expression: '*/5 * * * *', timezone: 'Asia/Shanghai' } })).toBe('Cron: */5 * * * *');
    expect(describeTrigger({ Fuzzy: { period: 'Daily', window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } })).toBe('模糊时间：每天 09:00:00-10:00:00 随机');
    expect(describeTrigger({ Fuzzy: { period: 'Weekdays', window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } })).toBe('模糊时间：工作日 09:00:00-10:00:00 随机');
    expect(describeTrigger({ Fuzzy: { period: 'Weekends', window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } })).toBe('模糊时间：周末 09:00:00-10:00:00 随机');
    expect(describeTrigger({ Fuzzy: { period: { Weekly: { days_of_week: ['Mon', 'Fri'] } }, window_start: '09:00:00', window_end: '10:00:00', timezone: 'UTC' } })).toBe('模糊时间：每周 Mon,Fri 09:00:00-10:00:00 随机');
    expect(describeTrigger({ Network: { events: ['Connect'], network_name: 'MyHome' } })).toBe('网络变动：连接时 (MyHome)');
    expect(describeTrigger({ Network: { events: ['Connect', 'Disconnect'], network_name: null } })).toBe('网络变动：连接时/断开时（任意网络）');
    expect(describeTrigger({ Network: { events: ['Online'], network_name: null } })).toBe('网络变动：可上网时（任意网络）');
  });
});