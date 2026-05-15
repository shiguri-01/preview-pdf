use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui_image::Resize;
use ratatui_image::StatefulImage;
use ratatui_image::picker::Picker;
use ratatui_image::picker::ProtocolType;
use ratatui_image::protocol::StatefulProtocol;
use std::time::Instant;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};
use tokio::task::JoinHandle;

use crate::backend::RgbaFrame;
use crate::error::{AppError, AppResult};
use crate::perf::PerfStats;
use crate::render::cache::RenderedPageKey;
use crate::work::WorkClass;

use super::encode::{
    ENCODE_RESIZE_FILTER, EncodeLaneKind, EncodeWorkerEvent, EncodeWorkerRequest,
    EncodeWorkerResult, EncodeWorkerRuntime, send_encode_request, spawn_encode_worker,
};
use super::image_ops::{fit_downscale_dimensions, fit_resize_dimensions};
use super::l2_cache::{
    L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES, TerminalFrameCache, TerminalFrameKey,
    TerminalFrameState,
};
use super::terminal_cell::{picker_with_resolved_cell_size, protocol_type_label};
use super::traits::{
    ImagePresenter, PanOffset, PresenterBackgroundEvent, PresenterCaps, PresenterFeedback,
    PresenterHorizontalAlign, PresenterRenderOptions, PresenterRenderOutcome, PresenterRenderSlot,
    PresenterRuntimeInfo, PresenterSlot, PresenterSlotOutcome, Viewport,
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
                self.state
                    .perf_stats
                    .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                return Ok(None);
            }
        }

        self.state
            .perf_stats
            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
        Ok(Some(key))
    }

    fn ready_key_for_slot(&self, slot_index: usize) -> Option<TerminalFrameKey> {
        self.state
            .last_ready_keys
            .get(slot_index)
            .copied()
            .flatten()
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
                    self.state
                        .perf_stats
                        .set_l2_hit_rate(self.state.l2_cache.hit_rate());
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
        self.state
            .perf_stats
            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
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

    fn draw_protocol(
        frame: &mut Frame<'_>,
        area: Rect,
        protocol: &mut StatefulProtocol,
    ) -> AppResult<()> {
        frame.render_stateful_widget(
            StatefulImage::<StatefulProtocol>::default()
                .resize(Resize::Fit(Some(ENCODE_RESIZE_FILTER))),
            area,
            protocol,
        );

        if let Some(result) = protocol.last_encoding_result() {
            result.map_err(|_| AppError::unsupported(ENCODE_FAILURE_MESSAGE))?;
        }
        Ok(())
    }

    fn preserve_terminal_area(frame: &mut Frame<'_>, area: Rect) {
        // Do not redraw a stable terminal image just to satisfy ratatui's
        // immediate-mode buffer. Re-emitting the image protocol can race with
        // later text overlays and make the right spread slot appear to clear or
        // paint over the overlay; skipped cells keep the existing image intact.
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                    cell.set_skip(true);
                }
            }
        }
    }

    fn stable_slot_is_drawn(
        &self,
        slot_index: usize,
        key: TerminalFrameKey,
        render_area: Rect,
    ) -> bool {
        self.state
            .last_drawn_keys
            .get(slot_index)
            .is_some_and(|last_key| *last_key == Some(key))
            && self
                .state
                .last_drawn_areas
                .get(slot_index)
                .is_some_and(|last_area| *last_area == Some(render_area))
    }

    fn record_drawn_slot(&mut self, slot_index: usize, key: TerminalFrameKey, render_area: Rect) {
        if let Some(last_ready_key) = self.state.last_ready_keys.get_mut(slot_index) {
            *last_ready_key = Some(key);
        }
        if let Some(last_drawn_key) = self.state.last_drawn_keys.get_mut(slot_index) {
            *last_drawn_key = Some(key);
        }
        if let Some(last_drawn_area) = self.state.last_drawn_areas.get_mut(slot_index) {
            *last_drawn_area = Some(render_area);
        }
    }

    fn try_draw_ready_key(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        key: TerminalFrameKey,
        slot_index: usize,
        horizontal_align: PresenterHorizontalAlign,
        options: PresenterRenderOptions,
    ) -> AppResult<bool> {
        if self.state.l2_cache.cached_mut(&key).is_none() {
            if self
                .state
                .last_ready_keys
                .get(slot_index)
                .is_some_and(|last_key| *last_key == Some(key))
            {
                self.state.last_ready_keys[slot_index] = None;
            }
            return Ok(false);
        };
        let state = self
            .state
            .l2_cache
            .replace_state(&key, TerminalFrameState::Encoding)
            .expect("entry existence checked above");
        match state {
            TerminalFrameState::Ready(mut protocol) => {
                let blit_start = std::time::Instant::now();
                let target_size = protocol.size_for(Resize::Fit(Some(ENCODE_RESIZE_FILTER)), area);
                let render_area = align_rect_within(
                    area,
                    target_size.width,
                    target_size.height,
                    horizontal_align,
                );
                if options.preserve_stable_image
                    && !options.force_image_redraw
                    && self.stable_slot_is_drawn(slot_index, key, render_area)
                {
                    Self::preserve_terminal_area(frame, render_area);
                    self.state
                        .l2_cache
                        .set_state(&key, TerminalFrameState::Ready(protocol));
                    self.record_drawn_slot(slot_index, key, render_area);
                    self.state
                        .perf_stats
                        .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                    return Ok(true);
                }
                if options.force_image_redraw
                    || self
                        .state
                        .last_drawn_areas
                        .get(slot_index)
                        .is_none_or(|last_area| *last_area != Some(render_area))
                {
                    frame.render_widget(Clear, area);
                }
                if let Err(err) = Self::draw_protocol(frame, render_area, &mut protocol) {
                    self.state
                        .l2_cache
                        .set_state(&key, TerminalFrameState::Failed);
                    self.state
                        .perf_stats
                        .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                    return Err(err);
                }
                self.state.perf_stats.record_blit(blit_start.elapsed());
                self.state
                    .l2_cache
                    .set_state(&key, TerminalFrameState::Ready(protocol));
                self.record_drawn_slot(slot_index, key, render_area);
                self.state
                    .perf_stats
                    .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                Ok(true)
            }
            TerminalFrameState::PendingFrame(frame) => {
                self.state
                    .l2_cache
                    .set_state(&key, TerminalFrameState::PendingFrame(frame));
                self.state
                    .perf_stats
                    .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                Ok(false)
            }
            TerminalFrameState::Encoding => {
                self.state
                    .l2_cache
                    .set_state(&key, TerminalFrameState::Encoding);
                self.state
                    .perf_stats
                    .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                Ok(false)
            }
            TerminalFrameState::Failed => {
                self.state
                    .l2_cache
                    .set_state(&key, TerminalFrameState::Failed);
                if self
                    .state
                    .last_ready_keys
                    .get(slot_index)
                    .is_some_and(|last_key| *last_key == Some(key))
                {
                    self.state.last_ready_keys[slot_index] = None;
                }
                self.state
                    .perf_stats
                    .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                Ok(false)
            }
        }
    }

    fn try_draw_stale_fallback(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        current_key: Option<TerminalFrameKey>,
        slot_index: usize,
        horizontal_align: PresenterHorizontalAlign,
        options: PresenterRenderOptions,
    ) -> AppResult<bool> {
        let Some(last_key) = self
            .state
            .last_ready_keys
            .get(slot_index)
            .copied()
            .flatten()
        else {
            return Ok(false);
        };
        if Some(last_key) == current_key {
            return Ok(false);
        }
        self.try_draw_ready_key(frame, area, last_key, slot_index, horizontal_align, options)
    }

    fn is_current_slot_key(&self, key: TerminalFrameKey) -> bool {
        self.state.current_keys.contains(&Some(key))
    }

    fn render_slot(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        options: PresenterRenderOptions,
        slot_index: usize,
        horizontal_align: PresenterHorizontalAlign,
    ) -> AppResult<PresenterRenderOutcome> {
        if area.width == 0 || area.height == 0 {
            return Ok(PresenterRenderOutcome::from_slot(
                PresenterSlotOutcome::active(area, false, PresenterFeedback::Pending, false),
            ));
        }

        let current_key = self.state.current_keys.get(slot_index).copied().flatten();
        let Some(key) = current_key else {
            let drew_image = if options.allow_stale_fallback {
                self.try_draw_stale_fallback(
                    frame,
                    area,
                    None,
                    slot_index,
                    horizontal_align,
                    options,
                )?
            } else {
                false
            };
            return Ok(PresenterRenderOutcome::from_slot(
                PresenterSlotOutcome::active(
                    area,
                    drew_image,
                    PresenterFeedback::Pending,
                    drew_image,
                ),
            ));
        };
        let request_tx = self.encode_request_tx(EncodeLaneKind::Current);
        if self.state.l2_cache.cached_mut(&key).is_none() {
            self.state
                .perf_stats
                .set_l2_hit_rate(self.state.l2_cache.hit_rate());
            let drew_image = if options.allow_stale_fallback {
                self.try_draw_stale_fallback(
                    frame,
                    area,
                    Some(key),
                    slot_index,
                    horizontal_align,
                    options,
                )?
            } else {
                false
            };
            return Ok(PresenterRenderOutcome::from_slot(
                PresenterSlotOutcome::active(
                    area,
                    drew_image,
                    PresenterFeedback::Pending,
                    drew_image,
                ),
            ));
        }

        let feedback = {
            let state = self
                .state
                .l2_cache
                .replace_state(&key, TerminalFrameState::Encoding)
                .expect("current key existence checked above");
            match state {
                TerminalFrameState::Ready(mut protocol) => {
                    let blit_start = std::time::Instant::now();
                    let target_size =
                        protocol.size_for(Resize::Fit(Some(ENCODE_RESIZE_FILTER)), area);
                    let render_area = align_rect_within(
                        area,
                        target_size.width,
                        target_size.height,
                        horizontal_align,
                    );
                    if options.preserve_stable_image
                        && !options.force_image_redraw
                        && self.stable_slot_is_drawn(slot_index, key, render_area)
                    {
                        Self::preserve_terminal_area(frame, render_area);
                        self.state
                            .l2_cache
                            .set_state(&key, TerminalFrameState::Ready(protocol));
                        self.record_drawn_slot(slot_index, key, render_area);
                        self.state
                            .perf_stats
                            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                        return Ok(PresenterRenderOutcome::from_slot(
                            PresenterSlotOutcome::active(
                                area,
                                true,
                                PresenterFeedback::None,
                                false,
                            ),
                        ));
                    }
                    if options.force_image_redraw
                        || self
                            .state
                            .last_drawn_areas
                            .get(slot_index)
                            .is_none_or(|last_area| *last_area != Some(render_area))
                    {
                        frame.render_widget(Clear, area);
                    }
                    if let Err(err) = Self::draw_protocol(frame, render_area, &mut protocol) {
                        self.state
                            .l2_cache
                            .set_state(&key, TerminalFrameState::Failed);
                        self.state
                            .perf_stats
                            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                        return Err(err);
                    }
                    self.state.perf_stats.record_blit(blit_start.elapsed());
                    self.state
                        .l2_cache
                        .set_state(&key, TerminalFrameState::Ready(protocol));
                    self.record_drawn_slot(slot_index, key, render_area);
                    self.state
                        .perf_stats
                        .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                    return Ok(PresenterRenderOutcome::from_slot(
                        PresenterSlotOutcome::active(area, true, PresenterFeedback::None, false),
                    ));
                }
                TerminalFrameState::PendingFrame(frame) => {
                    let picker = if options.is_initial_preview() {
                        Picker::halfblocks()
                    } else {
                        self.config.picker.clone()
                    };
                    let encode_area = aligned_fit_area(
                        frame.width,
                        frame.height,
                        picker.font_size(),
                        area,
                        horizontal_align,
                        options.is_initial_preview(),
                    );
                    let request = EncodeWorkerRequest::Encode {
                        key,
                        picker,
                        frame,
                        area: encode_area,
                        allow_upscale: options.is_initial_preview(),
                        class: WorkClass::CriticalCurrent,
                        generation: self
                            .state
                            .current_generations
                            .get(slot_index)
                            .copied()
                            .unwrap_or(0),
                        enqueued_at: Instant::now(),
                    };

                    let (new_state, feedback) = match send_encode_request(&request_tx, request) {
                        Ok(()) => (TerminalFrameState::Encoding, PresenterFeedback::Pending),
                        Err(err) => match *err {
                            EncodeWorkerRequest::Encode { .. } | EncodeWorkerRequest::Shutdown => {
                                (TerminalFrameState::Failed, PresenterFeedback::Failed)
                            }
                        },
                    };
                    self.state.l2_cache.set_state(&key, new_state);
                    feedback
                }
                TerminalFrameState::Encoding => {
                    self.state
                        .l2_cache
                        .set_state(&key, TerminalFrameState::Encoding);
                    PresenterFeedback::Pending
                }
                TerminalFrameState::Failed => {
                    self.state
                        .l2_cache
                        .set_state(&key, TerminalFrameState::Failed);
                    PresenterFeedback::Failed
                }
            }
        };
        self.state
            .perf_stats
            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
        let drew_image = if options.allow_stale_fallback {
            self.try_draw_stale_fallback(
                frame,
                area,
                Some(key),
                slot_index,
                horizontal_align,
                options,
            )?
        } else {
            false
        };
        Ok(PresenterRenderOutcome::from_slot(
            PresenterSlotOutcome::active(area, drew_image, feedback, drew_image),
        ))
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

    fn prepare(
        &mut self,
        cache_key: RenderedPageKey,
        frame: &RgbaFrame,
        viewport: Viewport,
        pan: PanOffset,
        overlay_stamp: u64,
        generation: u64,
    ) -> AppResult<()> {
        self.drain_encode_results();
        let previous_ready_key = self.ready_key_for_slot(0);
        let protected_ready_keys = self.protected_ready_keys();
        let frame_key = TerminalFrameKey {
            rendered_page: cache_key,
            viewport,
            pan,
            overlay_stamp,
        };
        let Some(key) = self.ensure_frame_entry(frame_key, frame, true, &protected_ready_keys)?
        else {
            self.state.current_keys = vec![None];
            self.state.last_ready_keys = vec![previous_ready_key];
            self.state.last_drawn_keys.resize(1, None);
            self.state.last_drawn_areas.resize(1, None);
            self.state.current_generations = vec![generation];
            return Ok(());
        };
        self.state.current_keys = vec![Some(key)];
        self.state.last_ready_keys = vec![previous_ready_key];
        self.state.last_drawn_keys.resize(1, None);
        self.state.last_drawn_areas.resize(1, None);
        self.state.current_generations = vec![generation];
        Ok(())
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
        let font_size = self.config.picker.font_size();
        let request_tx = self.encode_request_tx(EncodeLaneKind::Background);
        if self.state.l2_cache.cached_mut(&key).is_none() {
            self.state
                .perf_stats
                .set_l2_hit_rate(self.state.l2_cache.hit_rate());
            return Ok(());
        };

        let state = self
            .state
            .l2_cache
            .replace_state(&key, TerminalFrameState::Encoding)
            .expect("entry existence checked above");
        match state {
            TerminalFrameState::PendingFrame(frame) => {
                let area = centered_fit_area(frame.width, frame.height, font_size, viewport_area);
                let request = EncodeWorkerRequest::Encode {
                    key,
                    picker: self.config.picker.clone(),
                    frame,
                    area,
                    allow_upscale: false,
                    class,
                    generation,
                    enqueued_at: Instant::now(),
                };
                let new_state = match send_encode_request(&request_tx, request) {
                    Ok(()) => TerminalFrameState::Encoding,
                    Err(err) => match *err {
                        EncodeWorkerRequest::Encode { frame, .. } => {
                            TerminalFrameState::PendingFrame(frame)
                        }
                        EncodeWorkerRequest::Shutdown => TerminalFrameState::Failed,
                    },
                };
                self.state.l2_cache.set_state(&key, new_state);
            }
            TerminalFrameState::Encoding => {
                self.state
                    .l2_cache
                    .set_state(&key, TerminalFrameState::Encoding);
            }
            TerminalFrameState::Ready(protocol) => {
                self.state
                    .l2_cache
                    .set_state(&key, TerminalFrameState::Ready(protocol));
            }
            TerminalFrameState::Failed => {
                self.state
                    .l2_cache
                    .set_state(&key, TerminalFrameState::Failed);
            }
        }

        self.state
            .perf_stats
            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
        Ok(())
    }

    fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        options: PresenterRenderOptions,
    ) -> AppResult<PresenterRenderOutcome> {
        self.drain_encode_results();
        if self.state.current_keys.is_empty() {
            self.state.current_keys = vec![None];
            self.state.last_ready_keys = vec![None];
            self.state.last_drawn_keys.resize(1, None);
            self.state.last_drawn_areas.resize(1, None);
            self.state.current_generations = vec![0];
        }
        self.render_slot(frame, area, options, 0, PresenterHorizontalAlign::Center)
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

fn centered_fit_area(
    image_width_px: u32,
    image_height_px: u32,
    font_size: (u16, u16),
    area: Rect,
) -> Rect {
    aligned_fit_area(
        image_width_px,
        image_height_px,
        font_size,
        area,
        PresenterHorizontalAlign::Center,
        false,
    )
}

fn aligned_fit_area(
    image_width_px: u32,
    image_height_px: u32,
    font_size: (u16, u16),
    area: Rect,
    horizontal_align: PresenterHorizontalAlign,
    allow_upscale: bool,
) -> Rect {
    if area.width == 0 || area.height == 0 {
        return area;
    }

    let cell_width_px = u32::from(font_size.0.max(1));
    let cell_height_px = u32::from(font_size.1.max(1));
    let max_width_px = u32::from(area.width).saturating_mul(cell_width_px);
    let max_height_px = u32::from(area.height).saturating_mul(cell_height_px);

    let fit_dimensions = if allow_upscale {
        fit_resize_dimensions(
            image_width_px,
            image_height_px,
            max_width_px,
            max_height_px,
            true,
        )
    } else {
        fit_downscale_dimensions(image_width_px, image_height_px, max_width_px, max_height_px)
    };
    let (fit_width_px, fit_height_px) = fit_dimensions.unwrap_or((image_width_px, image_height_px));

    let width_cells = px_to_cells(fit_width_px, cell_width_px, area.width);
    let height_cells = px_to_cells(fit_height_px, cell_height_px, area.height);
    align_rect_within(area, width_cells, height_cells, horizontal_align)
}

fn px_to_cells(px: u32, cell_px: u32, max_cells: u16) -> u16 {
    let cells = px.saturating_add(cell_px.saturating_sub(1)) / cell_px.max(1);
    cells.max(1).min(u32::from(max_cells)) as u16
}

#[cfg(test)]
fn center_rect_within(area: Rect, width: u16, height: u16) -> Rect {
    align_rect_within(area, width, height, PresenterHorizontalAlign::Center)
}

fn align_rect_within(
    area: Rect,
    width: u16,
    height: u16,
    horizontal_align: PresenterHorizontalAlign,
) -> Rect {
    let width = width.max(1).min(area.width);
    let height = height.max(1).min(area.height);
    let spare_width = area.width.saturating_sub(width);
    let x = match horizontal_align {
        PresenterHorizontalAlign::Start => area.x,
        PresenterHorizontalAlign::Center => area.x + spare_width / 2,
        PresenterHorizontalAlign::End => area.x + spare_width,
    };
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    use super::{align_rect_within, center_rect_within, centered_fit_area};
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

    #[test]
    fn center_rect_within_places_rect_in_the_middle() {
        let area = Rect::new(10, 5, 20, 10);
        let centered = center_rect_within(area, 8, 4);
        assert_eq!(centered, Rect::new(16, 8, 8, 4));
    }

    #[test]
    fn align_rect_within_can_pin_to_horizontal_edges() {
        let area = Rect::new(10, 5, 20, 10);

        assert_eq!(
            align_rect_within(area, 8, 4, PresenterHorizontalAlign::Start),
            Rect::new(10, 8, 8, 4)
        );
        assert_eq!(
            align_rect_within(area, 8, 4, PresenterHorizontalAlign::End),
            Rect::new(22, 8, 8, 4)
        );
    }

    #[test]
    fn centered_fit_area_keeps_aspect_and_centers() {
        let area = Rect::new(0, 0, 40, 20);
        let fit = centered_fit_area(2000, 1000, (10, 20), area);
        assert_eq!(fit, Rect::new(0, 5, 40, 10));
    }
}
