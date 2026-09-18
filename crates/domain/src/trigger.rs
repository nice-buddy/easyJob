use chrono::{DateTime, NaiveTime, Utc, Weekday};
use easyjob_common::{TaskId, TriggerId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerKind {
    Once {
        fire_at: DateTime<Utc>,
    },
    Daily {
        time: NaiveTime,
        timezone: String,
    },
    Weekly {
        days_of_week: Vec<Weekday>,
        time: NaiveTime,
        timezone: String,
    },
    Interval {
        interval_secs: u64,
        start_at: Option<DateTime<Utc>>,
    },
    AgentStarted,
    /// 标准 5 字段 cron（分 时 日 月 周），按指定时区解释
    Cron {
        expression: String,
        timezone: String,
    },
    /// 周期窗口内随机触发：每个匹配周期在 [window_start, window_end) 内随机选点
    Fuzzy {
        period: FuzzyPeriod,
        window_start: NaiveTime,
        window_end: NaiveTime,
        timezone: String,
    },
    /// 网络变动触发（仅 macOS / Windows）
    Network {
        events: Vec<NetworkEventKind>,
        network_name: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FuzzyPeriod {
    Daily,
    Weekdays,
    Weekends,
    Weekly { days_of_week: Vec<Weekday> },
}

impl FuzzyPeriod {
    pub fn matches_weekday(&self, w: Weekday) -> bool {
        match self {
            FuzzyPeriod::Daily => true,
            FuzzyPeriod::Weekdays => !matches!(w, Weekday::Sat | Weekday::Sun),
            FuzzyPeriod::Weekends => matches!(w, Weekday::Sat | Weekday::Sun),
            FuzzyPeriod::Weekly { days_of_week } => days_of_week.contains(&w),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkEventKind {
    Connect,
    Disconnect,
    Online,
}

impl TriggerKind {
    /// 纯变体名（持久化 triggers.kind 列使用）
    pub fn kind_name(&self) -> &'static str {
        match self {
            TriggerKind::Once { .. } => "Once",
            TriggerKind::Daily { .. } => "Daily",
            TriggerKind::Weekly { .. } => "Weekly",
            TriggerKind::Interval { .. } => "Interval",
            TriggerKind::AgentStarted => "AgentStarted",
            TriggerKind::Cron { .. } => "Cron",
            TriggerKind::Fuzzy { .. } => "Fuzzy",
            TriggerKind::Network { .. } => "Network",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub id: TriggerId,
    pub task_id: TaskId,
    pub enabled: bool,
    pub kind: TriggerKind,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_kind_serializes_with_expression_and_timezone() {
        let kind = TriggerKind::Cron {
            expression: "*/5 * * * *".into(),
            timezone: "Asia/Shanghai".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains(r#""Cron""#));
        assert!(json.contains("*/5 * * * *"));
        let back: TriggerKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }

    #[test]
    fn fuzzy_period_unit_variants_serialize_as_strings() {
        // serde 对无字段枚举变体序列化为字符串
        assert_eq!(
            serde_json::to_string(&FuzzyPeriod::Weekdays).unwrap(),
            r#""Weekdays""#
        );
        let weekly = FuzzyPeriod::Weekly {
            days_of_week: vec![Weekday::Mon, Weekday::Fri],
        };
        let json = serde_json::to_string(&weekly).unwrap();
        assert!(json.contains(r#""Weekly""#));
        assert!(json.contains("days_of_week"));
        let back: FuzzyPeriod = serde_json::from_str(&json).unwrap();
        assert_eq!(back, weekly);
    }

    #[test]
    fn network_kind_serializes_events_and_optional_name() {
        let kind = TriggerKind::Network {
            events: vec![NetworkEventKind::Connect, NetworkEventKind::Online],
            network_name: Some("MyHome".into()),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains(r#""Connect""#));
        assert!(json.contains("MyHome"));
        let none_kind = TriggerKind::Network {
            events: vec![NetworkEventKind::Disconnect],
            network_name: None,
        };
        let back: TriggerKind =
            serde_json::from_str(&serde_json::to_string(&none_kind).unwrap()).unwrap();
        assert_eq!(back, none_kind);
    }

    #[test]
    fn kind_name_returns_pure_variant_names() {
        let mk = |k: TriggerKind| k.kind_name().to_string();
        assert_eq!(
            mk(TriggerKind::Once {
                fire_at: Utc::now()
            }),
            "Once"
        );
        assert_eq!(
            mk(TriggerKind::Daily {
                time: NaiveTime::MIN,
                timezone: "UTC".into()
            }),
            "Daily"
        );
        assert_eq!(mk(TriggerKind::AgentStarted), "AgentStarted");
        assert_eq!(
            mk(TriggerKind::Cron {
                expression: "".into(),
                timezone: "".into()
            }),
            "Cron"
        );
        assert_eq!(
            mk(TriggerKind::Fuzzy {
                period: FuzzyPeriod::Daily,
                window_start: NaiveTime::MIN,
                window_end: NaiveTime::MIN,
                timezone: "".into(),
            }),
            "Fuzzy"
        );
        assert_eq!(
            mk(TriggerKind::Network {
                events: vec![],
                network_name: None
            }),
            "Network"
        );
    }

    #[test]
    fn fuzzy_period_matches_weekday() {
        assert!(FuzzyPeriod::Daily.matches_weekday(Weekday::Sun));
        assert!(FuzzyPeriod::Weekdays.matches_weekday(Weekday::Mon));
        assert!(!FuzzyPeriod::Weekdays.matches_weekday(Weekday::Sat));
        assert!(FuzzyPeriod::Weekends.matches_weekday(Weekday::Sun));
        assert!(!FuzzyPeriod::Weekends.matches_weekday(Weekday::Mon));
        let weekly = FuzzyPeriod::Weekly {
            days_of_week: vec![Weekday::Fri],
        };
        assert!(weekly.matches_weekday(Weekday::Fri));
        assert!(!weekly.matches_weekday(Weekday::Sat));
    }
}
