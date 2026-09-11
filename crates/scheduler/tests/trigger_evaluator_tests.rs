use chrono::{Duration, NaiveTime, TimeZone, Utc, Weekday};
use easyjob_common::{TaskId, TriggerId};
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_scheduler::evaluator::{evaluate_next_occurrence, TriggerEvaluator};

#[test]
fn test_once_trigger() {
    let fire_at = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 0).unwrap();
    let trigger = TriggerKind::Once { fire_at };

    let before = Utc.with_ymd_and_hms(2026, 9, 30, 10, 0, 0).unwrap();
    assert_eq!(evaluate_next_occurrence(&trigger, before), Some(fire_at));

    let exact = fire_at;
    assert_eq!(evaluate_next_occurrence(&trigger, exact), None);

    let after = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 1).unwrap();
    assert_eq!(evaluate_next_occurrence(&trigger, after), None);

    // Also test via TriggerEvaluator trait
    assert_eq!(trigger.next_occurrence(before), Some(fire_at));
    assert_eq!(trigger.next_occurrence(after), None);
}

#[test]
fn test_daily_trigger_timezone() {
    let trigger = TriggerKind::Daily {
        time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        timezone: "Asia/Shanghai".to_string(), // UTC+8
    };

    // 2026-09-11 00:00:00 UTC == 2026-09-11 08:00:00 Shanghai
    let current_utc = Utc.with_ymd_and_hms(2026, 9, 11, 0, 0, 0).unwrap();
    let next = evaluate_next_occurrence(&trigger, current_utc).unwrap();

    // 9:00 AM Shanghai on 2026-09-11 is 1:00 AM UTC
    let expected_utc = Utc.with_ymd_and_hms(2026, 9, 11, 1, 0, 0).unwrap();
    assert_eq!(next, expected_utc);

    // After 9:00 AM Shanghai (e.g. 2:00 AM UTC == 10:00 AM Shanghai)
    let after_utc = Utc.with_ymd_and_hms(2026, 9, 11, 2, 0, 0).unwrap();
    let next_day = evaluate_next_occurrence(&trigger, after_utc).unwrap();
    let expected_next_day_utc = Utc.with_ymd_and_hms(2026, 9, 12, 1, 0, 0).unwrap();
    assert_eq!(next_day, expected_next_day_utc);
}

#[test]
fn test_daily_trigger_invalid_timezone_fallback() {
    let trigger = TriggerKind::Daily {
        time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        timezone: "Invalid/Timezone".to_string(), // Should fallback to UTC
    };

    let current_utc = Utc.with_ymd_and_hms(2026, 9, 11, 8, 0, 0).unwrap();
    let next = evaluate_next_occurrence(&trigger, current_utc).unwrap();
    let expected_utc = Utc.with_ymd_and_hms(2026, 9, 11, 9, 0, 0).unwrap();
    assert_eq!(next, expected_utc);
}

#[test]
fn test_interval_trigger() {
    let start_at = Utc.with_ymd_and_hms(2026, 9, 11, 12, 0, 0).unwrap();
    let trigger = TriggerKind::Interval {
        interval_secs: 60,
        start_at: Some(start_at),
    };

    let check_time = Utc.with_ymd_and_hms(2026, 9, 11, 12, 0, 30).unwrap();
    let next = evaluate_next_occurrence(&trigger, check_time).unwrap();
    let expected = Utc.with_ymd_and_hms(2026, 9, 11, 12, 1, 0).unwrap();
    assert_eq!(next, expected);

    // Before start_at should return start_at
    let before_start = Utc.with_ymd_and_hms(2026, 9, 11, 11, 0, 0).unwrap();
    assert_eq!(
        evaluate_next_occurrence(&trigger, before_start),
        Some(start_at)
    );

    // Exactly at start_at should return next interval
    let at_start = start_at;
    assert_eq!(
        evaluate_next_occurrence(&trigger, at_start),
        Some(start_at + Duration::seconds(60))
    );

    // Interval without start_at
    let trigger_no_start = TriggerKind::Interval {
        interval_secs: 120,
        start_at: None,
    };
    let now = Utc.with_ymd_and_hms(2026, 9, 11, 10, 0, 0).unwrap();
    assert_eq!(
        evaluate_next_occurrence(&trigger_no_start, now),
        Some(Utc.with_ymd_and_hms(2026, 9, 11, 10, 2, 0).unwrap())
    );

    // Interval with 0 secs should return None
    let trigger_zero = TriggerKind::Interval {
        interval_secs: 0,
        start_at: Some(start_at),
    };
    assert_eq!(evaluate_next_occurrence(&trigger_zero, check_time), None);
}

#[test]
fn test_weekly_trigger() {
    // Mon and Wed at 10:00 UTC
    let trigger = TriggerKind::Weekly {
        days_of_week: vec![Weekday::Mon, Weekday::Wed],
        time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        timezone: "UTC".to_string(),
    };

    // 2026-09-07 was a Monday
    // Check Monday before 10:00: should fire today at 10:00
    let mon_morning = Utc.with_ymd_and_hms(2026, 9, 7, 8, 0, 0).unwrap();
    let next = evaluate_next_occurrence(&trigger, mon_morning).unwrap();
    assert_eq!(next, Utc.with_ymd_and_hms(2026, 9, 7, 10, 0, 0).unwrap());

    // Check Monday after 10:00: should fire Wednesday at 10:00
    let mon_afternoon = Utc.with_ymd_and_hms(2026, 9, 7, 11, 0, 0).unwrap();
    let next_wed = evaluate_next_occurrence(&trigger, mon_afternoon).unwrap();
    assert_eq!(
        next_wed,
        Utc.with_ymd_and_hms(2026, 9, 9, 10, 0, 0).unwrap()
    );

    // Check Thursday after Wednesday: should wrap around to next Monday (2026-09-14)
    let thu = Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
    let next_mon = evaluate_next_occurrence(&trigger, thu).unwrap();
    assert_eq!(
        next_mon,
        Utc.with_ymd_and_hms(2026, 9, 14, 10, 0, 0).unwrap()
    );

    // Empty days_of_week should return None
    let trigger_empty = TriggerKind::Weekly {
        days_of_week: vec![],
        time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        timezone: "UTC".to_string(),
    };
    assert_eq!(evaluate_next_occurrence(&trigger_empty, mon_morning), None);
}

#[test]
fn test_agent_started_trigger() {
    let trigger = TriggerKind::AgentStarted;
    let now = Utc::now();
    assert_eq!(evaluate_next_occurrence(&trigger, now), None);
}

#[test]
fn test_dst_spring_forward_gap() {
    // 2026-03-08 in America/New_York, 02:00 skips to 03:00 (EDT)
    // 02:30 local time does not exist
    let trigger = TriggerKind::Daily {
        time: NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
        timezone: "America/New_York".to_string(),
    };

    // Before the gap: 2026-03-08 01:00 EST is 2026-03-08 06:00 UTC
    let before_utc = Utc.with_ymd_and_hms(2026, 3, 8, 6, 0, 0).unwrap();
    let next = evaluate_next_occurrence(&trigger, before_utc);
    assert!(
        next.is_some(),
        "Should shift forward to valid local time during DST gap"
    );
    let next_utc = next.unwrap();
    assert!(next_utc > before_utc);
}

#[test]
fn test_trigger_trait_enabled_disabled() {
    let fire_at = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 9, 30, 10, 0, 0).unwrap();

    let enabled_trigger = Trigger {
        id: TriggerId::new(),
        task_id: TaskId::new(),
        enabled: true,
        kind: TriggerKind::Once { fire_at },
        created_at: now,
        updated_at: now,
    };
    assert_eq!(enabled_trigger.next_occurrence(now), Some(fire_at));

    let disabled_trigger = Trigger {
        id: TriggerId::new(),
        task_id: TaskId::new(),
        enabled: false,
        kind: TriggerKind::Once { fire_at },
        created_at: now,
        updated_at: now,
    };
    assert_eq!(disabled_trigger.next_occurrence(now), None);
}
