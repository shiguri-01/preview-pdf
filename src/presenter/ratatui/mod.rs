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
use super::image_ops::font_size_px;
use super::l2_cache::{
    L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES, TerminalFrameCache, TerminalFrameKey,
    TerminalFrameState,
};
use super::terminal_cell::{picker_with_resolved_cell_size, protocol_type_label};
use super::traits::{
    ImagePresenter, PanOffset, PresenterBackgroundEvent, PresenterCaps, PresenterRenderOutcome,
    PresenterRenderSlot, PresenterRuntimeInfo, PresenterSlot, PresenterSlotOutcome, Viewport,
};

pub(super) const ENCODE_FAILURE_MESSAGE: &str = "failed to encode terminal image";

pub(super) struct PresenterConfig {
    pub(super) picker: Picker,
    pub(super) protocol_type: ProtocolType,
    pub(super) protocol_label: &'static str,
}

pub(super) struct PresenterState {
    pub(super) terminal_initialized: bool,
    pub(super) l2_cache: TerminalFrameCache,
    pub(super) perf_stats: PerfStats,
    pub(super) current_keys: Vec<Option<TerminalFrameKey>>,
    pub(super) last_ready_keys: Vec<Option<TerminalFrameKey>>,
    pub(super) last_drawn_keys: Vec<Option<TerminalFrameKey>>,
    pub(super) last_drawn_areas: Vec<Option<Rect>>,
    pub(super) current_generations: Vec<u64>,
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
    pub(super) config: PresenterConfig,
    pub(super) state: PresenterState,
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

    pub(super) fn shutdown_worker(&mut self) {
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
            cell_px: Some(font_size_px(self.config.picker.font_size())),
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

    fn reset_terminal_state(&mut self) {
        RatatuiImagePresenter::reset_terminal_state(self);
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
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    use crate::backend::RgbaFrame;
    use crate::presenter::l2_cache::TerminalFrameState;
    use crate::presenter::{
        ImagePresenter, PanOffset, PresenterFeedback, PresenterHorizontalAlign,
        PresenterRenderMode, PresenterRenderOptions, PresenterRenderSlot, PresenterSlot, Viewport,
    };
    use crate::render::cache::RenderedPageKey;

    fn frame() -> RgbaFrame {
        RgbaFrame {
            width: 4,
            height: 4,
            pixels: vec![200; 4 * 4 * 4].into(),
        }
    }

    fn render_until_ready(presenter: &mut super::RatatuiImagePresenter, area: Rect) {
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        let deadline = Instant::now() + Duration::from_secs(2);
        while presenter
            .state
            .last_ready_keys
            .first()
            .copied()
            .flatten()
            .is_none()
            && Instant::now() < deadline
        {
            terminal
                .draw(|frame| {
                    let _ = presenter.render(frame, area, PresenterRenderOptions::default());
                })
                .expect("draw should pass");
            let _ = presenter.drain_background_events();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            presenter
                .state
                .last_ready_keys
                .first()
                .copied()
                .flatten()
                .is_some(),
            "presenter should have a ready frame for fallback"
        );
    }

    #[test]
    fn render_pending_uses_stale_fallback_when_allowed() {
        let mut presenter = super::RatatuiImagePresenter::new();
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 12,
            height: 7,
        };
        let area = Rect::new(1, 1, 12, 7);
        presenter
            .prepare(
                RenderedPageKey::new(9, 1, 1.0),
                &frame(),
                viewport,
                PanOffset::default(),
                0,
                1,
            )
            .expect("first prepare should pass");
        render_until_ready(&mut presenter, area);
        presenter
            .prepare(
                RenderedPageKey::new(9, 2, 1.0),
                &frame(),
                viewport,
                PanOffset::default(),
                0,
                2,
            )
            .expect("second prepare should pass");

        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        let mut result = None;
        terminal
            .draw(|frame| {
                result = Some(presenter.render(
                    frame,
                    area,
                    PresenterRenderOptions::new(true, PresenterRenderMode::Full),
                ));
            })
            .expect("draw should pass");

        let outcome = result
            .expect("render result should be captured")
            .expect("render should succeed");
        assert_eq!(outcome.feedback, PresenterFeedback::Pending);
        assert!(outcome.drew_image);
        assert!(outcome.used_stale_fallback);
        assert_eq!(outcome.slots.len(), 1);
        assert_eq!(outcome.slots[0].area, area);
        assert!(outcome.slots[0].used_stale_fallback);
    }

    fn render_slots_until_ready(
        presenter: &mut super::RatatuiImagePresenter,
        slots: &[PresenterRenderSlot],
    ) {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        let deadline = Instant::now() + Duration::from_secs(2);
        while presenter.state.last_ready_keys.iter().any(Option::is_none)
            && Instant::now() < deadline
        {
            terminal
                .draw(|frame| {
                    let _ = presenter.render_slots(frame, slots);
                })
                .expect("draw should pass");
            let _ = presenter.drain_background_events();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            presenter.state.last_ready_keys.iter().all(Option::is_some),
            "presenter should have ready frames for all slots"
        );
    }
    #[test]
    fn presenter_tracks_last_drawn_area_for_stable_redraws() {
        let mut presenter = super::RatatuiImagePresenter::new();
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 12,
            height: 7,
        };
        let area = Rect::new(2, 1, 12, 7);
        presenter
            .prepare(
                RenderedPageKey::new(1, 0, 1.0),
                &frame(),
                viewport,
                PanOffset::default(),
                0,
                1,
            )
            .expect("prepare should pass");

        render_until_ready(&mut presenter, area);
        let first_drawn_area =
            presenter.state.last_drawn_areas[0].expect("ready render should record drawn area");

        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        terminal
            .draw(|frame| {
                presenter
                    .render(frame, area, PresenterRenderOptions::default())
                    .expect("ready redraw should pass");
            })
            .expect("draw should pass");

        assert_eq!(presenter.state.last_drawn_areas[0], Some(first_drawn_area));
    }
    #[test]
    fn presenter_preserves_stable_ready_image_without_reblitting() {
        let mut presenter = super::RatatuiImagePresenter::new();
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 12,
            height: 7,
        };
        let area = Rect::new(2, 1, 12, 7);
        presenter
            .prepare(
                RenderedPageKey::new(1, 0, 1.0),
                &frame(),
                viewport,
                PanOffset::default(),
                0,
                1,
            )
            .expect("prepare should pass");

        render_until_ready(&mut presenter, area);
        let first_drawn_key = presenter.state.last_drawn_keys[0];
        let first_drawn_area = presenter.state.last_drawn_areas[0];
        presenter.clear_perf_blit_metrics();

        let options = PresenterRenderOptions {
            preserve_stable_image: true,
            ..PresenterRenderOptions::default()
        };
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        let mut outcome = None;
        terminal
            .draw(|frame| {
                outcome = Some(
                    presenter
                        .render(frame, area, options)
                        .expect("stable redraw should pass"),
                );
            })
            .expect("draw should pass");

        assert!(
            outcome.expect("outcome should be captured").drew_image,
            "preserved image should still count as visible image content"
        );
        assert_eq!(presenter.state.last_drawn_keys[0], first_drawn_key);
        assert_eq!(presenter.state.last_drawn_areas[0], first_drawn_area);
        assert_eq!(presenter.perf_stats().blit_samples, 0);
    }
    #[test]
    fn inactive_spread_slot_forgets_last_drawn_area() {
        let mut presenter = super::RatatuiImagePresenter::new();
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 12,
            height: 7,
        };
        let left_area = Rect::new(0, 0, 12, 7);
        let right_area = Rect::new(15, 0, 12, 7);
        let slots = [
            PresenterRenderSlot {
                area: left_area,
                options: PresenterRenderOptions::default(),
                active: true,
                horizontal_align: PresenterHorizontalAlign::End,
            },
            PresenterRenderSlot {
                area: right_area,
                options: PresenterRenderOptions::default(),
                active: true,
                horizontal_align: PresenterHorizontalAlign::Start,
            },
        ];
        presenter
            .prepare_slots(&[
                PresenterSlot {
                    cache_key: Some(RenderedPageKey::new(1, 0, 1.0)),
                    frame: Some(&frame()),
                    viewport,
                    pan: PanOffset::default(),
                    overlay_stamp: 0,
                    generation: 1,
                },
                PresenterSlot {
                    cache_key: Some(RenderedPageKey::new(1, 1, 1.0)),
                    frame: Some(&frame()),
                    viewport,
                    pan: PanOffset::default(),
                    overlay_stamp: 0,
                    generation: 1,
                },
            ])
            .expect("slot prepare should pass");

        render_slots_until_ready(&mut presenter, &slots);
        assert!(
            presenter.state.last_drawn_areas[1].is_some(),
            "ready right slot should record a drawn area"
        );
        let right_drawn_area = presenter.state.last_drawn_areas[1].unwrap();

        let inactive_right = [
            slots[0],
            PresenterRenderSlot {
                area: Rect::default(),
                active: false,
                ..slots[1]
            },
        ];
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        terminal
            .draw(|frame| {
                for y in right_drawn_area.top()..right_drawn_area.bottom() {
                    for x in right_drawn_area.left()..right_drawn_area.right() {
                        if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                            cell.set_symbol("x");
                        }
                    }
                }
                presenter
                    .render_slots(frame, &inactive_right)
                    .expect("inactive render should pass");
            })
            .expect("draw should pass");

        let buffer = terminal.backend().buffer();
        for y in right_drawn_area.top()..right_drawn_area.bottom() {
            for x in right_drawn_area.left()..right_drawn_area.right() {
                assert_eq!(buffer[(x, y)].symbol(), " ");
            }
        }
        assert_eq!(presenter.state.last_drawn_areas[1], None);
        assert_eq!(presenter.state.last_drawn_keys[1], None);
    }
    #[test]
    fn presenter_prepare_slots_tracks_independent_current_keys() {
        let mut presenter = super::RatatuiImagePresenter::new();
        let left_viewport = Viewport {
            x: 0,
            y: 0,
            width: 40,
            height: 24,
        };
        let right_viewport = Viewport {
            x: 42,
            y: 0,
            width: 40,
            height: 24,
        };
        let left_key = RenderedPageKey::new(1, 0, 1.0);
        let right_key = RenderedPageKey::new(1, 1, 1.0);
        let left_frame = frame();
        let right_frame = frame();
        let slots = [
            PresenterSlot {
                cache_key: Some(left_key),
                frame: Some(&left_frame),
                viewport: left_viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
            PresenterSlot {
                cache_key: Some(right_key),
                frame: Some(&right_frame),
                viewport: right_viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
        ];

        presenter
            .prepare_slots(&slots)
            .expect("slot prepare should pass");

        assert_eq!(presenter.state.current_keys.len(), 2);
        assert_eq!(
            presenter.state.current_keys[0].map(|key| key.rendered_page),
            Some(left_key)
        );
        assert_eq!(
            presenter.state.current_keys[1].map(|key| key.rendered_page),
            Some(right_key)
        );
        assert_eq!(presenter.l2_cache_len(), 2);
    }
    #[test]
    fn presenter_prepare_slots_preserves_empty_slot_positions() {
        let mut presenter = super::RatatuiImagePresenter::new();
        let viewport = Viewport {
            x: 42,
            y: 0,
            width: 40,
            height: 24,
        };
        let right_key = RenderedPageKey::new(1, 2, 1.0);
        let right_frame = frame();
        let slots = [
            PresenterSlot {
                cache_key: None,
                frame: None,
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
            PresenterSlot {
                cache_key: Some(right_key),
                frame: Some(&right_frame),
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
        ];

        presenter
            .prepare_slots(&slots)
            .expect("slot prepare should pass");

        assert_eq!(presenter.state.current_keys.len(), 2);
        assert_eq!(presenter.state.current_keys[0], None);
        assert_eq!(
            presenter.state.current_keys[1].map(|key| key.rendered_page),
            Some(right_key)
        );
        assert_eq!(presenter.l2_cache_len(), 1);
    }
    #[test]
    fn render_slots_ignores_inactive_slots_for_feedback() {
        let mut presenter = super::RatatuiImagePresenter::new();
        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        let mut result = None;

        terminal
            .draw(|frame| {
                result = Some(presenter.render_slots(
                    frame,
                    &[PresenterRenderSlot {
                        area: Rect::new(0, 0, 12, 7),
                        options: PresenterRenderOptions::default(),
                        active: false,
                        horizontal_align: PresenterHorizontalAlign::Center,
                    }],
                ));
            })
            .expect("draw should pass");

        let outcome = result
            .expect("render result should be captured")
            .expect("render should pass");
        assert_eq!(outcome.feedback, PresenterFeedback::None);
        assert!(!outcome.drew_image);
        assert_eq!(outcome.slots.len(), 1);
        assert_eq!(outcome.slots[0].area, Rect::new(0, 0, 12, 7));
        assert!(!outcome.slots[0].active);
    }
    #[test]
    fn render_slots_aggregates_failed_and_pending_feedback() {
        let mut presenter = super::RatatuiImagePresenter::new();
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 12,
            height: 7,
        };
        let left_key = RenderedPageKey::new(1, 0, 1.0);
        let right_key = RenderedPageKey::new(1, 1, 1.0);
        let left_frame = frame();
        let right_frame = frame();
        let slots = [
            PresenterSlot {
                cache_key: Some(left_key),
                frame: Some(&left_frame),
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
            PresenterSlot {
                cache_key: Some(right_key),
                frame: Some(&right_frame),
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
        ];
        presenter
            .prepare_slots(&slots)
            .expect("slot prepare should pass");
        let failed_key = presenter.state.current_keys[0].expect("left key should exist");
        presenter
            .state
            .l2_cache
            .set_state(&failed_key, TerminalFrameState::Failed);

        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        let mut result = None;
        terminal
            .draw(|frame| {
                result = Some(presenter.render_slots(
                    frame,
                    &[
                        PresenterRenderSlot {
                            area: Rect::new(0, 0, 12, 7),
                            options: PresenterRenderOptions::default(),
                            active: true,
                            horizontal_align: PresenterHorizontalAlign::End,
                        },
                        PresenterRenderSlot {
                            area: Rect::new(15, 0, 12, 7),
                            options: PresenterRenderOptions::default(),
                            active: true,
                            horizontal_align: PresenterHorizontalAlign::Start,
                        },
                    ],
                ));
            })
            .expect("draw should pass");

        let outcome = result
            .expect("render result should be captured")
            .expect("render should pass");
        assert_eq!(outcome.feedback, PresenterFeedback::Failed);
        assert!(!outcome.drew_image);
        assert_eq!(outcome.slots.len(), 2);
        assert_eq!(outcome.slots[0].feedback, PresenterFeedback::Failed);
        assert_eq!(outcome.slots[1].feedback, PresenterFeedback::Pending);
    }
    #[test]
    fn render_slots_reports_stale_fallback_per_slot() {
        let mut presenter = super::RatatuiImagePresenter::new();
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 12,
            height: 7,
        };
        let left_area = Rect::new(0, 0, 12, 7);
        let right_area = Rect::new(15, 0, 12, 7);
        let left_key = RenderedPageKey::new(1, 0, 1.0);
        let right_key = RenderedPageKey::new(1, 1, 1.0);
        let next_right_key = RenderedPageKey::new(1, 2, 1.0);
        let left_frame = frame();
        let right_frame = frame();
        let next_right_frame = frame();
        let render_slots = [
            PresenterRenderSlot {
                area: left_area,
                options: PresenterRenderOptions::new(true, PresenterRenderMode::Full),
                active: true,
                horizontal_align: PresenterHorizontalAlign::End,
            },
            PresenterRenderSlot {
                area: right_area,
                options: PresenterRenderOptions::new(true, PresenterRenderMode::Full),
                active: true,
                horizontal_align: PresenterHorizontalAlign::Start,
            },
        ];

        presenter
            .prepare_slots(&[
                PresenterSlot {
                    cache_key: Some(left_key),
                    frame: Some(&left_frame),
                    viewport,
                    pan: PanOffset::default(),
                    overlay_stamp: 0,
                    generation: 1,
                },
                PresenterSlot {
                    cache_key: Some(right_key),
                    frame: Some(&right_frame),
                    viewport,
                    pan: PanOffset::default(),
                    overlay_stamp: 0,
                    generation: 1,
                },
            ])
            .expect("initial slot prepare should pass");
        render_slots_until_ready(&mut presenter, &render_slots);

        presenter
            .prepare_slots(&[
                PresenterSlot {
                    cache_key: Some(left_key),
                    frame: Some(&left_frame),
                    viewport,
                    pan: PanOffset::default(),
                    overlay_stamp: 0,
                    generation: 2,
                },
                PresenterSlot {
                    cache_key: Some(next_right_key),
                    frame: Some(&next_right_frame),
                    viewport,
                    pan: PanOffset::default(),
                    overlay_stamp: 0,
                    generation: 2,
                },
            ])
            .expect("second slot prepare should pass");

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        let mut result = None;
        terminal
            .draw(|frame| {
                result = Some(presenter.render_slots(frame, &render_slots));
            })
            .expect("draw should pass");

        let outcome = result
            .expect("render result should be captured")
            .expect("render should pass");
        assert_eq!(outcome.feedback, PresenterFeedback::Pending);
        assert!(outcome.drew_image);
        assert!(outcome.used_stale_fallback);
        assert_eq!(outcome.slots.len(), 2);
        assert_eq!(outcome.slots[0].area, left_area);
        assert_eq!(outcome.slots[0].feedback, PresenterFeedback::None);
        assert!(outcome.slots[0].drew_image);
        assert!(!outcome.slots[0].used_stale_fallback);
        assert_eq!(outcome.slots[1].area, right_area);
        assert_eq!(outcome.slots[1].feedback, PresenterFeedback::Pending);
        assert!(outcome.slots[1].drew_image);
        assert!(outcome.slots[1].used_stale_fallback);
    }
}
