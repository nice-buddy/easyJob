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

#[derive(Debug)]
pub enum SchedulerCommand {
    AddTask(Box<Task>),
    RemoveTask(TaskId),
    TriggerNow(TaskId),
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
