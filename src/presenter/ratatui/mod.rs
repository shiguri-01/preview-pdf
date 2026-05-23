use ratatui::Frame;

mod draw;
mod geometry;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui_image::picker::Picker;
use ratatui_image::picker::ProtocolType;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};
use tokio::task::JoinHandle;

use crate::backend::RgbaFrame;
use crate::error::AppResult;
use crate::perf::PerfStats;
use crate::render::cache::RenderedPageKey;
use crate::work::WorkClass;

use super::encode::{
    EncodeLaneKind, EncodeWorkerEvent, EncodeWorkerRequest, EncodeWorkerResult,
    EncodeWorkerRuntime, spawn_encode_worker,
};
use super::l2_cache::{
    L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES, TerminalFrameCache, TerminalFrameKey,
    TerminalFrameState,
};
use super::terminal_cell::{picker_with_resolved_cell_size, protocol_type_label};
use super::traits::{
    ImagePresenter, PanOffset, PresenterBackgroundEvent, PresenterCaps, PresenterRenderOutcome,
    PresenterRenderSlot, PresenterRuntimeInfo, PresenterSlot, PresenterSlotOutcome, Viewport,
};

pub(crate) const ENCODE_FAILURE_MESSAGE: &str = "failed to encode terminal image";

pub(crate) struct PresenterConfig {
    pub(crate) picker: Picker,
    pub(crate) protocol_type: ProtocolType,
    pub(crate) protocol_label: &'static str,
}

pub(crate) struct PresenterState {
    pub(crate) terminal_initialized: bool,
    pub(crate) l2_cache: TerminalFrameCache,
    pub(crate) perf_stats: PerfStats,
    pub(crate) current_keys: Vec<Option<TerminalFrameKey>>,
    pub(crate) last_ready_keys: Vec<Option<TerminalFrameKey>>,
    pub(crate) last_drawn_keys: Vec<Option<TerminalFrameKey>>,
    pub(crate) last_drawn_areas: Vec<Option<Rect>>,
    pub(crate) current_generations: Vec<u64>,
}

#[derive(Default)]
struct EncodeLaneState {
    depth: usize,
    in_flight: usize,
}

struct EncodeLane {
    request_tx: Option<UnboundedSender<EncodeWorkerRequest>>,
    result_rx: UnboundedReceiver<EncodeWorkerResult>,
    worker: Option<JoinHandle<()>>,
    state: EncodeLaneState,
}

struct EncodeChannels {
    _runtime: EncodeWorkerRuntime,
    current: EncodeLane,
    background: EncodeLane,
}

pub struct RatatuiImagePresenter {
    pub(crate) config: PresenterConfig,
    pub(crate) state: PresenterState,
    encode: EncodeChannels,
}

impl Default for RatatuiImagePresenter {
    fn default() -> Self {
        Self::with_cache_limits(L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES)
    }
}

impl RatatuiImagePresenter {
    pub fn with_cache_limits(l2_max_entries: usize, l2_memory_budget_bytes: usize) -> Self {
        let runtime = EncodeWorkerRuntime::new();
        let (current_tx, current_rx, current_worker) =
            spawn_encode_worker(&runtime, EncodeLaneKind::Current);
        let (background_tx, background_rx, background_worker) =
            spawn_encode_worker(&runtime, EncodeLaneKind::Background);
        Self {
            config: PresenterConfig {
                picker: Picker::halfblocks(),
                protocol_type: ProtocolType::Halfblocks,
                protocol_label: "halfblocks",
            },
            state: PresenterState {
                terminal_initialized: false,
                l2_cache: TerminalFrameCache::new(l2_max_entries, l2_memory_budget_bytes),
                perf_stats: PerfStats::default(),
                current_keys: Vec::new(),
                last_ready_keys: Vec::new(),
                last_drawn_keys: Vec::new(),
                last_drawn_areas: Vec::new(),
                current_generations: Vec::new(),
            },
            encode: EncodeChannels {
                _runtime: runtime,
                current: EncodeLane {
                    request_tx: Some(current_tx),
                    result_rx: current_rx,
                    worker: Some(current_worker),
                    state: EncodeLaneState::default(),
                },
                background: EncodeLane {
                    request_tx: Some(background_tx),
                    result_rx: background_rx,
                    worker: Some(background_worker),
                    state: EncodeLaneState::default(),
                },
            },
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn perf_stats(&self) -> &PerfStats {
        &self.state.perf_stats
    }

    pub fn l2_cache_len(&self) -> usize {
        self.state.l2_cache.len()
    }

    fn encode_request_tx(
        &self,
        lane: EncodeLaneKind,
    ) -> Option<UnboundedSender<EncodeWorkerRequest>> {
        match lane {
            EncodeLaneKind::Current => self.encode.current.request_tx.clone(),
            EncodeLaneKind::Background => self.encode.background.request_tx.clone(),
        }
    }

    fn sync_encode_perf_stats(&mut self) {
        let depth = self.encode.current.state.depth + self.encode.background.state.depth;
        let in_flight =
            self.encode.current.state.in_flight + self.encode.background.state.in_flight;
        self.state.perf_stats.set_encode_queue_depth(depth);
        self.state.perf_stats.set_encode_in_flight(in_flight);
    }

    fn sync_l2_hit_rate(&mut self) {
        self.state
            .perf_stats
            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
    }

    fn set_l2_state(&mut self, key: TerminalFrameKey, state: TerminalFrameState) {
        self.state.l2_cache.set_state(&key, state);
        self.sync_l2_hit_rate();
    }

    fn forget_last_ready_key_if_matches(&mut self, slot_index: usize, key: TerminalFrameKey) {
        if self
            .state
            .last_ready_keys
            .get(slot_index)
            .is_some_and(|last_key| *last_key == Some(key))
        {
            self.state.last_ready_keys[slot_index] = None;
        }
    }

    fn set_encode_lane_state(&mut self, lane: EncodeLaneKind, depth: usize, in_flight: usize) {
        let state = match lane {
            EncodeLaneKind::Current => &mut self.encode.current.state,
            EncodeLaneKind::Background => &mut self.encode.background.state,
        };
        state.depth = depth;
        state.in_flight = in_flight;
        self.sync_encode_perf_stats();
    }

    fn drain_encode_lane(&mut self, lane: EncodeLaneKind) -> Result<bool, TryRecvError> {
        let result = match lane {
            EncodeLaneKind::Current => self.encode.current.result_rx.try_recv(),
            EncodeLaneKind::Background => self.encode.background.result_rx.try_recv(),
        };

        match result {
            Ok(done) => Ok(matches!(
                self.handle_encode_result(done),
                Some(PresenterBackgroundEvent::EncodeComplete {
                    redraw_requested: true
                })
            )),
            Err(err) => Err(err),
        }
    }

    fn reset_terminal_state(&mut self) {
        self.state.l2_cache.clear();
        self.state.current_keys.clear();
        self.state.last_ready_keys.clear();
        self.state.last_drawn_keys.clear();
        self.state.last_drawn_areas.clear();
        self.state.current_generations.clear();
        self.encode.current.state = EncodeLaneState::default();
        self.encode.background.state = EncodeLaneState::default();
        self.sync_encode_perf_stats();
    }

    fn ensure_frame_entry(
        &mut self,
        key: TerminalFrameKey,
        frame: &RgbaFrame,
        allow_single_oversize: bool,
        protected_keys: &[TerminalFrameKey],
    ) -> AppResult<Option<TerminalFrameKey>> {
        if self.state.l2_cache.lookup_mut(&key).is_none() {
            let inserted = self.state.l2_cache.insert_protected(
                key,
                frame.clone(),
                frame.byte_len(),
                allow_single_oversize,
                protected_keys,
            );
            if !inserted {
                self.sync_l2_hit_rate();
                return Ok(None);
            }
        }

        self.sync_l2_hit_rate();
        Ok(Some(key))
    }

    fn protected_ready_keys(&self) -> Vec<TerminalFrameKey> {
        self.state
            .last_ready_keys
            .iter()
            .copied()
            .flatten()
            .collect()
    }

    fn handle_encode_result(
        &mut self,
        done: EncodeWorkerResult,
    ) -> Option<PresenterBackgroundEvent> {
        let lane = done.lane;
        let event = match done.event {
            EncodeWorkerEvent::Completed {
                key,
                protocol,
                queue_wait,
                elapsed,
                succeeded,
            } => {
                self.state.perf_stats.record_encode_queue_wait(queue_wait);
                if succeeded {
                    self.state.perf_stats.record_convert(elapsed);
                }

                if self.state.l2_cache.cached_mut(&key).is_none() {
                    self.sync_l2_hit_rate();
                    return Some(PresenterBackgroundEvent::EncodeComplete {
                        redraw_requested: self.is_current_slot_key(key),
                    });
                };

                let state = if succeeded {
                    if let Some(protocol) = protocol {
                        TerminalFrameState::Ready(protocol)
                    } else {
                        TerminalFrameState::Failed
                    }
                } else {
                    TerminalFrameState::Failed
                };
                self.state.l2_cache.set_state(&key, state);

                Some(PresenterBackgroundEvent::EncodeComplete {
                    redraw_requested: self.is_current_slot_key(key),
                })
            }
            EncodeWorkerEvent::CanceledStale { key } => {
                let removed = self.state.l2_cache.remove(&key);
                self.state.perf_stats.add_encode_canceled_tasks(1);
                if removed {
                    for last_ready_key in &mut self.state.last_ready_keys {
                        if *last_ready_key == Some(key) {
                            *last_ready_key = None;
                        }
                    }
                }

                Some(PresenterBackgroundEvent::EncodeComplete {
                    redraw_requested: removed && self.is_current_slot_key(key),
                })
            }
            EncodeWorkerEvent::QueueState { depth, in_flight } => {
                self.set_encode_lane_state(lane, depth, in_flight);
                None
            }
        };
        self.sync_l2_hit_rate();
        event
    }

    fn drain_encode_results(&mut self) -> bool {
        let mut changed = false;

        loop {
            let mut drained_any = false;

            match self.drain_encode_lane(EncodeLaneKind::Current) {
                Ok(redraw) => {
                    changed |= redraw;
                    drained_any = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => break,
            }

            match self.drain_encode_lane(EncodeLaneKind::Background) {
                Ok(redraw) => {
                    changed |= redraw;
                    drained_any = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => break,
            }

            if !drained_any {
                break;
            }
        }

        changed
    }

    pub(crate) fn shutdown_worker(&mut self) {
        if let Some(request_tx) = self.encode.current.request_tx.take() {
            let _ = request_tx.send(EncodeWorkerRequest::Shutdown);
        }
        if let Some(request_tx) = self.encode.background.request_tx.take() {
            let _ = request_tx.send(EncodeWorkerRequest::Shutdown);
        }
        if let Some(worker) = self.encode.current.worker.take() {
            worker.abort();
        }
        if let Some(worker) = self.encode.background.worker.take() {
            worker.abort();
        }
    }
}

impl ImagePresenter for RatatuiImagePresenter {
    fn initialize_terminal(&mut self) -> AppResult<()> {
        if self.state.terminal_initialized {
            return Ok(());
        }

        if let Ok(picker) = Picker::from_query_stdio() {
            let protocol_type = picker.protocol_type();
            self.config.protocol_type = protocol_type;
            self.config.protocol_label = protocol_type_label(protocol_type);
            self.config.picker = picker_with_resolved_cell_size(picker, protocol_type);
            self.reset_terminal_state();
        }

        self.state.terminal_initialized = true;
        Ok(())
    }

    fn initialize_headless_for_perf(&mut self) -> AppResult<()> {
        self.config.protocol_type = ProtocolType::Halfblocks;
        self.config.protocol_label = "halfblocks";
        self.config.picker = Picker::halfblocks();
        self.reset_terminal_state();
        self.state.perf_stats.reset();
        self.state.terminal_initialized = true;
        Ok(())
    }

    fn status_label(&self) -> String {
        format!("ratatui-image/{}", self.config.protocol_label)
    }

    fn runtime_info(&self) -> PresenterRuntimeInfo {
        PresenterRuntimeInfo {
            graphics_protocol: Some(self.config.protocol_label),
        }
    }

    fn prepare_slots(&mut self, slots: &[PresenterSlot<'_>]) -> AppResult<()> {
        self.drain_encode_results();
        let protected_ready_keys = self.protected_ready_keys();
        self.state.current_keys.clear();
        self.state.current_generations.clear();
        self.state.last_ready_keys.resize(slots.len(), None);
        self.state.last_drawn_keys.resize(slots.len(), None);
        self.state.last_drawn_areas.resize(slots.len(), None);

        for slot in slots {
            let key = if let (Some(cache_key), Some(frame)) = (slot.cache_key, slot.frame) {
                let frame_key = TerminalFrameKey {
                    rendered_page: cache_key,
                    viewport: slot.viewport,
                    pan: slot.pan,
                    overlay_stamp: slot.overlay_stamp,
                };
                self.ensure_frame_entry(frame_key, frame, true, &protected_ready_keys)?
            } else {
                None
            };
            self.state.current_keys.push(key);
            self.state.current_generations.push(slot.generation);
        }
        Ok(())
    }

    fn prefetch_encode(
        &mut self,
        cache_key: RenderedPageKey,
        frame: &RgbaFrame,
        viewport: Viewport,
        pan: PanOffset,
        overlay_stamp: u64,
        class: WorkClass,
        generation: u64,
    ) -> AppResult<()> {
        self.drain_encode_results();
        debug_assert_ne!(class, WorkClass::CriticalCurrent);
        let protected_ready_keys = self.protected_ready_keys();
        let frame_key = TerminalFrameKey {
            rendered_page: cache_key,
            viewport,
            pan,
            overlay_stamp,
        };
        let Some(key) = self.ensure_frame_entry(frame_key, frame, false, &protected_ready_keys)?
        else {
            return Ok(());
        };

        let viewport_area = Rect::new(
            viewport.x,
            viewport.y,
            viewport.width.max(1),
            viewport.height.max(1),
        );
        let request_tx = self.encode_request_tx(EncodeLaneKind::Background);
        if self.state.l2_cache.cached_mut(&key).is_none() {
            self.sync_l2_hit_rate();
            return Ok(());
        };

        let state = self
            .state
            .l2_cache
            .replace_state(&key, TerminalFrameState::Encoding)
            .expect("entry existence checked above");
        match state {
            TerminalFrameState::PendingFrame(frame) => {
                let request =
                    self.background_encode_request(key, frame, viewport_area, class, generation);
                let new_state = Self::enqueue_background_encode(&request_tx, request);
                self.set_l2_state(key, new_state);
            }
            TerminalFrameState::Encoding => {
                self.set_l2_state(key, TerminalFrameState::Encoding);
            }
            TerminalFrameState::Ready(protocol) => {
                self.set_l2_state(key, TerminalFrameState::Ready(protocol));
            }
            TerminalFrameState::Failed => {
                self.set_l2_state(key, TerminalFrameState::Failed);
            }
        }

        Ok(())
    }

    fn render_slots(
        &mut self,
        frame: &mut Frame<'_>,
        slots: &[PresenterRenderSlot],
    ) -> AppResult<PresenterRenderOutcome> {
        self.drain_encode_results();
        let mut slot_outcomes = Vec::with_capacity(slots.len());
        for (slot_index, slot) in slots.iter().enumerate() {
            if !slot.active {
                frame.render_widget(Clear, slot.area);
                let last_drawn_area = self
                    .state
                    .last_drawn_areas
                    .get(slot_index)
                    .copied()
                    .flatten();
                if let Some(last_drawn_area) = last_drawn_area
                    && last_drawn_area != slot.area
                {
                    frame.render_widget(Clear, last_drawn_area);
                }
                if let Some(last_drawn_key) = self.state.last_drawn_keys.get_mut(slot_index) {
                    *last_drawn_key = None;
                }
                if let Some(last_drawn_area) = self.state.last_drawn_areas.get_mut(slot_index) {
                    *last_drawn_area = None;
                }
                slot_outcomes.push(PresenterSlotOutcome::inactive(slot.area));
                continue;
            }
            let slot_outcome = self.render_slot(
                frame,
                slot.area,
                slot.options,
                slot_index,
                slot.horizontal_align,
            )?;
            if slot_outcome.slots.is_empty() {
                slot_outcomes.push(PresenterSlotOutcome::active(
                    slot.area,
                    slot_outcome.drew_image,
                    slot_outcome.feedback,
                    slot_outcome.used_stale_fallback,
                ));
            } else {
                slot_outcomes.extend(slot_outcome.slots);
            }
        }
        Ok(PresenterRenderOutcome::aggregate_slots(slot_outcomes))
    }

    fn capabilities(&self) -> PresenterCaps {
        PresenterCaps {
            backend_name: "ratatui-image",
            supports_l2_cache: true,
            cell_px: Some(self.config.picker.font_size()),
            preferred_max_render_scale: preferred_max_render_scale(self.config.protocol_type),
        }
    }

    fn has_pending_work(&self) -> bool {
        self.state.l2_cache.has_pending_work()
    }

    fn perf_snapshot(&self) -> Option<PerfStats> {
        Some(self.state.perf_stats.clone())
    }

    fn reset_perf_metrics(&mut self) {
        self.state.perf_stats.reset();
    }

    fn enable_perf_sample_collection(&mut self) {
        self.state.perf_stats.enable_sample_collection();
    }

    fn clear_perf_blit_metrics(&mut self) {
        self.state.perf_stats.clear_blit_metrics();
    }

    fn drain_background_events(&mut self) -> bool {
        self.drain_encode_results()
    }

    fn recv_background_event<'a>(
        &'a mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<PresenterBackgroundEvent>> + 'a>>
    {
        Box::pin(async move {
            loop {
                let done = tokio::select! {
                    done = self.encode.current.result_rx.recv() => done,
                    done = self.encode.background.result_rx.recv() => done,
                }?;
                if let Some(event) = self.handle_encode_result(done) {
                    return Some(event);
                }
            }
        })
    }
}

impl Drop for RatatuiImagePresenter {
    fn drop(&mut self) {
        self.shutdown_worker();
    }
}

fn preferred_max_render_scale(protocol: ProtocolType) -> f32 {
    match protocol {
        ProtocolType::Kitty | ProtocolType::Iterm2 | ProtocolType::Sixel => 2.5,
        ProtocolType::Halfblocks => 1.0,
    }
}

#[cfg(test)]
mod tests;
