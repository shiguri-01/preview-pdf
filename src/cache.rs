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
    hits: u64,
    misses: u64,
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
            hits: 0,
            misses: 0,
        }
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        if self.entries.peek(key).is_some() {
            self.hits += 1;
            return self.entries.get(key).map(|entry| &entry.value);
        }
        self.misses += 1;
        None
    }

    pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if self.entries.peek(key).is_some() {
            self.hits += 1;
            return self.entries.get_mut(key).map(|entry| &mut entry.value);
        }
        self.misses += 1;
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
        allow_single_oversize: bool,
    ) -> bool {
        self.insert_protected(key, value, cost_bytes, allow_single_oversize, &[])
    }

    pub(crate) fn insert_protected(
        &mut self,
        key: K,
        value: V,
        cost_bytes: usize,
        allow_single_oversize: bool,
        protected: &[K],
    ) -> bool {
        let cost_bytes = cost_bytes.max(1);
        if cost_bytes > self.limits.memory_budget_bytes && !allow_single_oversize {
            return false;
        }
        if !protected.is_empty()
            && !self.can_admit_with_protected(&key, cost_bytes, allow_single_oversize, protected)
        {
            return false;
        }
        self.remove(&key);
        self.memory_bytes = self.memory_bytes.saturating_add(cost_bytes);
        let _ = self
            .entries
            .push(key.clone(), CacheEntry { value, cost_bytes });

        let oversize_admitted =
            cost_bytes > self.limits.memory_budget_bytes && allow_single_oversize;

        if oversize_admitted {
            self.evict_unprotected_except(&key, protected);
        } else {
            self.evict_while_needed(protected);
        }
        true
    }

    pub(crate) fn try_insert_without_eviction(
        &mut self,
        key: K,
        value: V,
        cost_bytes: usize,
    ) -> bool {
        let cost_bytes = cost_bytes.max(1);
        if cost_bytes > self.limits.memory_budget_bytes
            || self.eviction_required_for_insert(&key, cost_bytes)
        {
            return false;
        }
        self.insert(key, value, cost_bytes, false)
    }

    pub(crate) fn remove(&mut self, key: &K) -> bool {
        self.pop_entry(key)
    }

    pub(crate) fn remove_where(&mut self, mut should_remove: impl FnMut(&K, &V) -> bool) {
        let keys = self
            .entries
            .iter()
            .filter_map(|(key, entry)| should_remove(key, &entry.value).then_some(key.clone()))
            .collect::<Vec<_>>();
        for key in keys {
            self.remove(&key);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.memory_bytes = 0;
    }

    pub(crate) fn any(&self, predicate: impl FnMut(&V) -> bool) -> bool {
        self.entries
            .iter()
            .map(|(_, entry)| &entry.value)
            .any(predicate)
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
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

    pub(crate) fn hit_rate(&self) -> f64 {
        let lookups = self.hits + self.misses;
        if lookups == 0 {
            return 0.0;
        }
        self.hits as f64 / lookups as f64
    }

    fn eviction_required_for_insert(&self, key: &K, cost_bytes: usize) -> bool {
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
            || projected_memory > self.limits.memory_budget_bytes
    }

    fn can_admit_with_protected(
        &self,
        key: &K,
        cost_bytes: usize,
        allow_single_oversize: bool,
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
            if self.fits_limits(
                projected_len,
                projected_memory,
                cost_bytes,
                allow_single_oversize,
            ) {
                return true;
            }
            if entry_key == key || protected.contains(entry_key) {
                continue;
            }
            projected_len = projected_len.saturating_sub(1);
            projected_memory = projected_memory.saturating_sub(entry.cost_bytes);
        }
        self.fits_limits(
            projected_len,
            projected_memory,
            cost_bytes,
            allow_single_oversize,
        )
    }

    fn fits_limits(
        &self,
        projected_len: usize,
        projected_memory: usize,
        cost_bytes: usize,
        allow_single_oversize: bool,
    ) -> bool {
        projected_len <= self.limits.max_entries
            && (projected_memory <= self.limits.memory_budget_bytes
                || (cost_bytes > self.limits.memory_budget_bytes && allow_single_oversize))
    }

    fn evict_while_needed(&mut self, protected: &[K]) {
        while self.entries.len() > self.limits.max_entries
            || self.memory_bytes > self.limits.memory_budget_bytes
        {
            if !self.pop_lru_unprotected(protected) {
                break;
            }
        }
    }

    fn evict_unprotected_except(&mut self, inserted_key: &K, protected: &[K]) {
        let keys = self
            .entries
            .iter()
            .filter_map(|(key, _)| {
                (key != inserted_key && !protected.contains(key)).then_some(key.clone())
            })
            .collect::<Vec<_>>();
        for key in keys {
            self.pop_entry(&key);
        }
    }

    fn pop_lru_unprotected(&mut self, protected: &[K]) -> bool {
        let mut protected_entries = Vec::new();
        loop {
            match self.entries.pop_lru() {
                Some((key, entry)) if protected.contains(&key) => {
                    protected_entries.push((key, entry));
                }
                Some((_key, entry)) => {
                    for (protected_key, protected_entry) in protected_entries.into_iter().rev() {
                        let _ = self.entries.push(protected_key.clone(), protected_entry);
                        let _ = self.entries.demote(&protected_key);
                    }
                    self.memory_bytes = self.memory_bytes.saturating_sub(entry.cost_bytes);
                    return true;
                }
                None => {
                    for (protected_key, protected_entry) in protected_entries.into_iter().rev() {
                        let _ = self.entries.push(protected_key.clone(), protected_entry);
                        let _ = self.entries.demote(&protected_key);
                    }
                    return false;
                }
            }
        }
    }

    fn pop_entry(&mut self, key: &K) -> bool {
        let Some(entry) = self.entries.pop(key) else {
            return false;
        };
        self.memory_bytes = self.memory_bytes.saturating_sub(entry.cost_bytes);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{BudgetedLruCache, CacheLimits};

    fn cache(max_entries: usize, memory_budget_bytes: usize) -> BudgetedLruCache<u8, &'static str> {
        BudgetedLruCache::new(CacheLimits::new(max_entries, memory_budget_bytes))
    }

    #[test]
    fn get_tracks_hit_rate_and_promotes_lru_order() {
        let mut cache = cache(2, 100);
        let _ = cache.insert(1, "one", 1, false);
        let _ = cache.insert(2, "two", 1, false);

        assert_eq!(cache.get(&1), Some(&"one"));
        assert_eq!(cache.get(&3), None);
        let _ = cache.insert(3, "three", 1, false);

        assert_eq!(cache.peek(&1), Some(&"one"));
        assert_eq!(cache.peek(&2), None);
        assert_eq!(cache.hit_rate(), 0.5);
    }

    #[test]
    fn peek_does_not_track_hits_or_promote_lru_order() {
        let mut cache = cache(2, 100);
        let _ = cache.insert(1, "one", 1, false);
        let _ = cache.insert(2, "two", 1, false);

        assert_eq!(cache.peek(&1), Some(&"one"));
        let _ = cache.insert(3, "three", 1, false);

        assert_eq!(cache.peek(&1), None);
        assert_eq!(cache.peek(&2), Some(&"two"));
        assert_eq!(cache.hit_rate(), 0.0);
    }

    #[test]
    fn insert_evicts_lru_over_capacity() {
        let mut cache = cache(2, 100);
        let _ = cache.insert(1, "one", 1, false);
        let _ = cache.insert(2, "two", 1, false);

        assert!(cache.insert(3, "three", 1, false));

        assert_eq!(cache.peek(&1), None);
        assert_eq!(cache.memory_bytes(), 2);
    }

    #[test]
    fn insert_evicts_until_memory_budget_is_satisfied() {
        let mut cache = cache(4, 10);
        let _ = cache.insert(1, "one", 4, false);
        let _ = cache.insert(2, "two", 4, false);

        assert!(cache.insert(3, "three", 6, false));

        assert_eq!(cache.peek(&1), None);
        assert_eq!(cache.memory_bytes(), 10);
    }

    #[test]
    fn reinserting_existing_key_replaces_cost_without_double_counting() {
        let mut cache = cache(4, 100);
        let _ = cache.insert(1, "one", 4, false);

        assert!(cache.insert(1, "uno", 7, false));

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.memory_bytes(), 7);
        assert_eq!(cache.peek(&1), Some(&"uno"));
    }

    #[test]
    fn oversize_reject_leaves_existing_entries_untouched() {
        let mut cache = cache(4, 5);
        let _ = cache.insert(1, "one", 4, false);

        let inserted = cache.insert(2, "two", 6, false);

        assert!(!inserted);
        assert_eq!(cache.peek(&1), Some(&"one"));
        assert_eq!(cache.peek(&2), None);
        assert_eq!(cache.memory_bytes(), 4);
    }

    #[test]
    fn oversize_admit_keeps_inserted_entry_and_evicts_unprotected_entries() {
        let mut cache = cache(4, 5);
        let _ = cache.insert(1, "one", 4, false);

        let inserted = cache.insert(2, "two", 6, true);

        assert!(inserted);
        assert_eq!(cache.peek(&1), None);
        assert_eq!(cache.peek(&2), Some(&"two"));
        assert_eq!(cache.memory_bytes(), 6);
    }

    #[test]
    fn protected_keys_are_not_evicted_when_an_unprotected_candidate_exists() {
        let mut cache = cache(2, 100);
        let protected = [1];
        let _ = cache.insert(1, "one", 1, false);
        let _ = cache.insert(2, "two", 1, false);

        let inserted = cache.insert_protected(3, "three", 1, false, &protected);

        assert!(inserted);
        assert_eq!(cache.peek(&1), Some(&"one"));
        assert_eq!(cache.peek(&2), None);
        assert_eq!(cache.peek(&3), Some(&"three"));
    }

    #[test]
    fn protected_insert_rejects_when_only_inserted_entry_could_be_evicted() {
        let mut cache = cache(2, 100);
        let protected = [1, 2];
        let _ = cache.insert(1, "one", 1, false);
        let _ = cache.insert(2, "two", 1, false);

        let inserted = cache.insert_protected(3, "three", 1, false, &protected);

        assert!(!inserted);
        assert_eq!(cache.peek(&1), Some(&"one"));
        assert_eq!(cache.peek(&2), Some(&"two"));
        assert_eq!(cache.peek(&3), None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn reject_if_eviction_required_rejects_capacity_or_memory_eviction() {
        let mut cache = cache(1, 5);
        let _ = cache.insert(1, "one", 3, false);

        assert!(!cache.try_insert_without_eviction(2, "two", 1));
        assert!(!cache.try_insert_without_eviction(1, "uno", 6));
        assert_eq!(cache.peek(&1), Some(&"one"));
    }

    #[test]
    fn clear_resets_entries_and_memory() {
        let mut cache = cache(2, 100);
        let _ = cache.insert(1, "one", 3, false);
        let _ = cache.insert(2, "two", 4, false);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert_eq!(cache.memory_bytes(), 0);
    }
}
