use std::num::NonZeroUsize;

use lru::LruCache;
use ratatui_image::protocol::StatefulProtocol;

use crate::backend::RgbaFrame;
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
    pub(crate) state: TerminalFrameState,
    pub(crate) approx_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TerminalFrameKey {
    pub(crate) rendered_page: RenderedPageKey,
    pub(crate) viewport: Viewport,
    pub(crate) pan: PanOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CacheCounters {
    hits: u64,
    misses: u64,
}

pub(crate) struct TerminalFrameCache {
    max_entries: usize,
    memory_budget_bytes: usize,
    pub(crate) entries: LruCache<TerminalFrameKey, TerminalFrameEntry>,
    pub(crate) memory_bytes: usize,
    counters: CacheCounters,
}

impl Default for TerminalFrameCache {
    fn default() -> Self {
        Self::new(L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES)
    }
}

impl TerminalFrameCache {
    pub(crate) fn new(max_entries: usize, memory_budget_bytes: usize) -> Self {
        let max_entries = max_entries.max(1);
        Self {
            max_entries,
            memory_budget_bytes: memory_budget_bytes.max(1),
            entries: LruCache::new(
                NonZeroUsize::new(max_entries).expect("l2 cache entries is non-zero"),
            ),
            memory_bytes: 0,
            counters: CacheCounters::default(),
        }
    }

    pub(crate) fn lookup_mut(&mut self, key: &TerminalFrameKey) -> Option<&mut TerminalFrameEntry> {
        if self.entries.peek(key).is_some() {
            self.counters.hits += 1;
            return self.entries.get_mut(key);
        }

        self.counters.misses += 1;
        None
    }

    pub(crate) fn cached_mut(&mut self, key: &TerminalFrameKey) -> Option<&mut TerminalFrameEntry> {
        self.entries.peek_mut(key)
    }

    pub(crate) fn insert(&mut self, key: TerminalFrameKey, frame: RgbaFrame, approx_bytes: usize) {
        if approx_bytes > self.memory_budget_bytes {
            self.clear();
            return;
        }

        if let Some(prev) = self.entries.pop(&key) {
            self.memory_bytes = self.memory_bytes.saturating_sub(prev.approx_bytes);
        }

        let implicit_evicted_bytes =
            if self.entries.len() >= self.max_entries && self.entries.peek(&key).is_none() {
                self.entries
                    .peek_lru()
                    .map(|(_key, entry)| entry.approx_bytes)
            } else {
                None
            };

        self.memory_bytes += approx_bytes;
        self.entries.put(
            key,
            TerminalFrameEntry {
                state: TerminalFrameState::PendingFrame(frame),
                approx_bytes,
            },
        );
        if let Some(evicted_bytes) = implicit_evicted_bytes {
            self.memory_bytes = self.memory_bytes.saturating_sub(evicted_bytes);
        }
        self.evict_while_needed();
    }

    pub(crate) fn hit_rate(&self) -> f64 {
        let lookups = self.counters.hits + self.counters.misses;
        if lookups == 0 {
            return 0.0;
        }
        self.counters.hits as f64 / lookups as f64
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn max_entries(&self) -> usize {
        self.max_entries
    }

    #[cfg(test)]
    pub(crate) fn memory_budget_bytes(&self) -> usize {
        self.memory_budget_bytes
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        self.entries.iter().any(|(_key, entry)| {
            matches!(
                &entry.state,
                TerminalFrameState::PendingFrame(_) | TerminalFrameState::Encoding
            )
        })
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.memory_bytes = 0;
    }

    fn evict_while_needed(&mut self) {
        while self.entries.len() > self.max_entries || self.memory_bytes > self.memory_budget_bytes
        {
            let Some((_key, entry)) = self.entries.pop_lru() else {
                break;
            };
            self.memory_bytes = self.memory_bytes.saturating_sub(entry.approx_bytes);
        }
    }
}
