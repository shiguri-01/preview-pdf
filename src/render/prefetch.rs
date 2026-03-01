use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};
use std::hash::Hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrefetchClass {
    CriticalCurrent,
    GuardReverse,
    DirectionalLead,
    Background,
}

impl PrefetchClass {
    fn rank(self) -> u8 {
        match self {
            Self::CriticalCurrent => 4,
            Self::GuardReverse => 3,
            Self::DirectionalLead => 2,
            Self::Background => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefetchQueueConfig {
    pub max_prefetch_depth: usize,
    pub guard_reverse_depth: u8,
    pub cancel_stale_generation: bool,
    pub dedupe_by_key: bool,
}

impl Default for PrefetchQueueConfig {
    fn default() -> Self {
        Self {
            max_prefetch_depth: 3,
            guard_reverse_depth: 1,
            cancel_stale_generation: true,
            dedupe_by_key: true,
        }
    }
}

impl PrefetchQueueConfig {
    pub fn effective_max_prefetch_depth(&self) -> usize {
        self.max_prefetch_depth.max(1)
    }

    pub fn effective_guard_reverse_depth(&self) -> usize {
        self.guard_reverse_depth as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueTaskMeta<K> {
    pub key: K,
    pub class: PrefetchClass,
    pub generation: u64,
}

#[derive(Debug)]
struct QueuedTask<K, T> {
    task: T,
    meta: QueueTaskMeta<K>,
    ordinal: u64,
}

impl<K: Eq, T> PartialEq for QueuedTask<K, T> {
    fn eq(&self, other: &Self) -> bool {
        self.meta.class == other.meta.class
            && self.meta.generation == other.meta.generation
            && self.ordinal == other.ordinal
    }
}

impl<K: Eq, T> Eq for QueuedTask<K, T> {}

impl<K: Eq, T> PartialOrd for QueuedTask<K, T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K: Eq, T> Ord for QueuedTask<K, T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.meta
            .class
            .rank()
            .cmp(&other.meta.class.rank())
            .then(self.meta.generation.cmp(&other.meta.generation))
            .then(other.ordinal.cmp(&self.ordinal))
    }
}

#[derive(Debug)]
pub struct PrefetchQueue<K, T> {
    tasks: BinaryHeap<QueuedTask<K, T>>,
    queued_keys: HashSet<K>,
    next_ordinal: u64,
    config: PrefetchQueueConfig,
}

impl<K, T> PrefetchQueue<K, T>
where
    K: Eq + Hash + Clone,
{
    pub fn new(config: PrefetchQueueConfig) -> Self {
        Self {
            tasks: BinaryHeap::new(),
            queued_keys: HashSet::new(),
            next_ordinal: 0,
            config,
        }
    }

    pub fn push(&mut self, task: T, meta: QueueTaskMeta<K>) -> bool {
        if self.config.dedupe_by_key && self.queued_keys.contains(&meta.key) {
            return false;
        }

        let queued_key = meta.key.clone();
        self.tasks.push(QueuedTask {
            task,
            meta,
            ordinal: self.next_ordinal,
        });
        self.next_ordinal = self.next_ordinal.saturating_add(1);

        if self.config.dedupe_by_key {
            self.queued_keys.insert(queued_key);
        }
        true
    }

    pub fn pop_next(&mut self) -> Option<T> {
        self.pop_next_with_meta().map(|(task, _)| task)
    }

    pub fn pop_next_with_meta(&mut self) -> Option<(T, QueueTaskMeta<K>)> {
        let item = self.tasks.pop()?;
        if self.config.dedupe_by_key {
            self.queued_keys.remove(&item.meta.key);
        }
        Some((item.task, item.meta))
    }

    pub fn cancel_stale_prefetch(&mut self, generation: u64) -> usize {
        if !self.config.cancel_stale_generation {
            return 0;
        }

        self.retain(|_, meta| {
            meta.generation >= generation
                || matches!(
                    meta.class,
                    PrefetchClass::CriticalCurrent | PrefetchClass::GuardReverse
                )
        })
    }

    pub fn clear(&mut self) -> usize {
        let removed = self.tasks.len();
        self.tasks.clear();
        self.queued_keys.clear();
        removed
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        if self.config.dedupe_by_key {
            return self.queued_keys.contains(key);
        }
        self.tasks.iter().any(|item| &item.meta.key == key)
    }

    pub fn retain<F>(&mut self, mut keep: F) -> usize
    where
        F: FnMut(&T, &QueueTaskMeta<K>) -> bool,
    {
        let mut removed = 0_usize;
        let mut kept = Vec::with_capacity(self.tasks.len());

        while let Some(item) = self.tasks.pop() {
            if keep(&item.task, &item.meta) {
                kept.push(item);
            } else {
                removed = removed.saturating_add(1);
            }
        }

        self.queued_keys.clear();
        for item in kept {
            if self.config.dedupe_by_key {
                self.queued_keys.insert(item.meta.key.clone());
            }
            self.tasks.push(item);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::{PrefetchClass, PrefetchQueue, PrefetchQueueConfig, QueueTaskMeta};

    fn meta(key: u8, class: PrefetchClass, generation: u64) -> QueueTaskMeta<u8> {
        QueueTaskMeta {
            key,
            class,
            generation,
        }
    }

    #[test]
    fn pop_order_follows_priority_and_generation() {
        let mut queue = PrefetchQueue::new(PrefetchQueueConfig::default());
        assert!(queue.push(1, meta(1, PrefetchClass::Background, 5)));
        assert!(queue.push(2, meta(2, PrefetchClass::DirectionalLead, 1)));
        assert!(queue.push(3, meta(3, PrefetchClass::DirectionalLead, 2)));
        assert!(queue.push(4, meta(4, PrefetchClass::GuardReverse, 1)));
        assert!(queue.push(5, meta(5, PrefetchClass::CriticalCurrent, 1)));

        assert_eq!(queue.pop_next(), Some(5));
        assert_eq!(queue.pop_next(), Some(4));
        assert_eq!(queue.pop_next(), Some(3));
        assert_eq!(queue.pop_next(), Some(2));
        assert_eq!(queue.pop_next(), Some(1));
        assert_eq!(queue.pop_next(), None);
    }

    #[test]
    fn fifo_within_same_class_and_generation() {
        let mut queue = PrefetchQueue::new(PrefetchQueueConfig::default());
        assert!(queue.push(10, meta(10, PrefetchClass::DirectionalLead, 7)));
        assert!(queue.push(11, meta(11, PrefetchClass::DirectionalLead, 7)));
        assert!(queue.push(12, meta(12, PrefetchClass::DirectionalLead, 7)));

        assert_eq!(queue.pop_next(), Some(10));
        assert_eq!(queue.pop_next(), Some(11));
        assert_eq!(queue.pop_next(), Some(12));
    }

    #[test]
    fn dedupe_by_key_skips_duplicate_tasks() {
        let mut queue = PrefetchQueue::new(PrefetchQueueConfig::default());
        assert!(queue.push(1, meta(42, PrefetchClass::Background, 1)));
        assert!(!queue.push(2, meta(42, PrefetchClass::CriticalCurrent, 2)));
        assert_eq!(queue.len(), 1);
        assert!(queue.contains_key(&42));
    }

    #[test]
    fn cancel_stale_prefetch_removes_only_lead_and_background() {
        let mut queue = PrefetchQueue::new(PrefetchQueueConfig::default());
        assert!(queue.push(1, meta(1, PrefetchClass::CriticalCurrent, 1)));
        assert!(queue.push(2, meta(2, PrefetchClass::GuardReverse, 1)));
        assert!(queue.push(3, meta(3, PrefetchClass::DirectionalLead, 1)));
        assert!(queue.push(4, meta(4, PrefetchClass::Background, 1)));
        assert!(queue.push(5, meta(5, PrefetchClass::DirectionalLead, 2)));

        let removed = queue.cancel_stale_prefetch(2);
        assert_eq!(removed, 2);

        let mut rest = Vec::new();
        while let Some(task) = queue.pop_next() {
            rest.push(task);
        }
        assert_eq!(rest, vec![1, 2, 5]);
    }

    #[test]
    fn guard_reverse_depth_config_supports_0_1_2() {
        let mut cfg = PrefetchQueueConfig {
            guard_reverse_depth: 0,
            ..Default::default()
        };
        assert_eq!(cfg.effective_guard_reverse_depth(), 0);

        cfg.guard_reverse_depth = 1;
        assert_eq!(cfg.effective_guard_reverse_depth(), 1);

        cfg.guard_reverse_depth = 2;
        assert_eq!(cfg.effective_guard_reverse_depth(), 2);
    }
}
