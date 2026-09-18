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
            // 规格仅支持标准 5 字段；croner 会接受 @daily 等别名，这里一并拒绝
            if expression.split_whitespace().count() != 5 {
                return None;
            }
            // NOTE: croner 的 `FromStr` 实现不校验表达式（恒返回 `Ok`），
            // 真正的解析/校验发生在 `Cron::parse`，非法表达式在此返回 `Err`。
            let cron = match croner::Cron::from_str(expression).and_then(|mut cron| cron.parse()) {
                Ok(c) => c,
                Err(_) => return None,
            };
            let tz: Tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
            let local_after = after.with_timezone(&tz);
            // croner 基于 tz-aware DateTime 计算，内部处理 DST；
            // inclusive=false → 严格晚于 after
            cron.find_next_occurrence(&local_after, false)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
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

            for _ in 0..8 {
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

/// 校验 cron 表达式是否为受支持的 5 字段合法表达式（agent 侧 warn 日志用）
pub fn validate_cron_expression(expression: &str) -> bool {
    // 规格仅支持标准 5 字段；croner 会接受 @daily 等别名，这里一并拒绝
    if expression.split_whitespace().count() != 5 {
        return false;
    }
    // NOTE: croner 的 `FromStr` 不校验表达式（恒返回 `Ok`），真正的校验发生在 `Cron::parse`
    croner::Cron::from_str(expression)
        .and_then(|mut cron| cron.parse())
        .is_ok()
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
