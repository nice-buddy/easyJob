use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use std::str::FromStr;

pub trait TriggerEvaluator: Send + Sync {
    fn next_occurrence(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>>;
}

impl TriggerEvaluator for TriggerKind {
    fn next_occurrence(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        evaluate_next_occurrence(self, after)
    }
}

impl TriggerEvaluator for Trigger {
    fn next_occurrence(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if !self.enabled {
            return None;
        }
        evaluate_next_occurrence(&self.kind, after)
    }
}

/// Resolves a candidate date and time in the given timezone, returning a valid UTC DateTime strictly after `after`.
/// If the local time falls in a DST gap (spring forward), it shifts forward to the next valid time.
fn resolve_candidate(
    tz: &Tz,
    date: NaiveDate,
    time: NaiveTime,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match tz.from_local_datetime(&date.and_time(time)) {
        chrono::LocalResult::Single(dt) => {
            let utc = dt.with_timezone(&Utc);
            if utc > after {
                Some(utc)
            } else {
                None
            }
        }
        chrono::LocalResult::Ambiguous(earliest, latest) => {
            let e_utc = earliest.with_timezone(&Utc);
            if e_utc > after {
                return Some(e_utc);
            }
            let l_utc = latest.with_timezone(&Utc);
            if l_utc > after {
                return Some(l_utc);
            }
            None
        }
        chrono::LocalResult::None => {
            // DST gap (e.g. spring forward): shift forward minute by minute up to 2 hours
            let mut shifted = date.and_time(time);
            for _ in 0..8 {
                shifted += Duration::minutes(15);
                match tz.from_local_datetime(&shifted) {
                    chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => {
                        let utc = dt.with_timezone(&Utc);
                        if utc > after {
                            return Some(utc);
                        }
                        break;
                    }
                    chrono::LocalResult::None => continue,
                }
            }
            None
        }
    }
}

pub fn evaluate_next_occurrence(kind: &TriggerKind, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match kind {
        TriggerKind::Once { fire_at } => {
            if *fire_at > after {
                Some(*fire_at)
            } else {
                None
            }
        }
        TriggerKind::Interval {
            interval_secs,
            start_at,
        } => {
            if *interval_secs == 0 {
                return None;
            }
            let interval = *interval_secs as i64;
            let base = start_at.unwrap_or(after);
            if after < base {
                return Some(base);
            }
            let elapsed = after - base;
            let count = (elapsed.num_seconds() / interval) + 1;
            Some(base + Duration::seconds(count * interval))
        }
        TriggerKind::Daily { time, timezone } => {
            let tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
            let local_after = after.with_timezone(&tz);
            let mut candidate_date = local_after.date_naive();

            for _ in 0..4 {
                if let Some(candidate_utc) = resolve_candidate(&tz, candidate_date, *time, after) {
                    return Some(candidate_utc);
                }
                candidate_date = candidate_date.succ_opt()?;
            }
            None
        }
        TriggerKind::Weekly {
            days_of_week,
            time,
            timezone,
        } => {
            if days_of_week.is_empty() {
                return None;
            }
            let tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
            let local_after = after.with_timezone(&tz);
            let mut candidate_date = local_after.date_naive();

            for _ in 0..14 {
                if days_of_week.contains(&candidate_date.weekday()) {
                    if let Some(candidate_utc) =
                        resolve_candidate(&tz, candidate_date, *time, after)
                    {
                        return Some(candidate_utc);
                    }
                }
                candidate_date = candidate_date.succ_opt()?;
            }
            None
        }
        TriggerKind::Cron {
            expression,
            timezone,
        } => {
            let cron = parse_standard_cron(expression)?;
            let tz: Tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
            cron_next_occurrence(&cron, &tz, after)
        }
        TriggerKind::Network { .. } => None, // Event-driven (network change), not periodic
        TriggerKind::Fuzzy {
            period,
            window_start,
            window_end,
            timezone,
        } => {
            if window_end <= window_start {
                return None;
            }
            let tz: Tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
            let local_after = after.with_timezone(&tz);
            let mut candidate_date = local_after.date_naive();

            // 上限 14 天。Weekly 只含单日且当日窗口已开始时，最坏需跨 7 天才命中；
            // 这里留一周余量，避免边界收得过紧导致静默返回 None（触发器会永久停止调度）。
            for _ in 0..14 {
                // 窗口已开始（含已结束）即跳过该日：保证每个匹配窗口至多触发一次。
                // 否则 fire 之后 scheduler 以 now 重新求值，会在同一天剩余窗口内反复重摇。
                let window_not_started =
                    candidate_date > local_after.date_naive() || *window_start > local_after.time();
                if period.matches_weekday(candidate_date.weekday()) && window_not_started {
                    let picked = pick_random_time_in_window(window_start, window_end);
                    if let Some(utc_dt) = resolve_candidate(&tz, candidate_date, picked, after) {
                        return Some(utc_dt);
                    }
                }
                candidate_date = candidate_date.succ_opt()?;
            }
            None
        }
        TriggerKind::AgentStarted => None, // Handled upon agent startup event, not periodic
    }
}

/// 校验 cron 表达式是否为受支持的标准 5 字段合法表达式，**且确实存在下一次触发时刻**
/// （agent 侧 warn 日志用）。
///
/// 像 `0 0 30 2 *` 这类语法合法但永不发生的表达式也必须判为非法，否则触发器会静默地
/// 永不触发且没有任何日志。
///
/// NOTE: 最坏情况约 236ms（`0 0 30 2 *` 这类需扫到 croner 的 YEAR_UPPER_LIMIT=5000）。
/// 它只在任务加载/保存时按 Cron 触发器调用一次，可接受。
pub fn validate_cron_expression(expression: &str) -> bool {
    let Some(cron) = parse_standard_cron(expression) else {
        return false;
    };
    cron_next_occurrence(&cron, &chrono_tz::UTC, Utc::now()).is_some()
}

const CRON_MONTH_NAMES: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];
const CRON_WEEKDAY_NAMES: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

/// 词法白名单：标准 5 字段 cron 的单个字段只允许数字、`*`、`?`、`,`、`-`、`/`，
/// 以及月/周字段的三字母别名（JAN..DEC / SUN..SAT）。
///
/// 必须拒绝 Quartz 扩展（`L` / `#` / `W`）等非标准记号：croner 的 parse 会接受它们，
/// 但 `find_next_occurrence` 可能一路搜索到 YEAR_UPPER_LIMIT(5000) 才放弃
/// （实测 `L * * * *` 单次求值 35 秒），而该调用在调度循环里是同步的，会阻塞全部任务。
fn is_standard_cron_field(field: &str, is_month: bool, is_weekday: bool) -> bool {
    let bytes = field.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_digit() || matches!(c, '*' | '?' | ',' | '-' | '/') {
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() {
            if i + 3 > bytes.len() {
                return false;
            }
            let name = &field[i..i + 3];
            if !name.bytes().all(|b| b.is_ascii_alphabetic()) {
                return false;
            }
            let upper = name.to_ascii_uppercase();
            let known = (is_month && CRON_MONTH_NAMES.contains(&upper.as_str()))
                || (is_weekday && CRON_WEEKDAY_NAMES.contains(&upper.as_str()));
            if !known {
                return false;
            }
            i += 3;
            continue;
        }
        return false;
    }
    true
}

/// 解析标准 5 字段 cron：字段数 + 字段词法 + croner 解析三重校验。
/// 字段下标：0=分 1=时 2=日 3=月 4=周。
fn parse_standard_cron(expression: &str) -> Option<croner::Cron> {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() != 5 {
        return None;
    }
    for (idx, field) in fields.iter().enumerate() {
        if !is_standard_cron_field(field, idx == 3, idx == 4) {
            return None;
        }
    }
    // NOTE: croner 的 `FromStr` 不校验表达式（恒返回 `Ok`），真正的解析/校验发生在 `Cron::parse`
    croner::Cron::from_str(expression)
        .and_then(|mut cron| cron.parse())
        .ok()
}

/// DST 回拨的重锚次数上限。
///
/// 回拨时同一段本地时间会重复出现，重复区间最长可达 2 小时（极少数时区）。每轮重锚只前进
/// 一次匹配，因此上限必须覆盖「重复区间内最密的匹配」——每分钟匹配时是 120 次，这里取 240
/// 留余量。上限若过小（例如 3），像 `*/15 1 * * *` 这种在重复小时里有 4 个匹配点的表达式会
/// 耗尽重试并返回 `None`，而 scheduler 对 `None` 不做重排，触发器会**永久停止调度**。
///
/// 放大上限不会带来额外开销：表达式永不匹配时第一次 `find_next_occurrence` 就返回 `Err`
/// 并由 `?` 短路；只有当候选确实落在 `after` 之前时才会继续下一轮，而每轮都是微秒级纯计算。
const CRON_DST_RETRY_LIMIT: usize = 240;

/// 用 croner 求下一次触发，并保证结果**在绝对时刻上**严格晚于 `after`。
///
/// croner 的 `inclusive = false` 只保证「本地时间」严格晚于锚点，不保证「绝对时刻」晚于
/// `after`：DST 回拨日同一个本地时刻会出现两次，croner 固定返回较早的那次，可能落在
/// `after` 之前。若直接返回，scheduler 会判定 next_fire_at <= now 并立即重排，形成忙循环
/// 且反复触发任务。这里以候选的绝对时刻重锚再搜，逐次跨过重复区间内的匹配点。
fn cron_next_occurrence(
    cron: &croner::Cron,
    tz: &Tz,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let mut anchor = after.with_timezone(tz);
    for _ in 0..CRON_DST_RETRY_LIMIT {
        let candidate = cron.find_next_occurrence(&anchor, false).ok()?;
        let candidate_utc = candidate.with_timezone(&Utc);
        if candidate_utc > after {
            return Some(candidate_utc);
        }
        anchor = candidate_utc.with_timezone(tz);
    }
    None
}

fn pick_random_time_in_window(start: &NaiveTime, end: &NaiveTime) -> NaiveTime {
    use rand::Rng;
    let span = (*end - *start).num_seconds().max(1) as u64;
    let offset = rand::thread_rng().gen_range(0..span);
    *start + chrono::Duration::seconds(offset as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyjob_domain::trigger::{FuzzyPeriod, NetworkEventKind};

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn fuzzy_kind(period: FuzzyPeriod) -> TriggerKind {
        TriggerKind::Fuzzy {
            period,
            window_start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            window_end: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            timezone: "UTC".into(),
        }
    }

    #[test]
    fn cron_daily_0930_shanghai() {
        // after = 2026-06-01 08:00 +08 → 下一次 09:30 +08 = 01:30Z
        let kind = TriggerKind::Cron {
            expression: "30 9 * * *".into(),
            timezone: "Asia/Shanghai".into(),
        };
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z")).unwrap();
        assert_eq!(next, utc("2026-06-01T01:30:00Z"));
    }

    #[test]
    fn cron_every_5_min_respects_after() {
        let kind = TriggerKind::Cron {
            expression: "*/5 * * * *".into(),
            timezone: "UTC".into(),
        };
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T10:02:00Z")).unwrap();
        assert_eq!(next, utc("2026-06-01T10:05:00Z"));
    }

    #[test]
    fn cron_invalid_expression_returns_none() {
        // 5 字段但内容非法：必须穿过 5 字段前置检查，走到 croner 的 parse 错误路径
        for expression in ["99 * * * *", "*/0 * * * *"] {
            let kind = TriggerKind::Cron {
                expression: expression.into(),
                timezone: "UTC".into(),
            };
            assert_eq!(
                evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z")),
                None,
                "expression {expression:?} should be rejected by croner"
            );
        }
    }

    #[test]
    fn cron_strictly_after_boundary() {
        // after 恰好落在匹配点（10:05）→ 必须严格晚于 after，取 10:10
        let kind = TriggerKind::Cron {
            expression: "*/5 * * * *".into(),
            timezone: "UTC".into(),
        };
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T10:05:00Z")).unwrap();
        assert_eq!(next, utc("2026-06-01T10:10:00Z"));
    }

    #[test]
    fn cron_rejects_non_5_field_expressions() {
        let kind = TriggerKind::Cron {
            expression: "0 */5 * * * *".into(), // 6 字段（含秒），规格明确不支持
            timezone: "UTC".into(),
        };
        assert_eq!(
            evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z")),
            None
        );
    }

    #[test]
    fn validate_cron_expression_accepts_valid_5_field() {
        assert!(validate_cron_expression("30 9 * * *"));
        assert!(validate_cron_expression("*/5 * * * *"));
        assert!(validate_cron_expression("0 0 1 1 0"));
        assert!(validate_cron_expression("0 0 * * MON-FRI"));
    }

    #[test]
    fn cron_fall_back_dst_returns_future() {
        // DST 回拨日同一本地时刻出现两次，croner 固定返回较早那次；必须保证绝对时刻 next > after
        let cases = [
            ("America/New_York", "30 1 * * *", "2026-11-01T04:00:00Z", 16),
            ("Europe/London", "30 1 * * *", "2026-10-25T00:00:00Z", 12),
        ];
        for (tz, expr, start, steps) in cases {
            let kind = TriggerKind::Cron {
                expression: expr.into(),
                timezone: tz.into(),
            };
            let mut after = utc(start);
            for _ in 0..steps {
                let next = evaluate_next_occurrence(&kind, after)
                    .unwrap_or_else(|| panic!("{tz} {expr} after {after} returned None"));
                assert!(
                    next > after,
                    "{tz} {expr}: next {next} must be strictly after {after}"
                );
                after += Duration::minutes(15);
            }
        }
    }

    #[test]
    fn cron_fall_back_dense_schedule_returns_future() {
        // 重复小时内有多个匹配点（每分钟 / 每 15 分钟）时，重锚次数必须足够：
        // 上限过小会耗尽重试并返回 None，而 scheduler 对 None 不重排 → 触发器永久停摆
        let cases = [
            ("America/New_York", "*/15 1 * * *", "2026-11-01T04:00:00Z"),
            ("America/New_York", "*/1 1 * * *", "2026-11-01T04:00:00Z"),
            ("Europe/London", "*/5 1 * * *", "2026-10-25T00:00:00Z"),
        ];
        for (tz, expr, start) in cases {
            let kind = TriggerKind::Cron {
                expression: expr.into(),
                timezone: tz.into(),
            };
            // 逐分钟扫过整个回拨区间，每个采样点都必须得到「未来」的下一跳
            let mut after = utc(start);
            for _ in 0..240 {
                let next = match evaluate_next_occurrence(&kind, after) {
                    Some(next) => next,
                    None => panic!("{tz} {expr} after {after} returned None（重锚次数不足）"),
                };
                assert!(
                    next > after,
                    "{tz} {expr}: next {next} must be strictly after {after}"
                );
                after += Duration::minutes(1);
            }
        }
    }

    #[test]
    fn cron_quartz_extensions_are_rejected_fast() {
        // `L` 是 Quartz 扩展：croner 会接受并一路扫到 5000 年（实测 35s），
        // 同步阻塞调度循环；词法白名单必须让它立刻返回 None
        let kind = TriggerKind::Cron {
            expression: "L * * * *".into(),
            timezone: "UTC".into(),
        };
        let start = std::time::Instant::now();
        let result = evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z"));
        let elapsed = start.elapsed();
        assert_eq!(result, None);
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "rejection took {elapsed:?}, expected < 1s"
        );
    }

    #[test]
    fn cron_impossible_date_returns_none() {
        // 语法合法但永不发生（2 月 30 日 / 4 月 31 日）→ 必须返回 None
        for expression in ["0 0 30 2 *", "0 0 31 4 *"] {
            let kind = TriggerKind::Cron {
                expression: expression.into(),
                timezone: "UTC".into(),
            };
            assert_eq!(
                evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z")),
                None,
                "expression {expression:?} should have no occurrence"
            );
        }
    }

    #[test]
    fn validate_cron_expression_rejects_illegal_values() {
        // 5 字段但取值非法：必须真正经过 croner 的 parse 校验（防「FromStr 恒 Ok」回归）
        assert!(!validate_cron_expression("99 99 99 99 99"));
        assert!(!validate_cron_expression("99 * * * *"));
        assert!(!validate_cron_expression("*/0 * * * *"));
    }

    #[test]
    fn validate_cron_expression_rejects_wrong_field_count_and_aliases() {
        assert!(!validate_cron_expression("* * * *")); // 4 字段
        assert!(!validate_cron_expression("0 */5 * * * *")); // 6 字段
        assert!(!validate_cron_expression("@daily")); // 别名，规格不支持
        assert!(!validate_cron_expression("L * * * *")); // Quartz 扩展
        assert!(!validate_cron_expression("0 0 30 2 *")); // 语法合法但永不发生
        assert!(!validate_cron_expression("1/2/3 * * * *")); // croner 报 stepped range 错误
    }

    #[test]
    fn network_kind_is_event_driven_returns_none() {
        let kind = TriggerKind::Network {
            events: vec![NetworkEventKind::Connect],
            network_name: None,
        };
        assert_eq!(
            evaluate_next_occurrence(&kind, utc("2026-06-01T00:00:00Z")),
            None
        );
    }

    #[test]
    fn fuzzy_daily_returns_time_within_window() {
        // after = 2026-06-01 08:30Z → 今天窗口 [09:00,10:00) 内随机
        let kind = fuzzy_kind(FuzzyPeriod::Daily);
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T08:30:00Z")).unwrap();
        let t = next.time();
        assert!(
            t >= NaiveTime::from_hms_opt(9, 0, 0).unwrap()
                && t < NaiveTime::from_hms_opt(10, 0, 0).unwrap()
        );
        assert_eq!(next.date_naive(), utc("2026-06-01T08:30:00Z").date_naive());
    }

    #[test]
    fn fuzzy_mid_window_skips_to_next_matching_day() {
        // after 已落在窗口内 → 该窗口不再触发（每窗口至多一次），顺延到下一个匹配日
        let kind = fuzzy_kind(FuzzyPeriod::Daily);
        let after = utc("2026-06-01T09:30:00Z");
        let next = evaluate_next_occurrence(&kind, after).unwrap();
        assert!(next > after);
        assert_eq!(next.date_naive(), utc("2026-06-02T00:00:00Z").date_naive());
        let t = next.time();
        assert!(
            t >= NaiveTime::from_hms_opt(9, 0, 0).unwrap()
                && t < NaiveTime::from_hms_opt(10, 0, 0).unwrap()
        );
    }

    #[test]
    fn fuzzy_window_start_boundary_is_skipped() {
        // after 恰好等于 window_start：窗口已开始 → 跳到下一个匹配日
        let kind = fuzzy_kind(FuzzyPeriod::Daily);
        let after = utc("2026-06-01T09:00:00Z");
        assert_eq!(
            evaluate_next_occurrence(&kind, after).unwrap().date_naive(),
            utc("2026-06-02T00:00:00Z").date_naive()
        );
    }

    #[test]
    fn fuzzy_fires_at_most_once_per_window() {
        // 模拟 scheduler 的 fire → 以 now 重排：首个窗口内只能触发一次
        let kind = fuzzy_kind(FuzzyPeriod::Daily);
        let window_start = utc("2026-06-01T09:00:00Z");
        let window_end = utc("2026-06-01T10:00:00Z");

        let mut after = utc("2026-06-01T08:30:00Z");
        let mut fires_in_first_window = 0;
        for _ in 0..20 {
            let next = evaluate_next_occurrence(&kind, after).unwrap();
            assert!(next > after, "next occurrence must be strictly after");
            if next >= window_start && next < window_end {
                fires_in_first_window += 1;
            }
            after = next;
        }
        assert_eq!(fires_in_first_window, 1);
    }

    #[test]
    fn fuzzy_weekly_single_day_survives_skipping_current_window() {
        // 最紧路径：Weekly 只含周日，且 after 正落在周日窗口内 → 跳过当天，
        // 必须跨整整 7 天才能命中下一个周日（须落在搜索上限之内，否则永久不触发）
        let kind = fuzzy_kind(FuzzyPeriod::Weekly {
            days_of_week: vec![chrono::Weekday::Sun],
        });
        let after = utc("2026-06-07T09:30:00Z"); // 2026-06-07 是周日
        assert_eq!(after.weekday(), chrono::Weekday::Sun);
        let next = evaluate_next_occurrence(&kind, after).unwrap();
        assert!(next > after);
        assert_eq!(next.date_naive(), utc("2026-06-14T00:00:00Z").date_naive());
    }

    #[test]
    fn fuzzy_window_over_moves_to_next_matching_day() {
        // 周一(2026-06-01)窗口已过 → 下一个匹配日周二
        let kind = fuzzy_kind(FuzzyPeriod::Weekdays);
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T10:30:00Z")).unwrap();
        assert_eq!(next.date_naive(), utc("2026-06-02T00:00:00Z").date_naive());
        let t = next.time();
        assert!(
            t >= NaiveTime::from_hms_opt(9, 0, 0).unwrap()
                && t < NaiveTime::from_hms_opt(10, 0, 0).unwrap()
        );
    }

    #[test]
    fn fuzzy_weekly_skips_unlisted_days() {
        // Weekly 只含 Sunday：2026-06-01 是周一 → 下一个周日是 2026-06-07
        let kind = fuzzy_kind(FuzzyPeriod::Weekly {
            days_of_week: vec![chrono::Weekday::Sun],
        });
        let next = evaluate_next_occurrence(&kind, utc("2026-06-01T08:30:00Z")).unwrap();
        assert_eq!(next.date_naive(), utc("2026-06-07T00:00:00Z").date_naive());
    }

    #[test]
    fn fuzzy_invalid_window_returns_none() {
        let kind = TriggerKind::Fuzzy {
            period: FuzzyPeriod::Daily,
            window_start: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            window_end: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            timezone: "UTC".into(),
        };
        assert_eq!(
            evaluate_next_occurrence(&kind, utc("2026-06-01T08:30:00Z")),
            None
        );
    }
}
