use easyjob_common::TaskId;
use easyjob_domain::policy::ConcurrencyPolicy;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRequest {
    pub task_id: TaskId,
    pub policy: ConcurrencyPolicy,
}

impl ExecutionRequest {
    pub fn new(task_id: TaskId, policy: ConcurrencyPolicy) -> Self {
        Self { task_id, policy }
    }
}

pub struct ExecutionManager {
    global_limiter: Arc<Semaphore>,
    task_running_counts: Arc<Mutex<HashMap<TaskId, usize>>>,
}

impl ExecutionManager {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            global_limiter: Arc::new(Semaphore::new(max_concurrent)),
            task_running_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn try_acquire_slot(&self, task_id: &TaskId, policy: ConcurrencyPolicy) -> bool {
        let mut counts = self.task_running_counts.lock().await;
        let count = counts.entry(*task_id).or_insert(0);

        match policy {
            ConcurrencyPolicy::AllowParallel => {
                *count += 1;
                true
            }
            ConcurrencyPolicy::SkipIfRunning => {
                if *count > 0 {
                    false
                } else {
                    *count += 1;
                    true
                }
            }
            ConcurrencyPolicy::QueueOne => {
                if *count >= 2 {
                    false
                } else {
                    *count += 1;
                    true
                }
            }
        }
    }

    pub async fn try_acquire_request(&self, request: &ExecutionRequest) -> bool {
        self.try_acquire_slot(&request.task_id, request.policy)
            .await
    }

    pub async fn release_slot(&self, task_id: &TaskId) {
        let mut counts = self.task_running_counts.lock().await;
        if let Some(count) = counts.get_mut(task_id) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                counts.remove(task_id);
            }
        }
    }

    pub async fn running_count(&self, task_id: &TaskId) -> usize {
        let counts = self.task_running_counts.lock().await;
        counts.get(task_id).copied().unwrap_or(0)
    }

    pub fn global_semaphore(&self) -> Arc<Semaphore> {
        self.global_limiter.clone()
    }
}
