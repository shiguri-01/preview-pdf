use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui_image::Resize;
use ratatui_image::StatefulImage;
use ratatui_image::picker::Picker;
use ratatui_image::picker::ProtocolType;
use ratatui_image::protocol::StatefulProtocol;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};
use tokio::task::JoinHandle;

use crate::backend::RgbaFrame;
use crate::error::{AppError, AppResult};
use crate::perf::PerfStats;
use crate::render::cache::RenderedPageKey;
use crate::render::prefetch::PrefetchClass;

use super::encode::{
    ENCODE_RESIZE_FILTER, EncodeWorkerRequest, EncodeWorkerResult, EncodeWorkerRuntime,
    send_encode_request, spawn_encode_worker,
};
use super::image_ops::fit_downscale_dimensions;
use super::l2_cache::{
    L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES, TerminalFrameCache, TerminalFrameKey,
    TerminalFrameState,
};
use super::terminal_cell::{picker_with_resolved_cell_size, protocol_type_label};
use super::traits::{ImagePresenter, PanOffset, PresenterCaps, PresenterRuntimeInfo, Viewport};

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
    pub(crate) current_generation: u64,
}

struct EncodeChannel {
    request_tx: Option<UnboundedSender<EncodeWorkerRequest>>,
    result_rx: UnboundedReceiver<EncodeWorkerResult>,
    _runtime: EncodeWorkerRuntime,
    worker: Option<JoinHandle<()>>,
}

pub struct RatatuiImagePresenter {
    pub(crate) config: PresenterConfig,
    pub(crate) state: PresenterState,
    encode: EncodeChannel,
}

impl Default for RatatuiImagePresenter {
    fn default() -> Self {
        Self::with_cache_limits(L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES)
    }
}

impl RatatuiImagePresenter {
    pub fn with_cache_limits(l2_max_entries: usize, l2_memory_budget_bytes: usize) -> Self {
        let runtime = EncodeWorkerRuntime::new();
        let (request_tx, result_rx, worker) = spawn_encode_worker(&runtime);
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
                current_generation: 0,
            },
            encode: EncodeChannel {
                request_tx: Some(request_tx),
                result_rx,
                _runtime: runtime,
                worker: Some(worker),
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

    fn ensure_frame_entry(
        &mut self,
        cache_key: RenderedPageKey,
        frame: &RgbaFrame,
        viewport: Viewport,
        pan: PanOffset,
    ) -> AppResult<TerminalFrameKey> {
        let key = TerminalFrameKey {
            rendered_page: cache_key,
            viewport,
            pan,
        };

        if self.state.l2_cache.lookup_mut(&key).is_none() {
            self.state
                .l2_cache
                .insert(key, frame.clone(), frame.byte_len());
        }

        self.state
            .perf_stats
            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
        Ok(key)
    }

    fn drain_encode_results(&mut self) -> bool {
        let mut changed = false;
        let current_key = self.state.current_key;

        loop {
            match self.encode.result_rx.try_recv() {
                Ok(done) => {
                    let Some(entry) = self.state.l2_cache.cached_mut(&done.key) else {
                        continue;
                    };

                    if done.succeeded {
                        if let Some(protocol) = done.protocol {
                            entry.state = TerminalFrameState::Ready(Box::new(protocol));
                        } else {
                            entry.state = TerminalFrameState::Failed;
                        }
                        self.state.perf_stats.record_convert(done.elapsed);
                    } else {
                        entry.state = TerminalFrameState::Failed;
                    }

                    if Some(done.key) == current_key {
                        changed = true;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        self.state
            .perf_stats
            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
        changed
    }

    pub(crate) fn shutdown_worker(&mut self) {
        if let Some(request_tx) = self.encode.request_tx.take() {
            let _ = request_tx.send(EncodeWorkerRequest::Shutdown);
        }
        if let Some(worker) = self.encode.worker.take() {
            worker.abort();
        }
    }

    fn draw_protocol(
        frame: &mut Frame<'_>,
        area: Rect,
        protocol: &mut StatefulProtocol,
    ) -> AppResult<()> {
        frame.render_widget(Clear, area);
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
            self.state.l2_cache.clear();
            self.state.current_key = None;
            self.state.current_generation = 0;
        }

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
        generation: u64,
    ) -> AppResult<()> {
        self.drain_encode_results();
        let key = self.ensure_frame_entry(cache_key, frame, viewport, pan)?;
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
        class: PrefetchClass,
        generation: u64,
    ) -> AppResult<()> {
        self.drain_encode_results();
        let key = self.ensure_frame_entry(cache_key, frame, viewport, pan)?;

        let viewport_area = Rect::new(
            viewport.x,
            viewport.y,
            viewport.width.max(1),
            viewport.height.max(1),
        );
        let font_size = self.config.picker.font_size();
        let request_tx = self.encode.request_tx.clone();
        let Some(entry) = self.state.l2_cache.cached_mut(&key) else {
            self.state
                .perf_stats
                .set_l2_hit_rate(self.state.l2_cache.hit_rate());
            return Ok(());
        };

        let state = std::mem::replace(&mut entry.state, TerminalFrameState::Encoding);
        match state {
            TerminalFrameState::PendingFrame(frame) => {
                let area = centered_fit_area(frame.width, frame.height, font_size, viewport_area);
                let request = EncodeWorkerRequest::Encode {
                    key,
                    picker: self.config.picker.clone(),
                    frame,
                    area,
                    class,
                    generation,
                };
                match send_encode_request(&request_tx, request) {
                    Ok(()) => {
                        entry.state = TerminalFrameState::Encoding;
                    }
                    Err(err) => match err {
                        EncodeWorkerRequest::Encode { frame, .. } => {
                            entry.state = TerminalFrameState::PendingFrame(frame);
                        }
                        EncodeWorkerRequest::Shutdown => {
                            entry.state = TerminalFrameState::Failed;
                        }
                    },
                }
            }
            TerminalFrameState::Encoding => {
                entry.state = TerminalFrameState::Encoding;
            }
            TerminalFrameState::Ready(protocol) => {
                entry.state = TerminalFrameState::Ready(protocol);
            }
            TerminalFrameState::Failed => {
                entry.state = TerminalFrameState::Failed;
            }
        }

        self.state
            .perf_stats
            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame<'_>, area: Rect) -> AppResult<bool> {
        self.drain_encode_results();

        if area.width == 0 || area.height == 0 {
            return Ok(false);
        }

        let Some(key) = self.state.current_key else {
            return Ok(false);
        };
        let font_size = self.config.picker.font_size();
        let request_tx = self.encode.request_tx.clone();
        let Some(entry) = self.state.l2_cache.cached_mut(&key) else {
            return Ok(false);
        };

        let state = std::mem::replace(&mut entry.state, TerminalFrameState::Encoding);
        match state {
            TerminalFrameState::Ready(mut protocol) => {
                let blit_start = std::time::Instant::now();
                let target_size = protocol.size_for(Resize::Fit(Some(ENCODE_RESIZE_FILTER)), area);
                let render_area = center_rect_within(area, target_size.width, target_size.height);
                if let Err(err) = Self::draw_protocol(frame, render_area, &mut protocol) {
                    entry.state = TerminalFrameState::Failed;
                    self.state
                        .perf_stats
                        .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                    return Err(err);
                }
                self.state.perf_stats.record_blit(blit_start.elapsed());
                entry.state = TerminalFrameState::Ready(protocol);
                self.state
                    .perf_stats
                    .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                Ok(true)
            }
            TerminalFrameState::PendingFrame(frame) => {
                let encode_area = centered_fit_area(frame.width, frame.height, font_size, area);
                let request = EncodeWorkerRequest::Encode {
                    key,
                    picker: self.config.picker.clone(),
                    frame,
                    area: encode_area,
                    class: PrefetchClass::CriticalCurrent,
                    generation: self.state.current_generation,
                };

                match send_encode_request(&request_tx, request) {
                    Ok(()) => {
                        entry.state = TerminalFrameState::Encoding;
                        self.state
                            .perf_stats
                            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                        Ok(false)
                    }
                    Err(EncodeWorkerRequest::Encode { .. } | EncodeWorkerRequest::Shutdown) => {
                        entry.state = TerminalFrameState::Failed;
                        self.state
                            .perf_stats
                            .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                        Err(AppError::unsupported(ENCODE_FAILURE_MESSAGE))
                    }
                }
            }
            TerminalFrameState::Encoding => {
                entry.state = TerminalFrameState::Encoding;
                self.state
                    .perf_stats
                    .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                Ok(false)
            }
            TerminalFrameState::Failed => {
                entry.state = TerminalFrameState::Failed;
                self.state
                    .perf_stats
                    .set_l2_hit_rate(self.state.l2_cache.hit_rate());
                Err(AppError::unsupported(ENCODE_FAILURE_MESSAGE))
            }
        }
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

    fn drain_background_events(&mut self) -> bool {
        self.drain_encode_results()
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
