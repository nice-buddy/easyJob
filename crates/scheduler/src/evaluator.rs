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
        // NOTE: temporary placeholder; real implementation lands in Task 3
        TriggerKind::Fuzzy { .. } => None,
        TriggerKind::AgentStarted => None, // Handled upon agent startup event, not periodic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyjob_domain::trigger::NetworkEventKind;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
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
}
