use serde::{Deserialize, Serialize};

/// 任务执行完成后的系统通知策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TaskNotificationPolicy {
    #[default]
    None, // 不通知
    OnlySuccess, // 仅成功时通知
    OnlyFailure, // 仅失败时通知
    All,         // 无论成功失败均通知
}

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
    #[serde(default)]
    pub notification: TaskNotificationPolicy,
    #[serde(default)]
    pub log_retention: LogRetentionPolicy,
}

/// 任务日志保留策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", content = "days")]
pub enum LogRetentionPolicy {
    /// 跟随系统设置（默认）
    #[default]
    #[serde(rename = "SystemDefault")]
    SystemDefault,
    /// 自定义保留天数（例如 7 天）
    #[serde(rename = "KeepDays")]
    KeepDays(u32),
    /// 永久保留，从不自动清理
    #[serde(rename = "Permanent")]
    Permanent,
}

/// 系统全局配置模型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSettings {
    /// 全局默认日志保留策略，默认 7 天
    pub default_log_retention: SystemLogRetention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "days")]
pub enum SystemLogRetention {
    #[serde(rename = "KeepDays")]
    KeepDays(u32),
    #[serde(rename = "Permanent")]
    Permanent,
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            default_log_retention: SystemLogRetention::KeepDays(7),
        }
    }
}
