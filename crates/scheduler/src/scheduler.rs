use crate::evaluator::evaluate_next_occurrence;
use crate::queue::{ScheduleQueue, ScheduledItem};
use chrono::{DateTime, Utc};
use easyjob_common::{TaskId, TriggerId};
use easyjob_domain::task::Task;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// 重摇回执：`Ok(Some(next))` 表示新的下次触发时间，`Ok(None)` 表示该触发器不再有下一次。
/// `Err(原因)` 表示任务未注册到调度器、任务或触发器已停用、或触发器不存在。
pub type RerollReply =
    tokio::sync::oneshot::Sender<std::result::Result<Option<DateTime<Utc>>, String>>;

#[derive(Debug)]
pub enum SchedulerCommand {
    AddTask(Box<Task>),
    RemoveTask(TaskId),
    TriggerNow(TaskId),
    RerollTrigger {
        task_id: TaskId,
        trigger_id: TriggerId,
        reply: RerollReply,
    },
    Shutdown,
}

impl SchedulerCommand {
    pub fn add_task(task: Task) -> Self {
        Self::AddTask(Box::new(task))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerEvent {
    pub task_id: TaskId,
    pub trigger_id: Option<TriggerId>,
    pub scheduled_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Scheduler {
    queue: Arc<Mutex<ScheduleQueue>>,
    cmd_tx: Sender<SchedulerCommand>,
    event_tx: Sender<TriggerEvent>,
}

impl Scheduler {
    pub fn new(event_tx: Sender<TriggerEvent>) -> (Self, Receiver<SchedulerCommand>) {
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let scheduler = Self {
            queue: Arc::new(Mutex::new(ScheduleQueue::new())),
            cmd_tx,
            event_tx,
        };
        (scheduler, cmd_rx)
    }

    pub fn sender(&self) -> Sender<SchedulerCommand> {
        self.cmd_tx.clone()
    }

    pub fn queue(&self) -> Arc<Mutex<ScheduleQueue>> {
        self.queue.clone()
    }

    pub fn event_sender(&self) -> Sender<TriggerEvent> {
        self.event_tx.clone()
    }

    pub async fn run(
        queue: Arc<Mutex<ScheduleQueue>>,
        mut cmd_rx: Receiver<SchedulerCommand>,
        event_tx: Sender<TriggerEvent>,
    ) {
        let mut registered_tasks: HashMap<TaskId, Box<Task>> = HashMap::new();
        let mut jump_interval = tokio::time::interval(Duration::from_secs(10));
        jump_interval.reset();
        let mut last_instant = Instant::now();
        let mut last_wall = Utc::now();

        loop {
            let next_deadline = {
                let mut q = queue.lock().await;
                q.peek_valid().cloned()
            };

            let sleep_duration = match &next_deadline {
                Some(item) => {
                    let now = Utc::now();
                    if item.next_fire_at <= now {
                        Duration::ZERO
                    } else {
                        (item.next_fire_at - now).to_std().unwrap_or(Duration::ZERO)
                    }
                }
                None => Duration::from_secs(3600), // Idle wait if queue empty
            };

            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(SchedulerCommand::Shutdown) | None => {
                            info!("Scheduler loop stopping");
                            break;
                        }
                        Some(SchedulerCommand::AddTask(task)) => {
                            let mut q = queue.lock().await;
                            let gen = q.bump_generation(&task.id);
                            if task.enabled {
                                for tr in &task.triggers {
                                    if tr.enabled {
                                        if let Some(next) = evaluate_next_occurrence(&tr.kind, Utc::now()) {
                                            q.push(ScheduledItem {
                                                task_id: task.id,
                                                trigger_id: tr.id,
                                                next_fire_at: next,
                                                generation: gen,
                                            });
                                        }
                                    }
                                }
                            }
                            registered_tasks.insert(task.id, task);
                        }
                        Some(SchedulerCommand::RemoveTask(id)) => {
                            registered_tasks.remove(&id);
                            let mut q = queue.lock().await;
                            q.bump_generation(&id);
                        }
                        Some(SchedulerCommand::TriggerNow(id)) => {
                            let _ = event_tx.send(TriggerEvent {
                                task_id: id,
                                trigger_id: None,
                                scheduled_at: Utc::now(),
                            }).await;
                        }
                        Some(SchedulerCommand::RerollTrigger { task_id, trigger_id, reply }) => {
                            let result =
                                reroll_trigger(&queue, &registered_tasks, task_id, trigger_id)
                                    .await;
                            let _ = reply.send(result);
                        }
                    }
                }
                _ = tokio::time::sleep(sleep_duration), if next_deadline.is_some() => {
                    let mut q = queue.lock().await;
                    if let Some(item) = q.pop() {
                        if let Some(task) = registered_tasks.get(&item.task_id) {
                            let current_gen = q.current_generation(&task.id);
                            if task.enabled && item.generation == current_gen {
                                if let Some(trigger) = task.triggers.iter().find(|t| t.id == item.trigger_id) {
                                    if trigger.enabled {
                                        if let Some(next_fire) = evaluate_next_occurrence(&trigger.kind, Utc::now()) {
                                            q.push(ScheduledItem {
                                                task_id: task.id,
                                                trigger_id: trigger.id,
                                                next_fire_at: next_fire,
                                                generation: current_gen,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        drop(q);
                        let _ = event_tx.send(TriggerEvent {
                            task_id: item.task_id,
                            trigger_id: Some(item.trigger_id),
                            scheduled_at: item.next_fire_at,
                        }).await;
                    }
                }
                _ = jump_interval.tick() => {
                    let wall_elapsed = (Utc::now() - last_wall).num_seconds();
                    let mono_elapsed = last_instant.elapsed().as_secs() as i64;
                    let drift = (wall_elapsed - mono_elapsed).abs();
                    if drift > 30 {
                        warn!(
                            "System clock jump detected (drift: {}s, wall_elapsed: {}s, mono_elapsed: {}s); rebuilding scheduler queue",
                            drift, wall_elapsed, mono_elapsed
                        );
                        let mut q = queue.lock().await;
                        q.clear();
                        let now = Utc::now();
                        for task in registered_tasks.values() {
                            if task.enabled {
                                let gen = q.current_generation(&task.id);
                                for tr in &task.triggers {
                                    if tr.enabled {
                                        if let Some(next) = evaluate_next_occurrence(&tr.kind, now) {
                                            q.push(ScheduledItem {
                                                task_id: task.id,
                                                trigger_id: tr.id,
                                                next_fire_at: next,
                                                generation: gen,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    last_wall = Utc::now();
                    last_instant = Instant::now();
                }
            }
        }
    }
}

/// 只重排目标触发器：目标触发器以 `now` 重新求值（Fuzzy 因此得到新的随机点），
/// 同一任务其它触发器的 `next_fire_at` 保持原值。
///
/// 实现要点：先按旧的当前 generation 过滤出该任务真正有效的条目，再 `bump_generation`
/// 让残留条目失效，最后按新 generation 重新入队。绝不复用 `add_task`
/// （那会以 `now` 为基准重算同任务的 Interval 触发器，把它的下次时间推后）。
async fn reroll_trigger(
    queue: &Arc<Mutex<ScheduleQueue>>,
    registered_tasks: &HashMap<TaskId, Box<Task>>,
    task_id: TaskId,
    trigger_id: TriggerId,
) -> std::result::Result<Option<DateTime<Utc>>, String> {
    let Some(task) = registered_tasks.get(&task_id) else {
        return Err(format!(
            "Task '{}' is not registered to the scheduler",
            task_id
        ));
    };
    if !task.enabled {
        return Err(format!("Task '{}' is disabled", task_id));
    }
    let Some(trigger) = task.triggers.iter().find(|t| t.id == trigger_id) else {
        return Err(format!(
            "Trigger '{}' not found in task '{}'",
            trigger_id, task_id
        ));
    };
    if !trigger.enabled {
        return Err(format!("Trigger '{}' is disabled", trigger_id));
    }

    let mut q = queue.lock().await;
    let old_generation = q.current_generation(&task_id);
    let valid_items: Vec<ScheduledItem> = q
        .take_task_items(&task_id)
        .into_iter()
        .filter(|item| item.generation == old_generation)
        .collect();
    let new_generation = q.bump_generation(&task_id);

    let mut target_rerolled = false;
    let mut new_next: Option<DateTime<Utc>> = None;
    for mut item in valid_items {
        if item.trigger_id == trigger_id {
            target_rerolled = true;
            match evaluate_next_occurrence(&trigger.kind, Utc::now()) {
                Some(next) => {
                    item.next_fire_at = next;
                    new_next = Some(next);
                }
                None => continue,
            }
        }
        item.generation = new_generation;
        q.push(item);
    }

    // 目标触发器原本不在队列中（例如 Network、已过期的 Once）：求值一次后按需入队
    if !target_rerolled {
        if let Some(next) = evaluate_next_occurrence(&trigger.kind, Utc::now()) {
            q.push(ScheduledItem {
                task_id,
                trigger_id,
                next_fire_at: next,
                generation: new_generation,
            });
            new_next = Some(next);
        }
    }

    Ok(new_next)
}
