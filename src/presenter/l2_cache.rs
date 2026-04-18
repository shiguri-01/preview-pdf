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
    pub(crate) overlay_stamp: u64,
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

    pub(crate) fn insert(
        &mut self,
        key: TerminalFrameKey,
        frame: RgbaFrame,
        approx_bytes: usize,
        allow_single_oversize: bool,
        protected_key: Option<TerminalFrameKey>,
    ) -> bool {
        if approx_bytes > self.memory_budget_bytes {
            if !allow_single_oversize {
                return false;
            }

            if let Some(prev) = self.entries.pop(&key) {
                self.memory_bytes = self.memory_bytes.saturating_sub(prev.approx_bytes);
            }

            // Keep the visible ready frame resident while swapping in an oversize current
            // entry. We allow this narrow over-budget state so the viewer never regresses
            // from "image visible" back to blank while the replacement frame is pending.
            // If the cache is configured for a single entry, honor that cap and fall back
            // to the original replacement behavior.
            let protected_key = (self.max_entries > 1)
                .then_some(protected_key)
                .flatten()
                .filter(|protected| *protected != key);
            self.retain_only(protected_key);
            self.memory_bytes = self.memory_bytes.saturating_add(approx_bytes);
            self.entries.put(
                key,
                TerminalFrameEntry {
                    state: TerminalFrameState::PendingFrame(frame),
                    approx_bytes,
                },
            );
            return true;
        }

        // Preserve single-entry oversize recovery for current page:
        // while a lone oversize entry is resident, reject unrelated
        // non-oversize inserts (typically prefetch) that would evict it.
        if !allow_single_oversize
            && self.memory_bytes > self.memory_budget_bytes
            && self.entries.peek(&key).is_none()
        {
            return false;
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
        self.evict_while_needed(protected_key);
        true
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

    pub(crate) fn remove(&mut self, key: &TerminalFrameKey) -> bool {
        let Some(entry) = self.entries.pop(key) else {
            return false;
        };
        self.memory_bytes = self.memory_bytes.saturating_sub(entry.approx_bytes);
        true
    }

    fn evict_while_needed(&mut self, protected_key: Option<TerminalFrameKey>) {
        while self.entries.len() > self.max_entries || self.memory_bytes > self.memory_budget_bytes
        {
            if self.entries.len() == 1
                && protected_key.is_some_and(|key| self.entries.peek(&key).is_some())
            {
                break;
            }
            let Some((_key, entry)) = self.pop_lru_unprotected(protected_key) else {
                break;
            };
            self.memory_bytes = self.memory_bytes.saturating_sub(entry.approx_bytes);
        }
    }

    fn retain_only(&mut self, protected_key: Option<TerminalFrameKey>) {
        let doomed: Vec<_> = self
            .entries
            .iter()
            .map(|(key, _)| *key)
            .filter(|key| Some(*key) != protected_key)
            .collect();

        for key in doomed {
            let _ = self.remove(&key);
        }
    }

    fn pop_lru_unprotected(
        &mut self,
        protected_key: Option<TerminalFrameKey>,
    ) -> Option<(TerminalFrameKey, TerminalFrameEntry)> {
        let mut protected_entry = Vec::new();

        loop {
            match self.entries.pop_lru() {
                Some((key, entry)) if Some(key) == protected_key => {
                    protected_entry.push((key, entry));
                    continue;
                }
                Some(pair) => {
                    for (key, entry) in protected_entry.into_iter().rev() {
                        let _ = self.entries.push(key, entry);
                        let _ = self.entries.demote(&key);
                    }
                    return Some(pair);
                }
                None => {
                    for (key, entry) in protected_entry.into_iter().rev() {
                        let _ = self.entries.push(key, entry);
                        let _ = self.entries.demote(&key);
                    }
                    return None;
                }
            }
        }
    }
}
