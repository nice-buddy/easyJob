use chrono::{DateTime, Duration, NaiveTime, Utc};
use easyjob_common::{TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::policy::{
    ConcurrencyPolicy, ExecutionPolicy, MissedRunPolicy, RetryPolicy, TaskNotificationPolicy,
};
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{FuzzyPeriod, Trigger, TriggerKind};
use easyjob_scheduler::queue::{ScheduleQueue, ScheduledItem};
use easyjob_scheduler::scheduler::{Scheduler, SchedulerCommand, TriggerEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

#[test]
fn test_schedule_queue_ordering_and_invalidation() {
    let mut queue = ScheduleQueue::new();
    let task1 = TaskId::new();
    let task2 = TaskId::new();

    let now = Utc::now();
    let item1 = ScheduledItem {
        task_id: task1,
        trigger_id: TriggerId::new(),
        next_fire_at: now + Duration::seconds(10),
        generation: 1,
    };
    let item2 = ScheduledItem {
        task_id: task2,
        trigger_id: TriggerId::new(),
        next_fire_at: now + Duration::seconds(5),
        generation: 1,
    };

    queue.push(item1.clone());
    queue.push(item2);

    // Earliest must be popped first
    let popped = queue.pop().expect("item in queue");
    assert_eq!(popped.task_id, task2);

    // Invalidate task2 (popped)
    queue.bump_generation(&task2);
    assert!(!queue.is_valid(&popped));

    // Invalidate task1 (still in queue)
    queue.bump_generation(&task1);
    assert!(!queue.is_valid(&item1));
    assert!(queue.pop().is_none());
}

#[test]
fn test_schedule_queue_stale_generation_push_rejected() {
    let mut queue = ScheduleQueue::new();
    let task_id = TaskId::new();
    let now = Utc::now();

    // Bump generation for task_id to 2
    let gen = queue.bump_generation(&task_id);
    assert_eq!(gen, 2);

    // Attempt to push item with stale generation 1
    let stale_item = ScheduledItem {
        task_id,
        trigger_id: TriggerId::new(),
        next_fire_at: now + Duration::seconds(5),
        generation: 1,
    };
    queue.push(stale_item);

    // Queue should remain empty
    assert!(queue.pop().is_none());

    // Push item with valid generation 2
    let valid_item = ScheduledItem {
        task_id,
        trigger_id: TriggerId::new(),
        next_fire_at: now + Duration::seconds(5),
        generation: 2,
    };
    queue.push(valid_item);
    assert!(queue.pop().is_some());
}

#[test]
fn test_schedule_queue_peek_and_clear() {
    let mut queue = ScheduleQueue::new();
    let task_id = TaskId::new();
    let now = Utc::now();

    let item = ScheduledItem {
        task_id,
        trigger_id: TriggerId::new(),
        next_fire_at: now + Duration::seconds(5),
        generation: 1,
    };
    queue.push(item);

    assert_eq!(queue.len(), 1);
    assert!(!queue.is_empty());
    assert!(queue.peek().is_some());
    assert_eq!(queue.peek().unwrap().task_id, task_id);

    queue.clear();
    assert_eq!(queue.len(), 0);
    assert!(queue.is_empty());
    assert!(queue.peek().is_none());
    assert!(queue.pop().is_none());
}

#[test]
fn test_scheduled_item_ordering_tie_breaker() {
    let now = Utc::now();
    let task1 = TaskId::new();
    let task2 = TaskId::new();
    let trigger_id = TriggerId::new();

    let item1 = ScheduledItem {
        task_id: task1.min(task2),
        trigger_id,
        next_fire_at: now,
        generation: 1,
    };
    let item2 = ScheduledItem {
        task_id: task1.max(task2),
        trigger_id,
        next_fire_at: now,
        generation: 1,
    };

    let mut queue = ScheduleQueue::new();
    queue.push(item2.clone());
    queue.push(item1.clone());

    // Tie-breaker pops item2 first (self.task_id.cmp(&other.task_id))
    let first = queue.pop().unwrap();
    let second = queue.pop().unwrap();
    assert_eq!(first.task_id, item2.task_id);
    assert_eq!(second.task_id, item1.task_id);
}

#[test]
fn test_scheduled_item_ord_consistency_with_generation_and_eq() {
    let now = Utc::now();
    let task_id = TaskId::new();
    let trigger_id = TriggerId::new();

    let item_gen1 = ScheduledItem {
        task_id,
        trigger_id,
        next_fire_at: now,
        generation: 1,
    };
    let item_gen2 = ScheduledItem {
        task_id,
        trigger_id,
        next_fire_at: now,
        generation: 2,
    };
    let item_gen1_clone = item_gen1.clone();

    // Ord vs Eq consistency:
    // When items are equal, cmp returns Ordering::Equal
    assert_eq!(item_gen1, item_gen1_clone);
    assert_eq!(item_gen1.cmp(&item_gen1_clone), std::cmp::Ordering::Equal);

    // When generations differ, Ord distinguishes them
    assert_ne!(item_gen1, item_gen2);
    assert_eq!(item_gen1.cmp(&item_gen2), std::cmp::Ordering::Less);
    assert_eq!(item_gen2.cmp(&item_gen1), std::cmp::Ordering::Greater);
}

#[test]
fn test_scheduler_command_add_task_boxed() {
    let task_id = TaskId::new();
    let trigger = Trigger {
        id: TriggerId::new(),
        task_id,
        enabled: true,
        kind: TriggerKind::Once {
            fire_at: Utc::now(),
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let task = sample_task(task_id, trigger);
    let cmd = SchedulerCommand::add_task(task);
    match cmd {
        SchedulerCommand::AddTask(boxed) => {
            assert_eq!(boxed.id, task_id);
        }
        _ => panic!("Expected SchedulerCommand::AddTask"),
    }
}

fn sample_task(task_id: TaskId, trigger: Trigger) -> Task {
    Task {
        id: task_id,
        name: "Test Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![trigger],
        actions: vec![Action {
            id: easyjob_common::ActionId::new(),
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo test".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy {
            concurrency_policy: ConcurrencyPolicy::AllowParallel,
            missed_run_policy: MissedRunPolicy::RunOnce,
            retry_policy: RetryPolicy::default(),
            timeout_secs: Some(10),
            notification: TaskNotificationPolicy::default(),
            log_retention: easyjob_domain::LogRetentionPolicy::SystemDefault,
        },
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn test_scheduler_trigger_now() {
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());

    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, event_tx));

    let task_id = TaskId::new();
    scheduler
        .sender()
        .send(SchedulerCommand::TriggerNow(task_id))
        .await
        .unwrap();

    let event: TriggerEvent =
        tokio::time::timeout(std::time::Duration::from_millis(500), event_rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("event received");

    assert_eq!(event.task_id, task_id);
    assert_eq!(event.trigger_id, None);

    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_scheduler_add_task_and_fire_trigger() {
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());

    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, event_tx));

    let task_id = TaskId::new();
    let trigger_id = TriggerId::new();

    // Trigger firing almost immediately (Once 50ms in future)
    let fire_at = Utc::now() + Duration::milliseconds(50);
    let trigger = Trigger {
        id: trigger_id,
        task_id,
        enabled: true,
        kind: TriggerKind::Once { fire_at },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let task = sample_task(task_id, trigger);

    scheduler
        .sender()
        .send(SchedulerCommand::add_task(task))
        .await
        .unwrap();

    let event: TriggerEvent =
        tokio::time::timeout(std::time::Duration::from_millis(1500), event_rx.recv())
            .await
            .expect("timed out waiting for scheduled event")
            .expect("event received");

    assert_eq!(event.task_id, task_id);
    assert_eq!(event.trigger_id, Some(trigger_id));

    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_scheduler_remove_task_cancels_future_trigger() {
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());

    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, event_tx));

    let task_id = TaskId::new();
    let trigger_id = TriggerId::new();

    // Trigger scheduled 200ms in future
    let fire_at = Utc::now() + Duration::milliseconds(200);
    let trigger = Trigger {
        id: trigger_id,
        task_id,
        enabled: true,
        kind: TriggerKind::Once { fire_at },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let task = sample_task(task_id, trigger);

    // Add then immediately remove task
    scheduler
        .sender()
        .send(SchedulerCommand::AddTask(Box::new(task)))
        .await
        .unwrap();

    scheduler
        .sender()
        .send(SchedulerCommand::RemoveTask(task_id))
        .await
        .unwrap();

    // Wait 400ms - no event should be dispatched
    let res = tokio::time::timeout(std::time::Duration::from_millis(400), event_rx.recv()).await;
    assert!(
        res.is_err(),
        "Expected timeout, but received event: {:?}",
        res
    );

    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_scheduler_disabled_task_and_trigger_ignored() {
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());

    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, event_tx));

    let task_id = TaskId::new();
    let fire_at = Utc::now() + Duration::milliseconds(50);

    // Disabled trigger
    let trigger = Trigger {
        id: TriggerId::new(),
        task_id,
        enabled: false,
        kind: TriggerKind::Once { fire_at },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let task = sample_task(task_id, trigger);

    scheduler
        .sender()
        .send(SchedulerCommand::add_task(task))
        .await
        .unwrap();

    // Disabled task
    let task_id_2 = TaskId::new();
    let trigger2 = Trigger {
        id: TriggerId::new(),
        task_id: task_id_2,
        enabled: true,
        kind: TriggerKind::Once { fire_at },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let mut task2 = sample_task(task_id_2, trigger2);
    task2.enabled = false;

    scheduler
        .sender()
        .send(SchedulerCommand::add_task(task2))
        .await
        .unwrap();

    // No events should be dispatched
    let res = tokio::time::timeout(std::time::Duration::from_millis(200), event_rx.recv()).await;
    assert!(
        res.is_err(),
        "Expected timeout, but received event: {:?}",
        res
    );

    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_scheduler_update_task_invalidates_earlier_trigger() {
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());

    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, event_tx));

    let task_id = TaskId::new();
    let trigger1_id = TriggerId::new();
    let trigger2_id = TriggerId::new();

    // Trigger 1 fires at 100ms
    let task_v1 = sample_task(
        task_id,
        Trigger {
            id: trigger1_id,
            task_id,
            enabled: true,
            kind: TriggerKind::Once {
                fire_at: Utc::now() + Duration::milliseconds(100),
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
    );

    scheduler
        .sender()
        .send(SchedulerCommand::AddTask(Box::new(task_v1)))
        .await
        .unwrap();

    // Update task to Trigger 2 at 250ms (invalidating trigger 1)
    let task_v2 = sample_task(
        task_id,
        Trigger {
            id: trigger2_id,
            task_id,
            enabled: true,
            kind: TriggerKind::Once {
                fire_at: Utc::now() + Duration::milliseconds(250),
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
    );

    scheduler
        .sender()
        .send(SchedulerCommand::AddTask(Box::new(task_v2)))
        .await
        .unwrap();

    // Receive first event - it should be trigger2, NOT trigger1!
    let event: TriggerEvent =
        tokio::time::timeout(std::time::Duration::from_millis(600), event_rx.recv())
            .await
            .expect("timed out waiting for scheduled event")
            .expect("event received");

    assert_eq!(event.task_id, task_id);
    assert_eq!(event.trigger_id, Some(trigger2_id));

    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_scheduler_loop_exits_on_sender_drop() {
    let (event_tx, _event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());

    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, event_tx));

    // Dropping scheduler drops the cmd_tx sender
    drop(scheduler);

    // Loop should detect cmd_rx.recv() == None and exit
    let res = tokio::time::timeout(std::time::Duration::from_millis(500), handle).await;
    assert!(
        res.is_ok(),
        "Scheduler loop did not terminate after sender drop"
    );
}

#[tokio::test]
async fn test_scheduler_interval_trigger_reschedules_multiple_events() {
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());

    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, event_tx));

    let task_id = TaskId::new();
    let trigger_id = TriggerId::new();

    // 1-second interval trigger starting almost 1 second ago so 1st event is immediate
    let trigger = Trigger {
        id: trigger_id,
        task_id,
        enabled: true,
        kind: TriggerKind::Interval {
            interval_secs: 1,
            start_at: Some(Utc::now() - Duration::milliseconds(950)),
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let task = sample_task(task_id, trigger);

    scheduler
        .sender()
        .send(SchedulerCommand::add_task(task))
        .await
        .unwrap();

    // 1st event should fire almost immediately (~50ms)
    let event1: TriggerEvent =
        tokio::time::timeout(std::time::Duration::from_millis(1500), event_rx.recv())
            .await
            .expect("timed out waiting for 1st event")
            .expect("1st event received");

    assert_eq!(event1.task_id, task_id);
    assert_eq!(event1.trigger_id, Some(trigger_id));

    // 2nd event should automatically fire ~1000ms later due to recurring rescheduling
    let event2: TriggerEvent =
        tokio::time::timeout(std::time::Duration::from_millis(2500), event_rx.recv())
            .await
            .expect("timed out waiting for 2nd rescheduled event")
            .expect("2nd event received");

    assert_eq!(event2.task_id, task_id);
    assert_eq!(event2.trigger_id, Some(trigger_id));

    // Clean shutdown
    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}

fn task_with_triggers(task_id: TaskId, triggers: Vec<Trigger>) -> Task {
    Task {
        id: task_id,
        name: "Reroll Test Task".to_string(),
        description: None,
        enabled: true,
        triggers,
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

async fn reroll(
    scheduler: &Scheduler,
    task_id: TaskId,
    trigger_id: TriggerId,
) -> std::result::Result<Option<DateTime<Utc>>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    scheduler
        .sender()
        .send(SchedulerCommand::RerollTrigger {
            task_id,
            trigger_id,
            reply: tx,
        })
        .await
        .unwrap();
    rx.await.unwrap()
}

async fn read_next_fire(
    queue: &Arc<tokio::sync::Mutex<ScheduleQueue>>,
    key: (TaskId, TriggerId),
) -> Option<DateTime<Utc>> {
    let q = queue.lock().await;
    q.next_fire_by_trigger().get(&key).copied()
}

#[tokio::test]
async fn test_reroll_trigger_rerolls_only_target_and_keeps_other_triggers() {
    let (event_tx, _event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());
    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue.clone(), cmd_rx, event_tx));

    let task_id = TaskId::new();
    let fuzzy_id = TriggerId::new();
    let interval_id = TriggerId::new();

    let fuzzy = Trigger {
        id: fuzzy_id,
        task_id,
        enabled: true,
        kind: TriggerKind::Fuzzy {
            period: FuzzyPeriod::Daily,
            window_start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            window_end: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            timezone: "UTC".into(),
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let interval = Trigger {
        id: interval_id,
        task_id,
        enabled: true,
        kind: TriggerKind::Interval {
            interval_secs: 3600,
            start_at: None,
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    scheduler
        .sender()
        .send(SchedulerCommand::add_task(task_with_triggers(
            task_id,
            vec![fuzzy, interval],
        )))
        .await
        .unwrap();

    // 第一次重摇的回执返回时，AddTask 必已被调度循环处理，可直接作为基线
    let fuzzy_baseline = reroll(&scheduler, task_id, fuzzy_id)
        .await
        .expect("reroll should succeed")
        .expect("daily fuzzy trigger always has a next occurrence");
    let interval_baseline = read_next_fire(&queue, (task_id, interval_id))
        .await
        .expect("interval trigger must be enqueued");

    let mut seen_different = false;
    for _ in 0..30 {
        let next = reroll(&scheduler, task_id, fuzzy_id)
            .await
            .expect("reroll should succeed")
            .expect("daily fuzzy trigger always has a next occurrence");
        if next != fuzzy_baseline {
            seen_different = true;
        }
        // 回归防护：同任务其它触发器的下次时间必须完全不变（复用 add_task 会把它推后）
        assert_eq!(
            read_next_fire(&queue, (task_id, interval_id)).await,
            Some(interval_baseline),
            "rerolling one trigger must not reschedule the other trigger"
        );
        // 不应累积或丢失条目
        assert_eq!(queue.lock().await.len(), 2);
    }
    assert!(
        seen_different,
        "30 次重摇都没有改变 Fuzzy 的随机点，说明重摇没有真正重新求值"
    );

    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_reroll_trigger_rejects_unregistered_task_and_disabled_trigger() {
    let (event_tx, _event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());
    let queue = scheduler.queue();
    let handle = tokio::spawn(Scheduler::run(queue, cmd_rx, event_tx));

    // 未注册任务
    let result = reroll(&scheduler, TaskId::new(), TriggerId::new()).await;
    assert!(result.is_err(), "unregistered task must be rejected");

    // 已注册任务但触发器停用（mpsc 保序，AddTask 必先于 RerollTrigger 被处理）
    let task_id = TaskId::new();
    let disabled_trigger_id = TriggerId::new();
    let task = task_with_triggers(
        task_id,
        vec![Trigger {
            id: disabled_trigger_id,
            task_id,
            enabled: false,
            kind: TriggerKind::Interval {
                interval_secs: 60,
                start_at: None,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }],
    );
    scheduler
        .sender()
        .send(SchedulerCommand::add_task(task))
        .await
        .unwrap();

    let result = reroll(&scheduler, task_id, disabled_trigger_id).await;
    assert!(result.is_err(), "disabled trigger must be rejected");

    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    handle.await.unwrap();
}
