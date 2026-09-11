use chrono::{DateTime, Utc};

pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

pub fn duration_ms_between(start: DateTime<Utc>, end: DateTime<Utc>) -> u64 {
    (end - start).num_milliseconds().max(0) as u64
}

pub fn duration_between_utc(start: DateTime<Utc>, end: DateTime<Utc>) -> u64 {
    duration_ms_between(start, end)
}
