use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConcurrencyPolicy {
    AllowParallel,
    #[default]
    SkipIfRunning,
    QueueOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MissedRunPolicy {
    #[default]
    RunOnce,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub delay_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionPolicy {
    pub concurrency_policy: ConcurrencyPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub retry_policy: RetryPolicy,
    pub timeout_secs: Option<u64>,
}
