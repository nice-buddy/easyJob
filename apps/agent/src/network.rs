use std::sync::Arc;

use easyjob_domain::trigger::{NetworkEventKind, TriggerKind};
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::SchedulerCommand;
use tokio::sync::mpsc;
use tracing::warn;

/// 运行时网络事件（含事件发生时的 SSID；有线或读取失败为 None）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEvent {
    Connect { ssid: Option<String> },
    Disconnect { ssid: Option<String> },
    Online { ssid: Option<String> },
}

impl NetworkEvent {
    pub fn kind(&self) -> NetworkEventKind {
        match self {
            NetworkEvent::Connect { .. } => NetworkEventKind::Connect,
            NetworkEvent::Disconnect { .. } => NetworkEventKind::Disconnect,
            NetworkEvent::Online { .. } => NetworkEventKind::Online,
        }
    }

    pub fn ssid(&self) -> Option<&str> {
        match self {
            NetworkEvent::Connect { ssid }
            | NetworkEvent::Disconnect { ssid }
            | NetworkEvent::Online { ssid } => ssid.as_deref(),
        }
    }
}

/// 事件类型匹配：触发器 events 列表非空且包含事件类型
pub fn event_matches(ev: &NetworkEvent, events: &[NetworkEventKind]) -> bool {
    events.contains(&ev.kind())
}

/// SSID 过滤：network_name 为 None = 任意网络；Some(name) = 精确、区分大小写匹配
pub fn name_matches(ev: &NetworkEvent, network_name: &Option<String>) -> bool {
    match network_name {
        None => true,
        Some(expected) => ev.ssid() == Some(expected.as_str()),
    }
}

/// 共享 Online 探测：TCP 1.1.1.1:80（2s 超时），失败 fallback DNS 解析 example.com
pub async fn probe_online() -> bool {
    if tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::net::TcpStream::connect("1.1.1.1:80"),
    )
    .await
    .is_ok()
    {
        return true;
    }
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::net::lookup_host("example.com:443"),
    )
    .await
    .map(|resolved| {
        resolved
            .map(|mut addrs| addrs.next().is_some())
            .unwrap_or(false)
    })
    .unwrap_or(false)
}

/// 派发：查启用任务 → 匹配 Network 触发器（事件类型 + SSID）→ TriggerNow（每任务至多一次）
pub async fn dispatch_network_event(
    ev: NetworkEvent,
    task_repo: Arc<SqliteTaskRepository>,
    scheduler_tx: mpsc::Sender<SchedulerCommand>,
) {
    let Ok(tasks) = task_repo.find_all_enabled().await else {
        warn!("network dispatch: failed to list enabled tasks");
        return;
    };
    for task in tasks {
        let hit = task.triggers.iter().any(|tr| {
            tr.enabled
                && match &tr.kind {
                    TriggerKind::Network {
                        events,
                        network_name,
                    } => event_matches(&ev, events) && name_matches(&ev, network_name),
                    _ => false,
                }
        });
        if hit {
            let _ = scheduler_tx
                .send(SchedulerCommand::TriggerNow(task.id))
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyjob_common::{ActionId, TaskId, TriggerId};
    use easyjob_domain::action::{Action, ActionKind};
    use easyjob_domain::policy::ExecutionPolicy;
    use easyjob_domain::task::Task;
    use easyjob_domain::trigger::{NetworkEventKind, Trigger};
    use easyjob_persistence::db::init_pool;
    use std::collections::HashMap;

    fn connect(ssid: Option<&str>) -> NetworkEvent {
        NetworkEvent::Connect {
            ssid: ssid.map(String::from),
        }
    }

    #[test]
    fn event_kind_maps_correctly() {
        assert_eq!(connect(Some("x")).kind(), NetworkEventKind::Connect);
        assert_eq!(
            NetworkEvent::Disconnect { ssid: None }.kind(),
            NetworkEventKind::Disconnect
        );
        assert_eq!(
            NetworkEvent::Online { ssid: None }.kind(),
            NetworkEventKind::Online
        );
    }

    #[test]
    fn event_matches_respects_list() {
        let ev = connect(Some("x"));
        assert!(event_matches(&ev, &[NetworkEventKind::Connect]));
        assert!(event_matches(
            &ev,
            &[NetworkEventKind::Disconnect, NetworkEventKind::Connect]
        ));
        assert!(!event_matches(&ev, &[NetworkEventKind::Disconnect]));
        assert!(!event_matches(&ev, &[]));
    }

    #[test]
    fn name_matches_none_is_wildcard() {
        assert!(name_matches(&connect(Some("Home")), &None));
        assert!(name_matches(&connect(None), &None));
    }

    #[test]
    fn name_matches_requires_exact_case_sensitive_ssid() {
        let name = Some("MyHome".to_string());
        assert!(name_matches(&connect(Some("MyHome")), &name));
        assert!(!name_matches(&connect(Some("myhome")), &name));
        assert!(!name_matches(&connect(None), &name));
        assert!(!name_matches(&connect(Some("MyHome2")), &name));
    }

    fn task_with_network_trigger(
        enabled: bool,
        trigger_enabled: bool,
        events: Vec<NetworkEventKind>,
        network_name: Option<String>,
    ) -> Task {
        let task_id = TaskId::new();
        Task {
            id: task_id,
            name: "network test".to_string(),
            description: None,
            enabled,
            triggers: vec![Trigger {
                id: TriggerId::new(),
                task_id,
                enabled: trigger_enabled,
                kind: TriggerKind::Network {
                    events,
                    network_name,
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }],
            actions: vec![Action {
                id: ActionId::new(),
                task_id,
                sequence: 1,
                enabled: true,
                kind: ActionKind::ExecuteShell {
                    command: "true".to_string(),
                },
            }],
            execution_policy: ExecutionPolicy::default(),
            working_directory: None,
            environment: HashMap::new(),
            version: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn dispatch_triggers_only_matching_enabled_tasks() {
        let pool = init_pool("sqlite::memory:?cache=shared").await.unwrap();
        let repo = Arc::new(SqliteTaskRepository::new(pool));

        let matching = task_with_network_trigger(
            true,
            true,
            vec![NetworkEventKind::Connect],
            Some("Home".to_string()),
        );
        let matching_id = matching.id;

        // 事件类型匹配但 SSID 不匹配
        let ssid_mismatch = task_with_network_trigger(
            true,
            true,
            vec![NetworkEventKind::Connect],
            Some("Office".to_string()),
        );
        // 触发器被禁用
        let disabled_trigger =
            task_with_network_trigger(true, false, vec![NetworkEventKind::Connect], None);
        // 任务本身被禁用（find_all_enabled 不会返回）
        let disabled_task =
            task_with_network_trigger(false, true, vec![NetworkEventKind::Connect], None);

        for task in [&matching, &ssid_mismatch, &disabled_trigger, &disabled_task] {
            repo.save(task).await.unwrap();
        }

        let (tx, mut rx) = mpsc::channel(8);
        dispatch_network_event(
            NetworkEvent::Connect {
                ssid: Some("Home".to_string()),
            },
            repo,
            tx,
        )
        .await;

        match rx.try_recv().expect("one TriggerNow dispatched") {
            SchedulerCommand::TriggerNow(id) => assert_eq!(id, matching_id),
            other => panic!("expected TriggerNow, got {other:?}"),
        }
        assert!(rx.try_recv().is_err(), "only one task should be dispatched");
    }
}
