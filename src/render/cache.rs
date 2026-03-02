use std::num::NonZeroUsize;

use lru::LruCache;

use crate::backend::RgbaFrame;

const DEFAULT_MEMORY_BUDGET_BYTES: usize = 512 * 1024 * 1024;
const DEFAULT_MAX_ENTRIES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderedPageKey {
    pub doc_id: u64,
    pub page: usize,
    pub scale_milli: u32,
}

impl RenderedPageKey {
    pub fn new(doc_id: u64, page: usize, scale: f32) -> Self {
        let scale_milli = (scale.max(0.0) * 1000.0).round() as u32;
        Self {
            doc_id,
            page,
            scale_milli,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheCounters {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

#[derive(Debug, Clone)]
pub struct RenderedPageCache {
    max_entries: usize,
    memory_budget_bytes: usize,
    memory_bytes: usize,
    entries: LruCache<RenderedPageKey, RgbaFrame>,
    counters: CacheCounters,
}

impl Default for RenderedPageCache {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ENTRIES, DEFAULT_MEMORY_BUDGET_BYTES)
    }
}

impl RenderedPageCache {
    pub fn new(max_entries: usize, memory_budget_bytes: usize) -> Self {
        let max_entries = max_entries.max(1);
        Self {
            max_entries,
            memory_budget_bytes: memory_budget_bytes.max(1),
            memory_bytes: 0,
            entries: LruCache::new(
                NonZeroUsize::new(max_entries).expect("max entries is non-zero"),
            ),
            counters: CacheCounters::default(),
        }
    }

    pub fn get(&mut self, key: &RenderedPageKey) -> Option<&RgbaFrame> {
        if self.entries.peek(key).is_some() {
            self.counters.hits += 1;
            return self.entries.get(key);
        }

        self.counters.misses += 1;
        None
    }

    pub fn get_cloned(&mut self, key: &RenderedPageKey) -> Option<RgbaFrame> {
        self.get(key).cloned()
    }

    pub fn insert(
        &mut self,
        key: RenderedPageKey,
        frame: RgbaFrame,
        allow_single_oversize: bool,
    ) -> bool {
        let frame_bytes = frame.byte_len();
        if frame_bytes > self.memory_budget_bytes {
            if !allow_single_oversize {
                return false;
            }
            self.clear();
            self.memory_bytes = frame_bytes;
            self.entries.put(key, frame);
            return true;
        }

        // Keep the single-entry oversize recovery path stable:
        // reject unrelated non-oversize inserts while a lone oversize
        // frame is intentionally resident.
        if !allow_single_oversize
            && self.memory_bytes > self.memory_budget_bytes
            && self.entries.len() == 1
            && self.entries.peek(&key).is_none()
            && self
                .entries
                .peek_lru()
                .is_some_and(|(_cached_key, cached)| cached.byte_len() > self.memory_budget_bytes)
        {
            return false;
        }

        if let Some(prev) = self.entries.pop(&key) {
            self.memory_bytes = self.memory_bytes.saturating_sub(prev.byte_len());
        }

        let implicit_evicted_bytes =
            if self.entries.len() >= self.max_entries && self.entries.peek(&key).is_none() {
                self.entries
                    .peek_lru()
                    .map(|(_key, frame)| frame.byte_len())
            } else {
                None
            };

        self.memory_bytes += frame_bytes;
        self.entries.put(key, frame);
        if let Some(evicted_bytes) = implicit_evicted_bytes {
            self.memory_bytes = self.memory_bytes.saturating_sub(evicted_bytes);
            self.counters.evictions += 1;
        }
        self.evict_while_needed();
        true
    }

    pub fn remove_doc(&mut self, doc_id: u64) {
        let doomed: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(key, _)| (key.doc_id == doc_id).then_some(*key))
            .collect();

        for key in doomed {
            self.remove(&key);
        }
    }

    pub fn remove(&mut self, key: &RenderedPageKey) {
        if let Some(frame) = self.entries.pop(key) {
            self.memory_bytes = self.memory_bytes.saturating_sub(frame.byte_len());
            self.counters.evictions += 1;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.memory_bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    pub fn memory_budget_bytes(&self) -> usize {
        self.memory_budget_bytes
    }

    pub fn contains(&self, key: &RenderedPageKey) -> bool {
        self.entries.peek(key).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    pub fn counters(&self) -> CacheCounters {
        self.counters
    }

    pub fn hit_rate(&self) -> f64 {
        let lookups = self.counters.hits + self.counters.misses;
        if lookups == 0 {
            return 0.0;
        }
        self.counters.hits as f64 / lookups as f64
    }

    fn evict_while_needed(&mut self) {
        while self.entries.len() > self.max_entries || self.memory_bytes > self.memory_budget_bytes
        {
            if self.entries.len() == 1 {
                break;
            }
            let Some((_key, frame)) = self.entries.pop_lru() else {
                break;
            };
            self.memory_bytes = self.memory_bytes.saturating_sub(frame.byte_len());
            self.counters.evictions += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderedPageCache, RenderedPageKey};
    use crate::backend::RgbaFrame;

    fn frame(width: u32, height: u32) -> RgbaFrame {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        pixels.resize(width as usize * height as usize * 4, 0xff);
        RgbaFrame {
            width,
            height,
            pixels: pixels.into(),
        }
    }

    #[test]
    fn cache_tracks_hit_rate() {
        let mut cache = RenderedPageCache::new(4, 1024 * 1024);
        let key = RenderedPageKey::new(10, 1, 1.0);
        let _ = cache.insert(key, frame(10, 10), false);

        assert!(cache.get(&key).is_some());
        assert!(cache.get(&RenderedPageKey::new(10, 2, 1.0)).is_none());

        let counters = cache.counters();
        assert_eq!(counters.hits, 1);
        assert_eq!(counters.misses, 1);
        assert_eq!(cache.hit_rate(), 0.5);
    }

    #[test]
    fn cache_evicts_when_over_budget() {
        let mut cache = RenderedPageCache::new(2, 10_000);
        let _ = cache.insert(RenderedPageKey::new(1, 1, 1.0), frame(40, 40), false);
        let _ = cache.insert(RenderedPageKey::new(1, 2, 1.0), frame(40, 40), false);

        assert!(cache.len() < 2);
        assert!(cache.memory_bytes() <= 10_000);
    }

    #[test]
    fn cache_reinsert_updates_memory_without_double_counting() {
        let mut cache = RenderedPageCache::new(4, 1024 * 1024);
        let key = RenderedPageKey::new(1, 0, 1.0);
        let _ = cache.insert(key, frame(8, 8), false);
        let first_bytes = cache.memory_bytes();
        let _ = cache.insert(key, frame(10, 10), false);

        assert_eq!(cache.len(), 1);
        assert!(cache.memory_bytes() > first_bytes);
        assert_eq!(cache.memory_bytes(), frame(10, 10).byte_len());
    }

    #[test]
    fn remove_doc_reduces_memory_and_counts_evictions() {
        let mut cache = RenderedPageCache::new(8, 1024 * 1024);
        let a = RenderedPageKey::new(10, 0, 1.0);
        let b = RenderedPageKey::new(10, 1, 1.0);
        let c = RenderedPageKey::new(11, 0, 1.0);
        let _ = cache.insert(a, frame(6, 6), false);
        let _ = cache.insert(b, frame(6, 6), false);
        let _ = cache.insert(c, frame(6, 6), false);
        let before = cache.memory_bytes();

        cache.remove_doc(10);

        assert!(!cache.contains(&a));
        assert!(!cache.contains(&b));
        assert!(cache.contains(&c));
        assert!(cache.memory_bytes() < before);
        assert_eq!(cache.counters().evictions, 2);
    }

    #[test]
    fn insert_at_capacity_keeps_memory_accounting_consistent() {
        let mut cache = RenderedPageCache::new(2, 1024 * 1024);
        let _ = cache.insert(RenderedPageKey::new(1, 0, 1.0), frame(4, 4), false);
        let _ = cache.insert(RenderedPageKey::new(1, 1, 1.0), frame(5, 5), false);
        let _ = cache.insert(RenderedPageKey::new(1, 2, 1.0), frame(6, 6), false);

        let expected = frame(5, 5).byte_len() + frame(6, 6).byte_len();
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.memory_bytes(), expected);
    }

    #[test]
    fn get_cloned_shares_pixel_buffer() {
        let mut cache = RenderedPageCache::new(2, 1024 * 1024);
        let key = RenderedPageKey::new(1, 0, 1.0);
        let stored = frame(4, 4);
        let _ = cache.insert(key, stored.clone(), false);

        let cloned = cache
            .get_cloned(&key)
            .expect("cached frame should be available");

        assert!(std::sync::Arc::ptr_eq(&stored.pixels, &cloned.pixels));
    }

    #[test]
    fn oversize_insert_without_override_does_not_clear_existing_entries() {
        let mut cache = RenderedPageCache::new(4, 100);
        let kept = RenderedPageKey::new(1, 0, 1.0);
        let oversize = RenderedPageKey::new(1, 1, 1.0);
        let _ = cache.insert(kept, frame(4, 4), false);

        let inserted = cache.insert(oversize, frame(8, 8), false);
        assert!(!inserted);
        assert!(cache.contains(&kept));
        assert!(!cache.contains(&oversize));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn oversize_insert_with_override_keeps_single_entry() {
        let mut cache = RenderedPageCache::new(4, 100);
        let kept = RenderedPageKey::new(1, 0, 1.0);
        let oversize = RenderedPageKey::new(1, 1, 1.0);
        let _ = cache.insert(kept, frame(4, 4), false);

        let inserted = cache.insert(oversize, frame(8, 8), true);
        assert!(inserted);
        assert!(!cache.contains(&kept));
        assert!(cache.contains(&oversize));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn non_oversize_insert_does_not_evict_single_oversize_entry() {
        let mut cache = RenderedPageCache::new(4, 100);
        let oversize = RenderedPageKey::new(1, 1, 1.0);
        let prefetch = RenderedPageKey::new(1, 2, 1.0);

        assert!(cache.insert(oversize, frame(8, 8), true));
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(&oversize));

        let inserted_prefetch = cache.insert(prefetch, frame(4, 4), false);
        assert!(!inserted_prefetch);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(&oversize));
        assert!(!cache.contains(&prefetch));
    }
}
