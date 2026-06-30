use std::hash::Hash;
use std::num::NonZeroUsize;

use lru::LruCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CacheLimits {
    pub(crate) max_entries: usize,
    pub(crate) memory_budget_bytes: usize,
}

impl CacheLimits {
    pub(crate) fn new(max_entries: usize, memory_budget_bytes: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            memory_budget_bytes: memory_budget_bytes.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheCounters {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OversizePolicy {
    Reject,
    Admit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvictionPolicy<'a, K> {
    Normal,
    Protect(&'a [K]),
    RejectIfEvictionRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InsertPolicy<'a, K> {
    pub(crate) oversize: OversizePolicy,
    pub(crate) eviction: EvictionPolicy<'a, K>,
}

impl<K> InsertPolicy<'_, K> {
    pub(crate) const NORMAL: Self = Self {
        oversize: OversizePolicy::Reject,
        eviction: EvictionPolicy::Normal,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovedEntry<K, V> {
    pub(crate) key: K,
    pub(crate) value: V,
    pub(crate) cost_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InsertOutcome<K, V> {
    pub(crate) inserted: bool,
    pub(crate) replaced: Option<RemovedEntry<K, V>>,
    pub(crate) evicted: Vec<RemovedEntry<K, V>>,
}

impl<K, V> InsertOutcome<K, V> {
    fn rejected() -> Self {
        Self {
            inserted: false,
            replaced: None,
            evicted: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct CacheEntry<V> {
    value: V,
    cost_bytes: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct BudgetedLruCache<K: Eq + Hash, V> {
    limits: CacheLimits,
    memory_bytes: usize,
    entries: LruCache<K, CacheEntry<V>>,
    counters: CacheCounters,
}

impl<K, V> BudgetedLruCache<K, V>
where
    K: Eq + Hash + Clone,
{
    pub(crate) fn new(limits: CacheLimits) -> Self {
        let limits = CacheLimits::new(limits.max_entries, limits.memory_budget_bytes);
        Self {
            limits,
            memory_bytes: 0,
            entries: LruCache::new(
                NonZeroUsize::new(limits.max_entries.saturating_mul(2).saturating_add(1))
                    .expect("cache entries is non-zero"),
            ),
            counters: CacheCounters::default(),
        }
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        if self.entries.peek(key).is_some() {
            self.counters.hits += 1;
            return self.entries.get(key).map(|entry| &entry.value);
        }
        self.counters.misses += 1;
        None
    }

    pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if self.entries.peek(key).is_some() {
            self.counters.hits += 1;
            return self.entries.get_mut(key).map(|entry| &mut entry.value);
        }
        self.counters.misses += 1;
        None
    }

    pub(crate) fn peek(&self, key: &K) -> Option<&V> {
        self.entries.peek(key).map(|entry| &entry.value)
    }

    pub(crate) fn peek_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries.peek_mut(key).map(|entry| &mut entry.value)
    }

    pub(crate) fn insert(
        &mut self,
        key: K,
        value: V,
        cost_bytes: usize,
        policy: InsertPolicy<'_, K>,
    ) -> InsertOutcome<K, V> {
        let cost_bytes = cost_bytes.max(1);
        if cost_bytes > self.limits.memory_budget_bytes && policy.oversize == OversizePolicy::Reject
        {
            return InsertOutcome::rejected();
        }
        if self.eviction_required_for_insert(&key, cost_bytes, policy.oversize)
            && policy.eviction == EvictionPolicy::RejectIfEvictionRequired
        {
            return InsertOutcome::rejected();
        }

        let protected = match policy.eviction {
            EvictionPolicy::Protect(keys) => keys,
            EvictionPolicy::Normal | EvictionPolicy::RejectIfEvictionRequired => &[],
        };
        if matches!(policy.eviction, EvictionPolicy::Protect(_))
            && !self.can_admit_with_protected(&key, cost_bytes, policy.oversize, protected)
        {
            return InsertOutcome::rejected();
        }
        let replaced = self.pop_entry(&key);
        self.memory_bytes = self.memory_bytes.saturating_add(cost_bytes);
        let mut evicted = Vec::new();
        let _ = self
            .entries
            .push(key.clone(), CacheEntry { value, cost_bytes });

        let oversize_admitted = cost_bytes > self.limits.memory_budget_bytes
            && policy.oversize == OversizePolicy::Admit;
        let eviction_forbidden = policy.eviction == EvictionPolicy::RejectIfEvictionRequired;

        if oversize_admitted && !eviction_forbidden {
            evicted.extend(self.evict_unprotected_except(&key, protected));
        } else if !oversize_admitted || !eviction_forbidden {
            evicted.extend(self.evict_while_needed(protected));
        }

        InsertOutcome {
            inserted: true,
            replaced,
            evicted,
        }
    }

    pub(crate) fn remove(&mut self, key: &K) -> Option<RemovedEntry<K, V>> {
        self.pop_entry(key)
    }

    pub(crate) fn remove_where(
        &mut self,
        mut should_remove: impl FnMut(&K, &V) -> bool,
    ) -> Vec<RemovedEntry<K, V>> {
        let keys = self
            .entries
            .iter()
            .filter_map(|(key, entry)| should_remove(key, &entry.value).then_some(key.clone()))
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| self.remove(&key))
            .collect()
    }

    pub(crate) fn clear(&mut self) -> Vec<RemovedEntry<K, V>> {
        let keys = self
            .entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let removed = keys
            .into_iter()
            .filter_map(|key| self.pop_entry(&key))
            .collect::<Vec<_>>();
        self.entries.clear();
        self.memory_bytes = 0;
        removed
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn max_entries(&self) -> usize {
        self.limits.max_entries
    }

    pub(crate) fn memory_budget_bytes(&self) -> usize {
        self.limits.memory_budget_bytes
    }

    pub(crate) fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    pub(crate) fn counters(&self) -> CacheCounters {
        self.counters
    }

    pub(crate) fn add_evictions(&mut self, count: u64) {
        self.counters.evictions = self.counters.evictions.saturating_add(count);
    }

    pub(crate) fn hit_rate(&self) -> f64 {
        let lookups = self.counters.hits + self.counters.misses;
        if lookups == 0 {
            return 0.0;
        }
        self.counters.hits as f64 / lookups as f64
    }

    fn eviction_required_for_insert(
        &self,
        key: &K,
        cost_bytes: usize,
        oversize: OversizePolicy,
    ) -> bool {
        let replaced_bytes = self.entries.peek(key).map_or(0, |entry| entry.cost_bytes);
        let projected_len = if self.entries.peek(key).is_some() {
            self.entries.len()
        } else {
            self.entries.len() + 1
        };
        let projected_memory = self
            .memory_bytes
            .saturating_sub(replaced_bytes)
            .saturating_add(cost_bytes);
        projected_len > self.limits.max_entries
            || (projected_memory > self.limits.memory_budget_bytes
                && !(cost_bytes > self.limits.memory_budget_bytes
                    && oversize == OversizePolicy::Admit))
    }

    fn can_admit_with_protected(
        &self,
        key: &K,
        cost_bytes: usize,
        oversize: OversizePolicy,
        protected: &[K],
    ) -> bool {
        let replaced_bytes = self.entries.peek(key).map_or(0, |entry| entry.cost_bytes);
        let mut projected_len = if self.entries.peek(key).is_some() {
            self.entries.len()
        } else {
            self.entries.len() + 1
        };
        let mut projected_memory = self
            .memory_bytes
            .saturating_sub(replaced_bytes)
            .saturating_add(cost_bytes);
        for (entry_key, entry) in self.entries.iter() {
            if self.fits_limits(projected_len, projected_memory, cost_bytes, oversize) {
                return true;
            }
            if entry_key == key || protected.contains(entry_key) {
                continue;
            }
            projected_len = projected_len.saturating_sub(1);
            projected_memory = projected_memory.saturating_sub(entry.cost_bytes);
        }
        self.fits_limits(projected_len, projected_memory, cost_bytes, oversize)
    }

    fn fits_limits(
        &self,
        projected_len: usize,
        projected_memory: usize,
        cost_bytes: usize,
        oversize: OversizePolicy,
    ) -> bool {
        projected_len <= self.limits.max_entries
            && (projected_memory <= self.limits.memory_budget_bytes
                || (cost_bytes > self.limits.memory_budget_bytes
                    && oversize == OversizePolicy::Admit))
    }

    fn evict_while_needed(&mut self, protected: &[K]) -> Vec<RemovedEntry<K, V>> {
        let mut removed = Vec::new();
        while self.entries.len() > self.limits.max_entries
            || self.memory_bytes > self.limits.memory_budget_bytes
        {
            let Some(entry) = self.pop_lru_unprotected(protected) else {
                break;
            };
            self.counters.evictions += 1;
            removed.push(entry);
        }
        removed
    }

    fn evict_unprotected_except(
        &mut self,
        inserted_key: &K,
        protected: &[K],
    ) -> Vec<RemovedEntry<K, V>> {
        let keys = self
            .entries
            .iter()
            .filter_map(|(key, _)| {
                (key != inserted_key && !protected.contains(key)).then_some(key.clone())
            })
            .collect::<Vec<_>>();
        let mut removed = Vec::new();
        for key in keys {
            if let Some(entry) = self.pop_entry(&key) {
                self.counters.evictions += 1;
                removed.push(entry);
            }
        }
        removed
    }

    fn pop_lru_unprotected(&mut self, protected: &[K]) -> Option<RemovedEntry<K, V>> {
        let mut protected_entries = Vec::new();
        loop {
            match self.entries.pop_lru() {
                Some((key, entry)) if protected.contains(&key) => {
                    protected_entries.push((key, entry));
                }
                Some((key, entry)) => {
                    for (protected_key, protected_entry) in protected_entries.into_iter().rev() {
                        let _ = self.entries.push(protected_key.clone(), protected_entry);
                        let _ = self.entries.demote(&protected_key);
                    }
                    let removed = self.removed_entry(key, entry);
                    self.memory_bytes = self.memory_bytes.saturating_sub(removed.cost_bytes);
                    return Some(removed);
                }
                None => {
                    for (protected_key, protected_entry) in protected_entries.into_iter().rev() {
                        let _ = self.entries.push(protected_key.clone(), protected_entry);
                        let _ = self.entries.demote(&protected_key);
                    }
                    return None;
                }
            }
        }
    }

    fn pop_entry(&mut self, key: &K) -> Option<RemovedEntry<K, V>> {
        let entry = self.entries.pop(key)?;
        let removed = self.removed_entry(key.clone(), entry);
        self.memory_bytes = self.memory_bytes.saturating_sub(removed.cost_bytes);
        Some(removed)
    }

    fn removed_entry(&self, key: K, entry: CacheEntry<V>) -> RemovedEntry<K, V> {
        RemovedEntry {
            key,
            value: entry.value,
            cost_bytes: entry.cost_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BudgetedLruCache, CacheLimits, EvictionPolicy, InsertPolicy, OversizePolicy};

    fn cache(max_entries: usize, memory_budget_bytes: usize) -> BudgetedLruCache<u8, &'static str> {
        BudgetedLruCache::new(CacheLimits::new(max_entries, memory_budget_bytes))
    }

    #[test]
    fn get_tracks_hit_rate_and_promotes_lru_order() {
        let mut cache = cache(2, 100);
        let _ = cache.insert(1, "one", 1, InsertPolicy::NORMAL);
        let _ = cache.insert(2, "two", 1, InsertPolicy::NORMAL);

        assert_eq!(cache.get(&1), Some(&"one"));
        assert_eq!(cache.get(&3), None);
        let _ = cache.insert(3, "three", 1, InsertPolicy::NORMAL);

        assert_eq!(cache.peek(&1), Some(&"one"));
        assert_eq!(cache.peek(&2), None);
        assert_eq!(cache.hit_rate(), 0.5);
    }

    #[test]
    fn peek_does_not_track_hits_or_promote_lru_order() {
        let mut cache = cache(2, 100);
        let _ = cache.insert(1, "one", 1, InsertPolicy::NORMAL);
        let _ = cache.insert(2, "two", 1, InsertPolicy::NORMAL);

        assert_eq!(cache.peek(&1), Some(&"one"));
        let _ = cache.insert(3, "three", 1, InsertPolicy::NORMAL);

        assert_eq!(cache.peek(&1), None);
        assert_eq!(cache.peek(&2), Some(&"two"));
        assert_eq!(cache.hit_rate(), 0.0);
    }

    #[test]
    fn insert_evicts_lru_over_capacity_and_returns_removed_entry() {
        let mut cache = cache(2, 100);
        let _ = cache.insert(1, "one", 1, InsertPolicy::NORMAL);
        let _ = cache.insert(2, "two", 1, InsertPolicy::NORMAL);

        let outcome = cache.insert(3, "three", 1, InsertPolicy::NORMAL);

        assert!(outcome.inserted);
        assert_eq!(outcome.evicted.len(), 1);
        assert_eq!(outcome.evicted[0].key, 1);
        assert_eq!(outcome.evicted[0].value, "one");
        assert_eq!(cache.memory_bytes(), 2);
    }

    #[test]
    fn insert_evicts_until_memory_budget_is_satisfied() {
        let mut cache = cache(4, 10);
        let _ = cache.insert(1, "one", 4, InsertPolicy::NORMAL);
        let _ = cache.insert(2, "two", 4, InsertPolicy::NORMAL);

        let outcome = cache.insert(3, "three", 6, InsertPolicy::NORMAL);

        assert_eq!(outcome.evicted.len(), 1);
        assert_eq!(outcome.evicted[0].key, 1);
        assert_eq!(cache.memory_bytes(), 10);
    }

    #[test]
    fn reinserting_existing_key_replaces_cost_without_double_counting() {
        let mut cache = cache(4, 100);
        let _ = cache.insert(1, "one", 4, InsertPolicy::NORMAL);

        let outcome = cache.insert(1, "uno", 7, InsertPolicy::NORMAL);

        assert_eq!(
            outcome
                .replaced
                .expect("old value should be returned")
                .value,
            "one"
        );
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.memory_bytes(), 7);
        assert_eq!(cache.peek(&1), Some(&"uno"));
    }

    #[test]
    fn oversize_reject_leaves_existing_entries_untouched() {
        let mut cache = cache(4, 5);
        let _ = cache.insert(1, "one", 4, InsertPolicy::NORMAL);

        let outcome = cache.insert(2, "two", 6, InsertPolicy::NORMAL);

        assert!(!outcome.inserted);
        assert_eq!(cache.peek(&1), Some(&"one"));
        assert_eq!(cache.peek(&2), None);
        assert_eq!(cache.memory_bytes(), 4);
    }

    #[test]
    fn oversize_admit_keeps_inserted_entry_and_evicts_unprotected_entries() {
        let mut cache = cache(4, 5);
        let _ = cache.insert(1, "one", 4, InsertPolicy::NORMAL);

        let outcome = cache.insert(
            2,
            "two",
            6,
            InsertPolicy {
                oversize: OversizePolicy::Admit,
                eviction: EvictionPolicy::Normal,
            },
        );

        assert!(outcome.inserted);
        assert_eq!(outcome.evicted[0].key, 1);
        assert_eq!(cache.peek(&1), None);
        assert_eq!(cache.peek(&2), Some(&"two"));
        assert_eq!(cache.memory_bytes(), 6);
    }

    #[test]
    fn oversize_admit_without_eviction_keeps_existing_entries() {
        let mut cache = cache(4, 5);
        let _ = cache.insert(1, "one", 4, InsertPolicy::NORMAL);

        let outcome = cache.insert(
            2,
            "two",
            6,
            InsertPolicy {
                oversize: OversizePolicy::Admit,
                eviction: EvictionPolicy::RejectIfEvictionRequired,
            },
        );

        assert!(outcome.inserted);
        assert!(outcome.evicted.is_empty());
        assert_eq!(cache.peek(&1), Some(&"one"));
        assert_eq!(cache.peek(&2), Some(&"two"));
        assert_eq!(cache.memory_bytes(), 10);
    }

    #[test]
    fn protected_keys_are_not_evicted_when_an_unprotected_candidate_exists() {
        let mut cache = cache(2, 100);
        let protected = [1];
        let _ = cache.insert(1, "one", 1, InsertPolicy::NORMAL);
        let _ = cache.insert(2, "two", 1, InsertPolicy::NORMAL);

        let outcome = cache.insert(
            3,
            "three",
            1,
            InsertPolicy {
                oversize: OversizePolicy::Reject,
                eviction: EvictionPolicy::Protect(&protected),
            },
        );

        assert!(outcome.inserted);
        assert_eq!(cache.peek(&1), Some(&"one"));
        assert_eq!(cache.peek(&2), None);
        assert_eq!(cache.peek(&3), Some(&"three"));
    }

    #[test]
    fn protected_insert_rejects_when_only_inserted_entry_could_be_evicted() {
        let mut cache = cache(2, 100);
        let protected = [1, 2];
        let _ = cache.insert(1, "one", 1, InsertPolicy::NORMAL);
        let _ = cache.insert(2, "two", 1, InsertPolicy::NORMAL);

        let outcome = cache.insert(
            3,
            "three",
            1,
            InsertPolicy {
                oversize: OversizePolicy::Reject,
                eviction: EvictionPolicy::Protect(&protected),
            },
        );

        assert!(!outcome.inserted);
        assert_eq!(cache.peek(&1), Some(&"one"));
        assert_eq!(cache.peek(&2), Some(&"two"));
        assert_eq!(cache.peek(&3), None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn reject_if_eviction_required_rejects_capacity_or_memory_eviction() {
        let mut cache = cache(1, 5);
        let _ = cache.insert(1, "one", 3, InsertPolicy::NORMAL);
        let policy = InsertPolicy {
            oversize: OversizePolicy::Reject,
            eviction: EvictionPolicy::RejectIfEvictionRequired,
        };

        assert!(!cache.insert(2, "two", 1, policy).inserted);
        assert!(!cache.insert(1, "uno", 6, policy).inserted);
        assert_eq!(cache.peek(&1), Some(&"one"));
    }

    #[test]
    fn clear_returns_removed_entries_and_resets_memory() {
        let mut cache = cache(2, 100);
        let _ = cache.insert(1, "one", 3, InsertPolicy::NORMAL);
        let _ = cache.insert(2, "two", 4, InsertPolicy::NORMAL);

        let removed = cache.clear();

        assert_eq!(removed.len(), 2);
        assert!(cache.is_empty());
        assert_eq!(cache.memory_bytes(), 0);
    }
}
