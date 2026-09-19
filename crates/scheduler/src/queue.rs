use chrono::{DateTime, Utc};
use easyjob_common::{TaskId, TriggerId};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ScheduledItem {
    pub task_id: TaskId,
    pub trigger_id: TriggerId,
    pub next_fire_at: DateTime<Utc>,
    pub generation: u64,
}

impl Ord for ScheduledItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for MinHeap (earliest time first)
        other
            .next_fire_at
            .cmp(&self.next_fire_at)
            .then_with(|| self.task_id.cmp(&other.task_id))
            .then_with(|| self.trigger_id.cmp(&other.trigger_id))
            .then_with(|| self.generation.cmp(&other.generation))
    }
}

impl PartialOrd for ScheduledItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Default, Debug)]
pub struct ScheduleQueue {
    heap: BinaryHeap<ScheduledItem>,
    generations: HashMap<TaskId, u64>,
}

impl ScheduleQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, item: ScheduledItem) {
        let current_gen = self.generations.entry(item.task_id).or_insert(1);
        if item.generation == *current_gen {
            self.heap.push(item);
        }
    }

    pub fn pop(&mut self) -> Option<ScheduledItem> {
        while let Some(item) = self.heap.pop() {
            if self.is_valid(&item) {
                return Some(item);
            }
        }
        None
    }

    pub fn peek(&self) -> Option<&ScheduledItem> {
        self.heap.peek()
    }

    pub fn peek_valid(&mut self) -> Option<&ScheduledItem> {
        while let Some(top) = self.heap.peek() {
            if self.is_valid(top) {
                return self.heap.peek();
            }
            self.heap.pop();
        }
        None
    }

    pub fn bump_generation(&mut self, task_id: &TaskId) -> u64 {
        let gen = self.generations.entry(*task_id).or_insert(1);
        *gen += 1;
        *gen
    }

    pub fn current_generation(&self, task_id: &TaskId) -> u64 {
        self.generations.get(task_id).copied().unwrap_or(1)
    }

    pub fn is_valid(&self, item: &ScheduledItem) -> bool {
        self.generations.get(&item.task_id).copied().unwrap_or(1) == item.generation
    }

    pub fn clear(&mut self) {
        self.heap.clear();
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// 返回每个触发器的下次触发时间，只统计当前 generation 有效的条目。
    /// 同一触发器存在多条有效条目时取最早的一条。
    pub fn next_fire_by_trigger(&self) -> HashMap<(TaskId, TriggerId), DateTime<Utc>> {
        let mut result: HashMap<(TaskId, TriggerId), DateTime<Utc>> = HashMap::new();
        for item in self.heap.iter() {
            if !self.is_valid(item) {
                continue;
            }
            let key = (item.task_id, item.trigger_id);
            match result.get_mut(&key) {
                Some(existing) => {
                    if item.next_fire_at < *existing {
                        *existing = item.next_fire_at;
                    }
                }
                None => {
                    result.insert(key, item.next_fire_at);
                }
            }
        }
        result
    }

    /// 取出某任务的全部条目（其余任务的条目保留在堆中），供重摇后按新 generation 重新入队。
    pub fn take_task_items(&mut self, task_id: &TaskId) -> Vec<ScheduledItem> {
        let heap = std::mem::take(&mut self.heap);
        let mut taken = Vec::new();
        let mut kept = BinaryHeap::new();
        for item in heap {
            if item.task_id == *task_id {
                taken.push(item);
            } else {
                kept.push(item);
            }
        }
        self.heap = kept;
        taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn base() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-19T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn next_fire_by_trigger_returns_earliest_valid_entry_per_trigger() {
        let mut queue = ScheduleQueue::new();
        let task_a = TaskId::new();
        let task_b = TaskId::new();
        let trigger_x = TriggerId::new();
        let trigger_y = TriggerId::new();
        let t = base();
        let mk =
            |task_id: TaskId, trigger_id: TriggerId, offset: i64, generation: u64| ScheduledItem {
                task_id,
                trigger_id,
                next_fire_at: t + Duration::seconds(offset),
                generation,
            };

        // 同一触发器两条有效条目 → 取最早的一条
        queue.push(mk(task_a, trigger_x, 60, 1));
        queue.push(mk(task_a, trigger_x, 30, 1));
        // 同任务的另一个触发器
        queue.push(mk(task_a, trigger_y, 120, 1));
        // 另一个任务
        queue.push(mk(task_b, trigger_x, 10, 1));

        let map = queue.next_fire_by_trigger();
        assert_eq!(map.len(), 3);
        assert_eq!(map[&(task_a, trigger_x)], t + Duration::seconds(30));
        assert_eq!(map[&(task_a, trigger_y)], t + Duration::seconds(120));
        assert_eq!(map[&(task_b, trigger_x)], t + Duration::seconds(10));
    }

    #[test]
    fn next_fire_by_trigger_ignores_stale_generation() {
        let mut queue = ScheduleQueue::new();
        let task_id = TaskId::new();
        let trigger_id = TriggerId::new();
        let t = base();

        queue.push(ScheduledItem {
            task_id,
            trigger_id,
            next_fire_at: t + Duration::seconds(5),
            generation: 1,
        });
        queue.bump_generation(&task_id); // 旧条目失效
        queue.push(ScheduledItem {
            task_id,
            trigger_id,
            next_fire_at: t + Duration::seconds(90),
            generation: 2,
        });

        let map = queue.next_fire_by_trigger();
        assert_eq!(map.len(), 1);
        assert_eq!(map[&(task_id, trigger_id)], t + Duration::seconds(90));
    }

    #[test]
    fn take_task_items_removes_only_target_task_and_keeps_others() {
        let mut queue = ScheduleQueue::new();
        let task_a = TaskId::new();
        let task_b = TaskId::new();
        let t = base();

        queue.push(ScheduledItem {
            task_id: task_a,
            trigger_id: TriggerId::new(),
            next_fire_at: t + Duration::seconds(1),
            generation: 1,
        });
        queue.push(ScheduledItem {
            task_id: task_a,
            trigger_id: TriggerId::new(),
            next_fire_at: t + Duration::seconds(2),
            generation: 1,
        });
        queue.push(ScheduledItem {
            task_id: task_b,
            trigger_id: TriggerId::new(),
            next_fire_at: t + Duration::seconds(3),
            generation: 1,
        });

        let taken = queue.take_task_items(&task_a);
        assert_eq!(taken.len(), 2);
        assert!(taken.iter().all(|item| item.task_id == task_a));
        assert_eq!(queue.len(), 1);
        let remaining = queue.peek().expect("task_b item remains");
        assert_eq!(remaining.task_id, task_b);
    }
}
