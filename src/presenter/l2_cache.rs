use ratatui_image::protocol::StatefulProtocol;

use crate::backend::RgbaFrame;
use crate::cache::{
    BudgetedLruCache, CacheLimits, EvictionPolicy, InsertPolicy, OversizePolicy, RemovedEntry,
};
use crate::render::cache::RenderedPageKey;

use super::traits::{PanOffset, Viewport};

pub(crate) const L2_MAX_ENTRIES: usize = 96;
pub(crate) const L2_MEMORY_BUDGET_BYTES: usize = 64 * 1024 * 1024;

pub(crate) enum TerminalFrameState {
    PendingFrame(RgbaFrame),
    Encoding,
    Ready(Box<StatefulProtocol>),
    Failed,
}

pub(crate) struct TerminalFrameEntry {
    state: TerminalFrameState,
}

impl TerminalFrameEntry {
    #[cfg(test)]
    pub(crate) fn state(&self) -> &TerminalFrameState {
        &self.state
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TerminalFrameKey {
    pub(crate) rendered_page: RenderedPageKey,
    pub(crate) viewport: Viewport,
    pub(crate) pan: PanOffset,
    pub(crate) overlay_stamp: u64,
}

pub(crate) struct TerminalFrameCache {
    entries: BudgetedLruCache<TerminalFrameKey, TerminalFrameEntry>,
    pending_work_count: usize,
}

impl Default for TerminalFrameCache {
    fn default() -> Self {
        Self::new(L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES)
    }
}

impl TerminalFrameCache {
    pub(crate) fn new(max_entries: usize, memory_budget_bytes: usize) -> Self {
        Self {
            entries: BudgetedLruCache::new(CacheLimits::new(max_entries, memory_budget_bytes)),
            pending_work_count: 0,
        }
    }

    pub(crate) fn lookup_mut(&mut self, key: &TerminalFrameKey) -> Option<&mut TerminalFrameEntry> {
        self.entries.get_mut(key)
    }

    pub(crate) fn cached_mut(&mut self, key: &TerminalFrameKey) -> Option<&mut TerminalFrameEntry> {
        self.entries.peek_mut(key)
    }

    #[cfg(test)]
    pub(crate) fn insert(
        &mut self,
        key: TerminalFrameKey,
        frame: RgbaFrame,
        approx_bytes: usize,
        allow_single_oversize: bool,
        protected_key: Option<TerminalFrameKey>,
    ) -> bool {
        let protected_keys = protected_key.into_iter().collect::<Vec<_>>();
        self.insert_protected(
            key,
            frame,
            approx_bytes,
            allow_single_oversize,
            &protected_keys,
        )
    }

    pub(crate) fn insert_protected(
        &mut self,
        key: TerminalFrameKey,
        frame: RgbaFrame,
        approx_bytes: usize,
        allow_single_oversize: bool,
        protected_keys: &[TerminalFrameKey],
    ) -> bool {
        let protected_keys = protected_keys
            .iter()
            .copied()
            .filter(|protected| *protected != key)
            .collect::<Vec<_>>();
        if !allow_single_oversize
            && self.entries.memory_bytes() > self.entries.memory_budget_bytes()
            && self.entries.peek(&key).is_none()
        {
            return false;
        }

        // Keep visible ready frames resident while swapping in an oversize current
        // entry. If the cache is configured for a single entry, honor that cap.
        let protected_keys = if self.entries.max_entries() > 1 {
            protected_keys.as_slice()
        } else {
            &[]
        };
        let outcome = self.entries.insert(
            key,
            TerminalFrameEntry {
                state: TerminalFrameState::PendingFrame(frame),
            },
            approx_bytes,
            InsertPolicy {
                oversize: if allow_single_oversize {
                    OversizePolicy::Admit
                } else {
                    OversizePolicy::Reject
                },
                eviction: if protected_keys.is_empty() {
                    EvictionPolicy::Normal
                } else {
                    EvictionPolicy::Protect(protected_keys)
                },
            },
        );
        if !outcome.inserted {
            return false;
        }
        self.note_removed_entries(outcome.replaced.into_iter().chain(outcome.evicted));
        self.pending_work_count += 1;
        true
    }

    pub(crate) fn hit_rate(&self) -> f64 {
        self.entries.hit_rate()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn max_entries(&self) -> usize {
        self.entries.max_entries()
    }

    #[cfg(test)]
    pub(crate) fn memory_budget_bytes(&self) -> usize {
        self.entries.memory_budget_bytes()
    }

    #[cfg(test)]
    pub(crate) fn memory_bytes(&self) -> usize {
        self.entries.memory_bytes()
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        self.pending_work_count > 0
    }

    pub(crate) fn set_state(&mut self, key: &TerminalFrameKey, state: TerminalFrameState) -> bool {
        self.replace_state(key, state).is_some()
    }

    pub(crate) fn replace_state(
        &mut self,
        key: &TerminalFrameKey,
        state: TerminalFrameState,
    ) -> Option<TerminalFrameState> {
        let entry = self.entries.peek_mut(key)?;
        let old_state = std::mem::replace(&mut entry.state, state);
        let old_pending = state_has_pending_work(&old_state);
        let new_pending = state_has_pending_work(&entry.state);
        match (old_pending, new_pending) {
            (true, false) => self.pending_work_count = self.pending_work_count.saturating_sub(1),
            (false, true) => self.pending_work_count += 1,
            _ => {}
        }
        Some(old_state)
    }

    pub(crate) fn clear(&mut self) {
        let _ = self.entries.clear();
        self.pending_work_count = 0;
    }

    pub(crate) fn remove(&mut self, key: &TerminalFrameKey) -> bool {
        let Some(removed) = self.entries.remove(key) else {
            return false;
        };
        self.note_removed_state(&removed.value.state);
        true
    }

    fn note_removed_state(&mut self, state: &TerminalFrameState) {
        if state_has_pending_work(state) {
            self.pending_work_count = self.pending_work_count.saturating_sub(1);
        }
    }

    fn note_removed_entries(
        &mut self,
        entries: impl IntoIterator<Item = RemovedEntry<TerminalFrameKey, TerminalFrameEntry>>,
    ) {
        for entry in entries {
            self.note_removed_state(&entry.value.state);
        }
    }
}

fn state_has_pending_work(state: &TerminalFrameState) -> bool {
    matches!(
        state,
        TerminalFrameState::PendingFrame(_) | TerminalFrameState::Encoding
    )
}

#[cfg(test)]
mod tests {
    use crate::backend::RgbaFrame;

    use super::*;

    fn frame() -> RgbaFrame {
        RgbaFrame {
            width: 4,
            height: 4,
            pixels: vec![200; 4 * 4 * 4].into(),
        }
    }

    fn key(page: usize) -> TerminalFrameKey {
        TerminalFrameKey {
            rendered_page: RenderedPageKey::new(1, page, 1.0),
            viewport: Viewport {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
            pan: PanOffset::default(),
            overlay_stamp: 0,
        }
    }

    #[test]
    fn cached_mut_does_not_touch_lru_order() {
        let mut cache = TerminalFrameCache::default();
        for page in 0..L2_MAX_ENTRIES {
            let _ = cache.insert(key(page), frame(), 1, false, None);
        }

        let oldest = key(0);
        assert!(cache.cached_mut(&oldest).is_some());
        let _ = cache.insert(key(L2_MAX_ENTRIES), frame(), 1, false, None);

        assert!(cache.cached_mut(&oldest).is_none());
        assert!(cache.cached_mut(&key(1)).is_some());
    }

    #[test]
    fn insert_at_capacity_keeps_memory_accounting_consistent() {
        let mut cache = TerminalFrameCache::default();
        for page in 0..L2_MAX_ENTRIES {
            let _ = cache.insert(key(page), frame(), 16, false, None);
        }
        let _ = cache.insert(key(L2_MAX_ENTRIES), frame(), 20, false, None);

        let expected = (L2_MAX_ENTRIES - 1) * 16 + 20;
        assert_eq!(cache.len(), L2_MAX_ENTRIES);
        assert_eq!(cache.memory_bytes(), expected);
    }

    #[test]
    fn insert_keeps_pending_frame_buffer_shared() {
        let mut cache = TerminalFrameCache::default();
        let key = key(0);
        let source = frame();
        let _ = cache.insert(key, source.clone(), source.byte_len(), false, None);

        let stored_pixels = match cache.cached_mut(&key).map(|entry| entry.state()) {
            Some(TerminalFrameState::PendingFrame(frame)) => &frame.pixels,
            _ => panic!("expected pending frame"),
        };
        assert!(source.pixels.ptr_eq(stored_pixels));
    }

    #[test]
    fn pending_work_tracks_state_transitions() {
        let mut cache = TerminalFrameCache::default();
        let key = key(0);
        let _ = cache.insert(key, frame(), 16, false, None);
        assert!(cache.has_pending_work());

        assert!(cache.set_state(&key, TerminalFrameState::Failed));
        assert!(!cache.has_pending_work());

        assert!(cache.set_state(&key, TerminalFrameState::Encoding));
        assert!(cache.has_pending_work());

        assert!(cache.remove(&key));
        assert!(!cache.has_pending_work());
    }

    #[test]
    fn pending_work_tracks_eviction_and_clear() {
        let mut cache = TerminalFrameCache::new(1, 64);
        let first = key(0);
        let second = key(1);
        assert!(cache.insert(first, frame(), 16, false, None));
        assert!(cache.insert(second, frame(), 16, false, None));
        assert!(cache.cached_mut(&first).is_none());
        assert!(cache.has_pending_work());

        cache.clear();
        assert!(!cache.has_pending_work());
    }

    #[test]
    fn oversize_insert_without_override_preserves_existing_entries() {
        let mut cache = TerminalFrameCache::new(8, 32);
        let kept = key(0);
        let oversize = key(1);
        let _ = cache.insert(kept, frame(), 16, false, None);

        let inserted = cache.insert(oversize, frame(), 64, false, None);
        assert!(!inserted);
        assert!(cache.cached_mut(&kept).is_some());
        assert!(cache.cached_mut(&oversize).is_none());
    }

    #[test]
    fn oversize_insert_with_override_keeps_single_entry() {
        let mut cache = TerminalFrameCache::new(8, 32);
        let kept = key(0);
        let oversize = key(1);
        let _ = cache.insert(kept, frame(), 16, false, None);

        let inserted = cache.insert(oversize, frame(), 64, true, None);
        assert!(inserted);
        assert!(cache.cached_mut(&kept).is_none());
        assert!(cache.cached_mut(&oversize).is_some());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn oversize_insert_with_protected_key_keeps_visible_entry() {
        let mut cache = TerminalFrameCache::new(8, 32);
        let visible = key(0);
        let oversize = key(1);
        let _ = cache.insert(visible, frame(), 16, false, None);

        let inserted = cache.insert(oversize, frame(), 64, true, Some(visible));
        assert!(inserted);
        assert!(cache.cached_mut(&visible).is_some());
        assert!(cache.cached_mut(&oversize).is_some());
        assert_eq!(cache.len(), 2);
        assert!(cache.memory_bytes() > cache.memory_budget_bytes());
    }

    #[test]
    fn oversize_insert_with_protected_keys_keeps_visible_entries() {
        let mut cache = TerminalFrameCache::new(8, 32);
        let left_visible = key(0);
        let right_visible = key(1);
        let oversize = key(2);
        let _ = cache.insert(left_visible, frame(), 16, false, None);
        let _ = cache.insert(right_visible, frame(), 16, false, None);

        let inserted =
            cache.insert_protected(oversize, frame(), 64, true, &[left_visible, right_visible]);
        assert!(inserted);
        assert!(cache.cached_mut(&left_visible).is_some());
        assert!(cache.cached_mut(&right_visible).is_some());
        assert!(cache.cached_mut(&oversize).is_some());
        assert_eq!(cache.len(), 3);
        assert!(cache.memory_bytes() > cache.memory_budget_bytes());
    }

    #[test]
    fn oversize_insert_with_protected_key_respects_single_entry_limit() {
        let mut cache = TerminalFrameCache::new(1, 32);
        let visible = key(0);
        let oversize = key(1);
        let _ = cache.insert(visible, frame(), 16, false, None);

        let inserted = cache.insert(oversize, frame(), 64, true, Some(visible));
        assert!(inserted);
        assert!(cache.cached_mut(&visible).is_none());
        assert!(cache.cached_mut(&oversize).is_some());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn non_oversize_insert_does_not_evict_single_oversize_entry() {
        let mut cache = TerminalFrameCache::new(8, 32);
        let oversize = key(1);
        let prefetch = key(2);

        assert!(cache.insert(oversize, frame(), 64, true, None));
        assert_eq!(cache.len(), 1);
        assert!(cache.cached_mut(&oversize).is_some());

        let inserted_prefetch = cache.insert(prefetch, frame(), 16, false, None);
        assert!(!inserted_prefetch);
        assert_eq!(cache.len(), 1);
        assert!(cache.cached_mut(&oversize).is_some());
        assert!(cache.cached_mut(&prefetch).is_none());
    }
}
