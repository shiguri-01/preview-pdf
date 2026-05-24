use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use ratatui_image::Resize;
use ratatui_image::StatefulImage;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use tokio::sync::mpsc::UnboundedSender;

use crate::backend::RgbaFrame;
use crate::error::{AppError, AppResult};
use crate::work::WorkClass;

use super::super::encode::{
    ENCODE_RESIZE_FILTER, EncodeLaneKind, EncodeWorkerRequest, send_encode_request,
};
use super::super::l2_cache::{TerminalFrameKey, TerminalFrameState};
use super::super::traits::{
    PresenterFeedback, PresenterHorizontalAlign, PresenterRenderOptions, PresenterRenderOutcome,
    PresenterSlotOutcome,
};
use super::geometry::{align_rect_within, aligned_fit_area, centered_fit_area};
use super::{ENCODE_FAILURE_MESSAGE, RatatuiImagePresenter};

#[derive(Debug, Clone, Copy)]
struct ReadyDrawTarget {
    area: Rect,
    slot_index: usize,
    horizontal_align: PresenterHorizontalAlign,
    options: PresenterRenderOptions,
}

impl RatatuiImagePresenter {
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

    fn draw_ready_protocol(
        &mut self,
        frame: &mut Frame<'_>,
        key: TerminalFrameKey,
        mut protocol: Box<StatefulProtocol>,
        target: ReadyDrawTarget,
    ) -> AppResult<bool> {
        let blit_start = std::time::Instant::now();
        let target_size = protocol.size_for(Resize::Fit(Some(ENCODE_RESIZE_FILTER)), target.area);
        let render_area = align_rect_within(
            target.area,
            target_size.width,
            target_size.height,
            target.horizontal_align,
        );
        if target.options.preserve_stable_image
            && !target.options.force_image_redraw
            && self.stable_slot_is_drawn(target.slot_index, key, render_area)
        {
            Self::preserve_terminal_area(frame, render_area);
            self.set_l2_state(key, TerminalFrameState::Ready(protocol));
            self.record_drawn_slot(target.slot_index, key, render_area);
            return Ok(true);
        }
        if target.options.force_image_redraw
            || self
                .state
                .last_drawn_areas
                .get(target.slot_index)
                .is_none_or(|last_area| *last_area != Some(render_area))
        {
            frame.render_widget(Clear, target.area);
        }
        if let Err(err) = Self::draw_protocol(frame, render_area, &mut protocol) {
            self.set_l2_state(key, TerminalFrameState::Failed);
            return Err(err);
        }
        self.state.perf_stats.record_blit(blit_start.elapsed());
        self.set_l2_state(key, TerminalFrameState::Ready(protocol));
        self.record_drawn_slot(target.slot_index, key, render_area);
        Ok(true)
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
            self.forget_last_ready_key_if_matches(slot_index, key);
            return Ok(false);
        };
        let state = self
            .state
            .l2_cache
            .replace_state(&key, TerminalFrameState::Encoding)
            .expect("entry existence checked above");
        match state {
            TerminalFrameState::Ready(protocol) => self.draw_ready_protocol(
                frame,
                key,
                protocol,
                ReadyDrawTarget {
                    area,
                    slot_index,
                    horizontal_align,
                    options,
                },
            ),
            TerminalFrameState::PendingFrame(frame) => {
                self.set_l2_state(key, TerminalFrameState::PendingFrame(frame));
                Ok(false)
            }
            TerminalFrameState::Encoding => {
                self.set_l2_state(key, TerminalFrameState::Encoding);
                Ok(false)
            }
            TerminalFrameState::Failed => {
                self.set_l2_state(key, TerminalFrameState::Failed);
                self.forget_last_ready_key_if_matches(slot_index, key);
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

    pub(super) fn is_current_slot_key(&self, key: TerminalFrameKey) -> bool {
        self.state.current_keys.contains(&Some(key))
    }

    fn current_encode_request(
        &self,
        key: TerminalFrameKey,
        frame: RgbaFrame,
        area: Rect,
        slot_index: usize,
        horizontal_align: PresenterHorizontalAlign,
        options: PresenterRenderOptions,
    ) -> EncodeWorkerRequest {
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
        EncodeWorkerRequest::Encode {
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
        }
    }

    pub(super) fn background_encode_request(
        &self,
        key: TerminalFrameKey,
        frame: RgbaFrame,
        viewport_area: Rect,
        class: WorkClass,
        generation: u64,
    ) -> EncodeWorkerRequest {
        let area = centered_fit_area(
            frame.width,
            frame.height,
            self.config.picker.font_size(),
            viewport_area,
        );
        EncodeWorkerRequest::Encode {
            key,
            picker: self.config.picker.clone(),
            frame,
            area,
            allow_upscale: false,
            class,
            generation,
            enqueued_at: Instant::now(),
        }
    }

    fn enqueue_current_encode(
        request_tx: &Option<UnboundedSender<EncodeWorkerRequest>>,
        request: EncodeWorkerRequest,
    ) -> (TerminalFrameState, PresenterFeedback) {
        match send_encode_request(request_tx, request) {
            Ok(()) => (TerminalFrameState::Encoding, PresenterFeedback::Pending),
            Err(err) => match *err {
                EncodeWorkerRequest::Encode { .. } | EncodeWorkerRequest::Shutdown => {
                    (TerminalFrameState::Failed, PresenterFeedback::Failed)
                }
            },
        }
    }

    pub(super) fn enqueue_background_encode(
        request_tx: &Option<UnboundedSender<EncodeWorkerRequest>>,
        request: EncodeWorkerRequest,
    ) -> TerminalFrameState {
        match send_encode_request(request_tx, request) {
            Ok(()) => TerminalFrameState::Encoding,
            Err(err) => match *err {
                EncodeWorkerRequest::Encode { frame, .. } => {
                    TerminalFrameState::PendingFrame(frame)
                }
                EncodeWorkerRequest::Shutdown => TerminalFrameState::Failed,
            },
        }
    }

    pub(super) fn render_slot(
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
            self.sync_l2_hit_rate();
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
                TerminalFrameState::Ready(protocol) => {
                    self.draw_ready_protocol(
                        frame,
                        key,
                        protocol,
                        ReadyDrawTarget {
                            area,
                            slot_index,
                            horizontal_align,
                            options,
                        },
                    )?;
                    return Ok(PresenterRenderOutcome::from_slot(
                        PresenterSlotOutcome::active(area, true, PresenterFeedback::None, false),
                    ));
                }
                TerminalFrameState::PendingFrame(frame) => {
                    let request = self.current_encode_request(
                        key,
                        frame,
                        area,
                        slot_index,
                        horizontal_align,
                        options,
                    );
                    let (new_state, feedback) = Self::enqueue_current_encode(&request_tx, request);
                    self.set_l2_state(key, new_state);
                    feedback
                }
                TerminalFrameState::Encoding => {
                    self.set_l2_state(key, TerminalFrameState::Encoding);
                    PresenterFeedback::Pending
                }
                TerminalFrameState::Failed => {
                    self.set_l2_state(key, TerminalFrameState::Failed);
                    PresenterFeedback::Failed
                }
            }
        };
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
