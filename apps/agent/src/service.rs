use async_trait::async_trait;
use easyjob_common::{ExecutionId, Result, TaskId};
use easyjob_domain::execution::{Execution, ExecutionStatus};
use easyjob_domain::task::Task;
use easyjob_domain::trigger::TriggerKind;
use easyjob_executor::manager::ExecutionManager;
use easyjob_executor::runner::ProcessRunner;
use easyjob_ipc::protocol::{AgentStatus, IpcEvent, IpcRequest, IpcResponse};
use easyjob_ipc::server::{IpcServer, RequestHandler};
use easyjob_persistence::db::init_pool;
use easyjob_persistence::execution_repo::{ExecutionRepository, SqliteExecutionRepository};
use easyjob_persistence::recovery::recover_dangling_executions;
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::{Scheduler, SchedulerCommand, TriggerEvent};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex, Notify};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub struct AgentService {
    task_repo: Arc<SqliteTaskRepository>,
    exec_repo: Arc<SqliteExecutionRepository>,
    exec_manager: Arc<ExecutionManager>,
    scheduler_tx: mpsc::Sender<SchedulerCommand>,
    event_rx: mpsc::Receiver<TriggerEvent>,
    scheduler_handle: tokio::task::JoinHandle<()>,
    ipc_server: IpcServer,
    ipc_path: PathBuf,
    start_time: Instant,
    shutdown_notify: Arc<Notify>,
    active_executions: Arc<Mutex<HashMap<ExecutionId, CancellationToken>>>,
}

impl AgentService {
    pub async fn init(db_url: &str, ipc_path: &Path, max_concurrent: usize) -> Result<Self> {
        let pool = init_pool(db_url).await?;
        let _ = recover_dangling_executions(&pool).await?;

        let task_repo = Arc::new(SqliteTaskRepository::new(pool.clone()));
        let exec_repo = Arc::new(SqliteExecutionRepository::new(pool.clone()));
        let exec_manager = Arc::new(ExecutionManager::new(max_concurrent));

        let (event_tx, event_rx) = mpsc::channel(100);
        let (scheduler, scheduler_cmd_rx) = Scheduler::new(event_tx);
        let scheduler_tx = scheduler.sender();

        // Spawn scheduler loop first before loading tasks to avoid bounded channel deadlock
        let sched_queue = scheduler.queue();
        let sched_event_tx = scheduler.event_sender();
        let scheduler_handle = tokio::spawn(Scheduler::run(
            sched_queue,
            scheduler_cmd_rx,
            sched_event_tx,
        ));

        // Load tasks into scheduler
        let enabled_tasks = task_repo.find_all_enabled().await?;
        for task in enabled_tasks {
            let has_agent_started = task
                .triggers
                .iter()
                .any(|tr| tr.enabled && matches!(tr.kind, TriggerKind::AgentStarted));
            let _ = scheduler_tx
                .send(SchedulerCommand::add_task(task.clone()))
                .await;
            if has_agent_started {
                let _ = scheduler_tx
                    .send(SchedulerCommand::TriggerNow(task.id))
                    .await;
            }
        }

        let shutdown_notify = Arc::new(Notify::new());
        let active_executions = Arc::new(Mutex::new(HashMap::new()));
        let start_time = Instant::now();

        let handler = Arc::new(AgentRpcHandler {
            task_repo: task_repo.clone(),
            exec_repo: exec_repo.clone(),
            exec_manager: exec_manager.clone(),
            scheduler_tx: scheduler_tx.clone(),
            start_time,
            shutdown_notify: shutdown_notify.clone(),
            active_executions: active_executions.clone(),
        });

        let ipc_server = IpcServer::bind(ipc_path, handler).await?;

        Ok(Self {
            task_repo,
            exec_repo,
            exec_manager,
            scheduler_tx,
            event_rx,
            scheduler_handle,
            ipc_server,
            ipc_path: ipc_path.to_path_buf(),
            start_time,
            shutdown_notify,
            active_executions,
        })
    }

    pub fn task_repository(&self) -> Arc<SqliteTaskRepository> {
        self.task_repo.clone()
    }

    pub fn execution_repository(&self) -> Arc<SqliteExecutionRepository> {
        self.exec_repo.clone()
    }

    pub fn execution_manager(&self) -> Arc<ExecutionManager> {
        self.exec_manager.clone()
    }

    pub fn active_executions(&self) -> Arc<Mutex<HashMap<ExecutionId, CancellationToken>>> {
        self.active_executions.clone()
    }

    pub fn ipc_path(&self) -> &Path {
        &self.ipc_path
    }

    pub fn start_time(&self) -> Instant {
        self.start_time
    }

    pub async fn run(mut self) -> Result<()> {
        let event_tx = self.ipc_server.event_sender();
        let task_repo = self.task_repo.clone();
        let exec_repo = self.exec_repo.clone();
        let exec_manager = self.exec_manager.clone();
        let active_executions = self.active_executions.clone();
        let cancel_token = CancellationToken::new();
        let dispatcher_cancel = cancel_token.clone();
        let exec_cancel_token = cancel_token.clone();

        // Spawn trigger event listener & execution dispatcher
        let dispatcher_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = dispatcher_cancel.cancelled() => break,
                    maybe_event = self.event_rx.recv() => {
                        let trigger_event = match maybe_event {
                            Some(ev) => ev,
                            None => break,
                        };

                        let task = match task_repo.find_by_id(&trigger_event.task_id).await {
                            Ok(Some(t)) => t,
                            _ => continue,
                        };

                        let permit = match exec_manager.global_semaphore().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                tracing::warn!(
                                    "Global concurrency limit reached, skipping execution for task {}",
                                    task.id
                                );
                                continue;
                            }
                        };

                        let acquired = exec_manager
                            .try_acquire_slot(&task.id, task.execution_policy.concurrency_policy)
                            .await;
                        if !acquired {
                            drop(permit);
                            continue;
                        }

                        let e_repo = exec_repo.clone();
                        let e_manager = exec_manager.clone();
                        let active_execs = active_executions.clone();
                        let ev_tx = event_tx.clone();
                        let action_cancel = exec_cancel_token.child_token();

                        tokio::spawn(async move {
                            let _permit = permit;
                            let mut exec = Execution::new(
                                task.id,
                                trigger_event.trigger_id,
                                Some(trigger_event.scheduled_at),
                            );
                            exec.status = ExecutionStatus::Running;
                            exec.started_at = chrono::Utc::now();
                            if let Err(e) = e_repo.create_run(&exec).await {
                                error!("Failed to create execution run {}: {:?}", exec.id, e);
                                e_manager.release_slot(&task.id).await;
                                return;
                            }

                            active_execs
                                .lock()
                                .await
                                .insert(exec.id, action_cancel.clone());

                            let _ = ev_tx.send(IpcEvent::new(
                                "execution.started",
                                serde_json::json!({
                                    "execution_id": exec.id,
                                    "task_id": exec.task_id,
                                }),
                            ));

                            let mut final_status = ExecutionStatus::Succeeded;
                            let mut exit_code = Some(0);
                            let mut error_message = None;

                            for action in &task.actions {
                                if !action.enabled {
                                    continue;
                                }
                                let res = ProcessRunner::run_action(
                                    action,
                                    task.working_directory.as_ref(),
                                    &task.environment,
                                    task.execution_policy.timeout_secs,
                                    action_cancel.clone(),
                                )
                                .await;

                                match res {
                                    Ok(run_res) => {
                                        if !run_res.stdout.is_empty() {
                                            let _ = e_repo
                                                .append_output(&exec.id, "stdout", &run_res.stdout)
                                                .await;
                                            let _ = ev_tx.send(IpcEvent::new(
                                                "execution.output",
                                                serde_json::json!({
                                                    "execution_id": exec.id,
                                                    "task_id": exec.task_id,
                                                    "stream": "stdout",
                                                    "content": run_res.stdout,
                                                }),
                                            ));
                                        }
                                        if !run_res.stderr.is_empty() {
                                            let _ = e_repo
                                                .append_output(&exec.id, "stderr", &run_res.stderr)
                                                .await;
                                            let _ = ev_tx.send(IpcEvent::new(
                                                "execution.output",
                                                serde_json::json!({
                                                    "execution_id": exec.id,
                                                    "task_id": exec.task_id,
                                                    "stream": "stderr",
                                                    "content": run_res.stderr,
                                                }),
                                            ));
                                        }
                                        exit_code = run_res.exit_code;
                                        if run_res.status != ExecutionStatus::Succeeded {
                                            final_status = run_res.status;
                                            error_message = run_res.error_message;
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        final_status = ExecutionStatus::Failed;
                                        error_message = Some(e.to_string());
                                        break;
                                    }
                                }
                            }

                            let finished_at = chrono::Utc::now();
                            let duration_ms = (finished_at - exec.started_at)
                                .num_milliseconds()
                                .max(0) as u64;

                            exec.status = final_status;
                            exec.finished_at = Some(finished_at);
                            exec.duration_ms = Some(duration_ms);
                            exec.exit_code = exit_code;
                            exec.error_message = error_message;

                            if let Err(e) = e_repo.update_run(&exec).await {
                                error!("Failed to update execution run {}: {:?}", exec.id, e);
                            }

                            let _ = ev_tx.send(IpcEvent::new(
                                "execution.finished",
                                serde_json::json!({
                                    "execution_id": exec.id,
                                    "task_id": exec.task_id,
                                    "status": exec.status,
                                    "exit_code": exec.exit_code,
                                }),
                            ));

                            active_execs.lock().await.remove(&exec.id);
                            e_manager.release_slot(&task.id).await;
                        });
                    }
                }
            }
        });

        let shutdown = self.shutdown_notify.clone();
        tokio::select! {
            res = self.ipc_server.run() => {
                if let Err(e) = res {
                    error!("IPC server run error: {:?}", e);
                }
            }
            _ = shutdown.notified() => {
                info!("Agent service shutdown notification received");
            }
        }

        cancel_token.cancel();
        let _ = dispatcher_handle.await;

        // Await in-flight executions to finish with timeout
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while self.exec_manager.total_running_count().await > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await;

        let _ = self.scheduler_tx.send(SchedulerCommand::Shutdown).await;
        let _ = self.scheduler_handle.await;

        info!("Agent service successfully shut down");
        Ok(())
    }
}

pub struct AgentRpcHandler {
    pub(crate) task_repo: Arc<SqliteTaskRepository>,
    pub(crate) exec_repo: Arc<SqliteExecutionRepository>,
    pub(crate) exec_manager: Arc<ExecutionManager>,
    pub(crate) scheduler_tx: mpsc::Sender<SchedulerCommand>,
    pub(crate) start_time: Instant,
    pub(crate) shutdown_notify: Arc<Notify>,
    pub(crate) active_executions: Arc<Mutex<HashMap<ExecutionId, CancellationToken>>>,
}

impl AgentRpcHandler {
    pub fn execution_repository(&self) -> Arc<SqliteExecutionRepository> {
        self.exec_repo.clone()
    }
}

#[async_trait]
impl RequestHandler for AgentRpcHandler {
    async fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        match req.method.as_str() {
            "agent.status" => {
                let tasks = self.task_repo.find_all_enabled().await.unwrap_or_default();
                let status = AgentStatus {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    uptime_secs: self.start_time.elapsed().as_secs(),
                    active_tasks: tasks.len(),
                    running_executions: self.exec_manager.total_running_count().await,
                };
                match serde_json::to_value(status) {
                    Ok(v) => IpcResponse::success(req.id, v),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "agent.shutdown" => {
                self.shutdown_notify.notify_waiters();
                IpcResponse::success(req.id, serde_json::json!(true))
            }
            "task.list" => match self.task_repo.find_all_enabled().await {
                Ok(tasks) => match serde_json::to_value(tasks) {
                    Ok(v) => IpcResponse::success(req.id, v),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                },
                Err(e) => IpcResponse::error(req.id, e.to_string()),
            },
            "task.get" => {
                let id_val = if req.params.is_object() && req.params.get("id").is_some() {
                    &req.params["id"]
                } else {
                    &req.params
                };
                let id: TaskId = match serde_json::from_value(id_val.clone()) {
                    Ok(id) => id,
                    Err(e) => {
                        return IpcResponse::error(
                            req.id,
                            format!("Invalid task id parameter: {}", e),
                        )
                    }
                };
                match self.task_repo.find_by_id(&id).await {
                    Ok(Some(task)) => match serde_json::to_value(task) {
                        Ok(v) => IpcResponse::success(req.id, v),
                        Err(e) => IpcResponse::error(req.id, e.to_string()),
                    },
                    Ok(None) => IpcResponse::error(req.id, format!("Task '{}' not found", id)),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "task.save" => {
                let task_val = if req.params.is_object() && req.params.get("task").is_some() {
                    &req.params["task"]
                } else {
                    &req.params
                };
                let task: Task = match serde_json::from_value(task_val.clone()) {
                    Ok(t) => t,
                    Err(e) => {
                        return IpcResponse::error(req.id, format!("Invalid task parameter: {}", e))
                    }
                };
                match self.task_repo.save(&task).await {
                    Ok(()) => {
                        let _ = self
                            .scheduler_tx
                            .send(SchedulerCommand::add_task(task.clone()))
                            .await;
                        match serde_json::to_value(task) {
                            Ok(v) => IpcResponse::success(req.id, v),
                            Err(e) => IpcResponse::error(req.id, e.to_string()),
                        }
                    }
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "task.delete" => {
                let id_val = if req.params.is_object() && req.params.get("id").is_some() {
                    &req.params["id"]
                } else {
                    &req.params
                };
                let id: TaskId = match serde_json::from_value(id_val.clone()) {
                    Ok(id) => id,
                    Err(e) => {
                        return IpcResponse::error(
                            req.id,
                            format!("Invalid task id parameter: {}", e),
                        )
                    }
                };
                match self.task_repo.delete(&id).await {
                    Ok(()) => {
                        let _ = self
                            .scheduler_tx
                            .send(SchedulerCommand::RemoveTask(id))
                            .await;
                        IpcResponse::success(req.id, serde_json::json!(true))
                    }
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "task.trigger_now" => {
                let id_val = if req.params.is_object() && req.params.get("id").is_some() {
                    &req.params["id"]
                } else {
                    &req.params
                };
                let id: TaskId = match serde_json::from_value(id_val.clone()) {
                    Ok(id) => id,
                    Err(e) => {
                        return IpcResponse::error(
                            req.id,
                            format!("Invalid task id parameter: {}", e),
                        )
                    }
                };
                match self
                    .scheduler_tx
                    .send(SchedulerCommand::TriggerNow(id))
                    .await
                {
                    Ok(()) => IpcResponse::success(req.id, serde_json::json!(true)),
                    Err(e) => {
                        IpcResponse::error(req.id, format!("Failed to send trigger command: {}", e))
                    }
                }
            }
            "execution.list" => {
                let limit = if req.params.is_object() && req.params.get("limit").is_some() {
                    req.params["limit"].as_u64().unwrap_or(50) as u32
                } else if req.params.is_number() {
                    req.params.as_u64().unwrap_or(50) as u32
                } else {
                    50
                };
                match self.exec_repo.find_recent_runs(limit).await {
                    Ok(runs) => match serde_json::to_value(runs) {
                        Ok(v) => IpcResponse::success(req.id, v),
                        Err(e) => IpcResponse::error(req.id, e.to_string()),
                    },
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "execution.get" => {
                let id_val = if req.params.is_object() && req.params.get("id").is_some() {
                    &req.params["id"]
                } else {
                    &req.params
                };
                let id: ExecutionId = match serde_json::from_value(id_val.clone()) {
                    Ok(id) => id,
                    Err(e) => {
                        return IpcResponse::error(
                            req.id,
                            format!("Invalid execution id parameter: {}", e),
                        )
                    }
                };
                match self.exec_repo.find_run_by_id(&id).await {
                    Ok(Some(run)) => match serde_json::to_value(run) {
                        Ok(v) => IpcResponse::success(req.id, v),
                        Err(e) => IpcResponse::error(req.id, e.to_string()),
                    },
                    Ok(None) => IpcResponse::error(req.id, format!("Execution '{}' not found", id)),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "execution.cancel" => {
                let id_val = if req.params.is_object() && req.params.get("id").is_some() {
                    &req.params["id"]
                } else {
                    &req.params
                };
                let id: ExecutionId = match serde_json::from_value(id_val.clone()) {
                    Ok(id) => id,
                    Err(e) => {
                        return IpcResponse::error(
                            req.id,
                            format!("Invalid execution id parameter: {}", e),
                        )
                    }
                };
                let active = self.active_executions.lock().await;
                if let Some(token) = active.get(&id) {
                    token.cancel();
                    IpcResponse::success(req.id, serde_json::json!(true))
                } else {
                    IpcResponse::error(
                        req.id,
                        format!("Execution '{}' is not actively running", id),
                    )
                }
            }
            _ => IpcResponse::error(req.id, format!("Method '{}' not found", req.method)),
        }
    }
}
