-- 支撑「每个任务最近一次执行」的窗口函数查询：
-- ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY started_at DESC, id DESC)
CREATE INDEX IF NOT EXISTS idx_runs_task_id_started_at ON task_runs(task_id, started_at);