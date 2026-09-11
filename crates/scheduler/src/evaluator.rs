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
        TriggerKind::AgentStarted => None, // Handled upon agent startup event, not periodic
    }
}
