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
use super::image_ops::fit_downscale_dimensions;
use super::l2_cache::{
    L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES, TerminalFrameCache, TerminalFrameKey,
    TerminalFrameState,
};
use super::terminal_cell::{picker_with_resolved_cell_size, protocol_type_label};
use super::traits::{
    ImagePresenter, PanOffset, PresenterBackgroundEvent, PresenterCaps, PresenterFeedback,
    PresenterRenderOptions, PresenterRenderOutcome, PresenterRuntimeInfo, Viewport,
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
    pub(crate) current_key: Option<TerminalFrameKey>,
    pub(crate) last_ready_key: Option<TerminalFrameKey>,
    pub(crate) current_generation: u64,
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
                current_key: None,
                last_ready_key: None,
                current_generation: 0,
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
        self.state.current_key = None;
        self.state.last_ready_key = None;
        self.state.current_generation = 0;
        self.encode.current.state = EncodeLaneState::default();
        self.encode.background.state = EncodeLaneState::default();
        self.sync_encode_perf_stats();
    }

    fn ensure_frame_entry(
        &mut self,
        cache_key: RenderedPageKey,
        frame: &RgbaFrame,
        viewport: Viewport,
        pan: PanOffset,
        overlay_stamp: u64,
        allow_single_oversize: bool,
    ) -> AppResult<Option<TerminalFrameKey>> {
        let key = TerminalFrameKey {
            rendered_page: cache_key,
            viewport,
            pan,
            overlay_stamp,
        };

        if self.state.l2_cache.lookup_mut(&key).is_none() {
            let inserted = self.state.l2_cache.insert(
                key,
                frame.clone(),
                frame.byte_len(),
                allow_single_oversize,
                self.state.last_ready_key,
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

    fn handle_encode_result(
        &mut self,
        done: EncodeWorkerResult,
    ) -> Option<PresenterBackgroundEvent> {
        let current_key = self.state.current_key;
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
                        redraw_requested: Some(key) == current_key,
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
                    redraw_requested: Some(key) == current_key,
                })
            }
            EncodeWorkerEvent::CanceledStale { key } => {
                let removed = self.state.l2_cache.remove(&key);
                self.state.perf_stats.add_encode_canceled_tasks(1);
                if removed && Some(key) == self.state.last_ready_key {
                    self.state.last_ready_key = None;
                }

                Some(PresenterBackgroundEvent::EncodeComplete {
                    redraw_requested: removed && Some(key) == current_key,
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

    fn try_draw_ready_key(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        key: TerminalFrameKey,
    ) -> AppResult<bool> {
        if self.state.l2_cache.cached_mut(&key).is_none() {
            if Some(key) == self.state.last_ready_key {
                self.state.last_ready_key = None;
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
                let render_area = center_rect_within(area, target_size.width, target_size.height);
                frame.render_widget(Clear, area);
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
                self.state.last_ready_key = Some(key);
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
                if Some(key) == self.state.last_ready_key {
                    self.state.last_ready_key = None;
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
    ) -> AppResult<bool> {
        let Some(last_key) = self.state.last_ready_key else {
            return Ok(false);
        };
        if Some(last_key) == current_key {
            return Ok(false);
        }
        self.try_draw_ready_key(frame, area, last_key)
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
        let Some(key) =
            self.ensure_frame_entry(cache_key, frame, viewport, pan, overlay_stamp, true)?
        else {
            self.state.current_key = None;
            return Ok(());
        };
        self.state.current_key = Some(key);
        self.state.current_generation = generation;
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
        let Some(key) =
            self.ensure_frame_entry(cache_key, frame, viewport, pan, overlay_stamp, false)?
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

        if area.width == 0 || area.height == 0 {
            return Ok(PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::Pending,
                used_stale_fallback: false,
            });
        }

        let Some(key) = self.state.current_key else {
            let drew_image = if options.allow_stale_fallback {
                self.try_draw_stale_fallback(frame, area, None)?
            } else {
                false
            };
            return Ok(PresenterRenderOutcome {
                drew_image,
                feedback: PresenterFeedback::Pending,
                used_stale_fallback: drew_image,
            });
        };
        let request_tx = self.encode_request_tx(EncodeLaneKind::Current);
        if self.state.l2_cache.cached_mut(&key).is_none() {
            self.state
                .perf_stats
                .set_l2_hit_rate(self.state.l2_cache.hit_rate());
            let drew_image = if options.allow_stale_fallback {
                self.try_draw_stale_fallback(frame, area, Some(key))?
            } else {
                false
            };
            return Ok(PresenterRenderOutcome {
                drew_image,
                feedback: PresenterFeedback::Pending,
                used_stale_fallback: drew_image,
            });
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
                    // Ensure stale cells are removed when a fresh current frame is smaller
                    // than the previously drawn content.
                    frame.render_widget(Clear, area);
                    let target_size =
                        protocol.size_for(Resize::Fit(Some(ENCODE_RESIZE_FILTER)), area);
                    let render_area =
                        center_rect_within(area, target_size.width, target_size.height);
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
                    self.state.last_ready_key = Some(key);
                    self.state
                        .perf_stats
                        .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                    return Ok(PresenterRenderOutcome {
                        drew_image: true,
                        feedback: PresenterFeedback::None,
                        used_stale_fallback: false,
                    });
                }
                TerminalFrameState::PendingFrame(frame) => {
                    let picker = if options.is_initial_preview() {
                        Picker::halfblocks()
                    } else {
                        self.config.picker.clone()
                    };
                    let encode_area = if options.is_initial_preview() {
                        area
                    } else {
                        centered_fit_area(frame.width, frame.height, picker.font_size(), area)
                    };
                    let request = EncodeWorkerRequest::Encode {
                        key,
                        picker,
                        frame,
                        area: encode_area,
                        allow_upscale: options.is_initial_preview(),
                        class: WorkClass::CriticalCurrent,
                        generation: self.state.current_generation,
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
            self.try_draw_stale_fallback(frame, area, Some(key))?
        } else {
            false
        };
        Ok(PresenterRenderOutcome {
            drew_image,
            feedback,
            used_stale_fallback: drew_image,
        })
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
    if area.width == 0 || area.height == 0 {
        return area;
    }

    let cell_width_px = u32::from(font_size.0.max(1));
    let cell_height_px = u32::from(font_size.1.max(1));
    let max_width_px = u32::from(area.width).saturating_mul(cell_width_px);
    let max_height_px = u32::from(area.height).saturating_mul(cell_height_px);

    let (fit_width_px, fit_height_px) =
        fit_downscale_dimensions(image_width_px, image_height_px, max_width_px, max_height_px)
            .unwrap_or((image_width_px, image_height_px));

    let width_cells = px_to_cells(fit_width_px, cell_width_px, area.width);
    let height_cells = px_to_cells(fit_height_px, cell_height_px, area.height);
    center_rect_within(area, width_cells, height_cells)
}

fn px_to_cells(px: u32, cell_px: u32, max_cells: u16) -> u16 {
    let cells = px.saturating_add(cell_px.saturating_sub(1)) / cell_px.max(1);
    cells.max(1).min(u32::from(max_cells)) as u16
}

fn center_rect_within(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.max(1).min(area.width);
    let height = height.max(1).min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{center_rect_within, centered_fit_area};

    #[test]
    fn center_rect_within_places_rect_in_the_middle() {
        let area = Rect::new(10, 5, 20, 10);
        let centered = center_rect_within(area, 8, 4);
        assert_eq!(centered, Rect::new(16, 8, 8, 4));
    }

    #[test]
    fn centered_fit_area_keeps_aspect_and_centers() {
        let area = Rect::new(0, 0, 40, 20);
        let fit = centered_fit_area(2000, 1000, (10, 20), area);
        assert_eq!(fit, Rect::new(0, 5, 40, 10));
    }
}
