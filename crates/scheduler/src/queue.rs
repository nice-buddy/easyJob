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
}
