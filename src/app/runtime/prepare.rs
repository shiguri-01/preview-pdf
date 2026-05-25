use ratatui::layout::Rect;

use crate::backend::{PdfBackend, RgbaFrame};
use crate::error::AppResult;
use crate::highlight::HighlightOverlaySnapshot;
use crate::presenter::{
    ImagePresenter, PanOffset, PresenterHorizontalAlign, PresenterRenderMode,
    PresenterRenderOptions, PresenterRenderSlot, PresenterSlot, Viewport,
};
use crate::render::cache::RenderedPageKey;
use crate::work::WorkClass;

use super::super::frame_ops::{
    PageRenderSpace, apply_highlight_overlay, crop_frame_region, prepare_presenter_frame,
};
use super::super::state::VisiblePageSlots;
use super::RenderRuntime;
use super::spread_canvas::{self, SpreadCanvasLayoutRequest, SpreadCanvasPage};

#[cfg(test)]
use crate::render::scheduler::RenderTask;

#[derive(Debug, Clone, Copy)]
pub(crate) struct FramePrepareOptions<'a> {
    pub(crate) cell_px: Option<(u16, u16)>,
    pub(crate) crop: bool,
    pub(crate) overlay: &'a HighlightOverlaySnapshot,
}

#[derive(Debug, Clone, Copy)]
#[cfg(test)]
pub(crate) struct CurrentPagePrepareRequest<'a> {
    pub(crate) viewport: Viewport,
    pub(crate) page: usize,
    pub(crate) scale: f32,
    pub(crate) pan: PanOffset,
    pub(crate) options: FramePrepareOptions<'a>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PageSlotPrepareRequest<'a> {
    pub(crate) page_slots: &'a [(Option<RenderedPageKey>, Viewport)],
    pub(crate) pan: PanOffset,
    pub(crate) options: FramePrepareOptions<'a>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SpreadCanvasPrepareRequest<'a> {
    pub(crate) viewport: Viewport,
    pub(crate) visible_pages: VisiblePageSlots,
    pub(crate) scale: f32,
    pub(crate) pan: PanOffset,
    pub(crate) cell_px: Option<(u16, u16)>,
    pub(crate) overlay: &'a HighlightOverlaySnapshot,
    pub(crate) gap_px: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrefetchEncodeRequest {
    pub(crate) viewport: Viewport,
    pub(crate) key: RenderedPageKey,
    pub(crate) pan: PanOffset,
    pub(crate) overlay_stamp: u64,
    pub(crate) cell_px: Option<(u16, u16)>,
    pub(crate) crop: bool,
    pub(crate) class: WorkClass,
    pub(crate) generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CachePrepareResult<T> {
    Prepared(T),
    Miss { pan: PanOffset },
}

pub(crate) struct PreparedPresenterSlots {
    slots: Vec<Option<PreparedPresenterSlot>>,
    pan: PanOffset,
}

impl PreparedPresenterSlots {
    fn new(slots: Vec<Option<PreparedPresenterSlot>>, pan: PanOffset) -> Self {
        Self { slots, pan }
    }

    #[cfg(test)]
    fn single(slot: PreparedPresenterSlot, pan: PanOffset) -> Self {
        Self::new(vec![Some(slot)], pan)
    }

    pub(crate) fn pan(&self) -> PanOffset {
        self.pan
    }

    pub(crate) fn presenter_slots(&self, generation: u64) -> Vec<PresenterSlot<'_>> {
        self.slots
            .iter()
            .map(|slot| presenter_slot_from_prepared(slot.as_ref(), generation))
            .collect()
    }
}

pub(crate) struct PreparedSpreadCanvas {
    presenter_slots: [Option<PreparedPresenterSlot>; 2],
    render_slots: [PreparedRenderSlot; 2],
    pan: PanOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreparedRenderSlot {
    area: Rect,
    active: bool,
}

impl PreparedRenderSlot {
    const fn inactive() -> Self {
        Self {
            area: Rect::new(0, 0, 0, 0),
            active: false,
        }
    }

    const fn active(area: Rect) -> Self {
        Self { area, active: true }
    }
}

impl PreparedSpreadCanvas {
    pub(crate) fn pan(&self) -> PanOffset {
        self.pan
    }

    #[cfg(test)]
    pub(crate) fn render_areas(&self) -> [Option<Rect>; 2] {
        self.render_slots
            .map(|slot| slot.active.then_some(slot.area))
    }

    pub(crate) fn presenter_slots(&self, generation: u64) -> Vec<PresenterSlot<'_>> {
        self.presenter_slots
            .iter()
            .map(|slot| presenter_slot_from_prepared(slot.as_ref(), generation))
            .collect()
    }

    pub(crate) fn render_slots(
        &self,
        render_mode: PresenterRenderMode,
    ) -> Vec<PresenterRenderSlot> {
        let options = PresenterRenderOptions::new(false, render_mode);
        self.render_slots
            .into_iter()
            .enumerate()
            .map(|(index, slot)| PresenterRenderSlot {
                area: slot.area,
                options,
                active: slot.active,
                horizontal_align: if index == 0 {
                    PresenterHorizontalAlign::End
                } else {
                    PresenterHorizontalAlign::Start
                },
            })
            .collect()
    }
}

struct PreparedPresenterSlot {
    cache_key: RenderedPageKey,
    frame: RgbaFrame,
    viewport: Viewport,
    pan: PanOffset,
    overlay_stamp: u64,
}

struct CachedDecoratedPage {
    key: RenderedPageKey,
    frame: RgbaFrame,
    overlay_stamp: u64,
}

struct SpreadCanvasSlotPage {
    page: Option<CachedDecoratedPage>,
    geometry: Option<SpreadCanvasPage>,
    active: bool,
}

impl RenderRuntime {
    #[cfg(test)]
    pub(crate) fn prepare_current_page(
        &mut self,
        doc: &dyn PdfBackend,
        request: CurrentPagePrepareRequest<'_>,
    ) -> AppResult<PreparedPresenterSlots> {
        let task = RenderTask {
            doc_id: doc.doc_id(),
            page: request.page,
            scale: request.scale,
            class: WorkClass::CriticalCurrent,
            generation: 0,
            reason: "current-page",
        };
        let frame = self.resolve_task_frame(doc, &task)?;
        let (frame, overlay_stamp) =
            decorate_single_page_frame(doc, task.page, &frame, request.options.overlay);
        let mut pan = request.pan;
        let (frame, pan_for_presenter) = prepare_presenter_frame(
            &frame,
            request.viewport,
            &mut pan,
            request.options.cell_px,
            request.options.crop,
        );

        Ok(PreparedPresenterSlots::single(
            PreparedPresenterSlot {
                cache_key: RenderedPageKey::new(task.doc_id, task.page, task.scale),
                frame,
                viewport: request.viewport,
                pan: pan_for_presenter,
                overlay_stamp,
            },
            pan,
        ))
    }

    pub(crate) fn prepare_page_slots_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        request: PageSlotPrepareRequest<'_>,
    ) -> AppResult<CachePrepareResult<PreparedPresenterSlots>> {
        let Some(mut prepared) = self.build_page_slots_from_cache(doc, request)? else {
            self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
            return Ok(CachePrepareResult::Miss { pan: request.pan });
        };

        if prepared.pan != request.pan {
            let normalized_request = PageSlotPrepareRequest {
                pan: prepared.pan,
                ..request
            };
            if let Some(rebuilt) = self.build_page_slots_from_cache(doc, normalized_request)? {
                prepared = rebuilt;
            }
        }

        Ok(CachePrepareResult::Prepared(prepared))
    }

    pub(crate) fn prepare_spread_canvas_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        request: SpreadCanvasPrepareRequest<'_>,
    ) -> AppResult<CachePrepareResult<PreparedSpreadCanvas>> {
        let left = self.spread_canvas_slot_page(
            doc,
            request.visible_pages.left_page,
            request.scale,
            request.overlay,
        )?;
        let right = self.spread_canvas_slot_page(
            doc,
            request.visible_pages.right_page,
            request.scale,
            request.overlay,
        )?;
        let pages = [left.geometry, right.geometry];
        let layout = spread_canvas::layout(SpreadCanvasLayoutRequest {
            pages,
            viewport: request.viewport,
            pan: request.pan,
            cell_px: request.cell_px,
            gap_px: request.gap_px,
        });
        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());

        let canvas_pages = [left, right];
        let mut presenter_slots = [None, None];
        let mut render_slots = [
            PreparedRenderSlot::inactive(),
            PreparedRenderSlot::inactive(),
        ];
        for (index, (page, clip)) in canvas_pages.into_iter().zip(layout.clips).enumerate() {
            let Some(clip) = clip else {
                continue;
            };
            if page.active {
                render_slots[index] = PreparedRenderSlot::active(clip.render_area);
            }
            if let Some(page) = page.page {
                let frame = crop_frame_region(
                    &page.frame,
                    clip.crop_x,
                    clip.crop_y,
                    clip.crop_width,
                    clip.crop_height,
                );
                presenter_slots[index] = Some(PreparedPresenterSlot {
                    cache_key: page.key,
                    frame,
                    viewport: clip.viewport,
                    pan: layout.pan,
                    overlay_stamp: page.overlay_stamp,
                });
            }
        }

        if presenter_slots.iter().all(Option::is_none) {
            return Ok(CachePrepareResult::Miss { pan: layout.pan });
        }
        Ok(CachePrepareResult::Prepared(PreparedSpreadCanvas {
            presenter_slots,
            render_slots,
            pan: layout.pan,
        }))
    }

    pub(crate) fn try_prefetch_encode_from_cache(
        &mut self,
        presenter: &mut dyn ImagePresenter,
        request: PrefetchEncodeRequest,
    ) -> AppResult<bool> {
        if request.overlay_stamp != 0 {
            // Prefetch encoding has no overlay snapshot to apply, so skip it while highlights are
            // active instead of caching an undecorated frame under the highlighted identity.
            return Ok(false);
        }
        let prepared = if let Some(frame) = self.l1_cache.get(&request.key) {
            let mut pan = request.pan;
            let (frame, pan_for_presenter) = prepare_presenter_frame(
                frame,
                request.viewport,
                &mut pan,
                request.cell_px,
                request.crop,
            );
            presenter.prefetch_encode(
                request.key,
                &frame,
                request.viewport,
                pan_for_presenter,
                request.overlay_stamp,
                request.class,
                request.generation,
            )?;
            true
        } else {
            false
        };
        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
        Ok(prepared)
    }

    fn build_page_slots_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        request: PageSlotPrepareRequest<'_>,
    ) -> AppResult<Option<PreparedPresenterSlots>> {
        let mut prepared = Vec::new();
        let mut normalized_pan = PanOffset {
            cells_x: request.pan.cells_x.max(0),
            cells_y: request.pan.cells_y.max(0),
        };
        let mut saw_cached_page = false;

        for (key, viewport) in request.page_slots {
            let Some(key) = *key else {
                prepared.push(None);
                continue;
            };
            let Some(frame) = self.l1_cache.get(&key) else {
                prepared.push(None);
                continue;
            };
            saw_cached_page = true;
            let (frame, overlay_stamp) =
                decorate_single_page_frame(doc, key.page, frame, request.options.overlay);
            let mut slot_pan = request.pan;
            let (frame, pan_for_presenter) = prepare_presenter_frame(
                &frame,
                *viewport,
                &mut slot_pan,
                request.options.cell_px,
                request.options.crop,
            );
            normalized_pan.cells_x = normalized_pan.cells_x.min(slot_pan.cells_x);
            normalized_pan.cells_y = normalized_pan.cells_y.min(slot_pan.cells_y);
            prepared.push(Some(PreparedPresenterSlot {
                cache_key: key,
                frame,
                viewport: *viewport,
                pan: pan_for_presenter,
                overlay_stamp,
            }));
        }

        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
        Ok(saw_cached_page.then(|| PreparedPresenterSlots::new(prepared, normalized_pan)))
    }

    fn cached_decorated_page(
        &mut self,
        doc: &dyn PdfBackend,
        page: Option<usize>,
        scale: f32,
        overlay: &HighlightOverlaySnapshot,
    ) -> AppResult<Option<CachedDecoratedPage>> {
        let Some(page) = page else {
            return Ok(None);
        };
        let key = RenderedPageKey::new(doc.doc_id(), page, scale);
        let Some(frame) = self.l1_cache.get(&key) else {
            return Ok(None);
        };
        let (frame, overlay_stamp) = decorate_single_page_frame(doc, page, frame, overlay);
        Ok(Some(CachedDecoratedPage {
            key,
            frame,
            overlay_stamp,
        }))
    }

    fn spread_canvas_slot_page(
        &mut self,
        doc: &dyn PdfBackend,
        page: Option<usize>,
        scale: f32,
        overlay: &HighlightOverlaySnapshot,
    ) -> AppResult<SpreadCanvasSlotPage> {
        let Some(page_index) = page else {
            return Ok(SpreadCanvasSlotPage {
                page: None,
                geometry: None,
                active: false,
            });
        };

        let page = self.cached_decorated_page(doc, Some(page_index), scale, overlay)?;
        let geometry = page
            .as_ref()
            .map(spread_canvas_page)
            .or_else(|| estimated_spread_canvas_page(doc, page_index, scale));

        Ok(SpreadCanvasSlotPage {
            page,
            geometry,
            active: true,
        })
    }
}

fn spread_canvas_page(page: &CachedDecoratedPage) -> SpreadCanvasPage {
    SpreadCanvasPage {
        width: page.frame.width,
        height: page.frame.height,
    }
}

fn estimated_spread_canvas_page(
    doc: &dyn PdfBackend,
    page: usize,
    scale: f32,
) -> Option<SpreadCanvasPage> {
    let (width_pt, height_pt) = doc.page_dimensions(page).ok()?;
    let width = scaled_page_dimension(width_pt, scale);
    let height = scaled_page_dimension(height_pt, scale);
    Some(SpreadCanvasPage { width, height })
}

fn scaled_page_dimension(points: f32, scale: f32) -> u32 {
    if !points.is_finite() || !scale.is_finite() || points <= 0.0 || scale <= 0.0 {
        return 1;
    }
    (points * scale).round().clamp(1.0, u32::MAX as f32) as u32
}

fn presenter_slot_from_prepared(
    slot: Option<&PreparedPresenterSlot>,
    generation: u64,
) -> PresenterSlot<'_> {
    PresenterSlot {
        cache_key: slot.map(|slot| slot.cache_key),
        frame: slot.map(|slot| &slot.frame),
        viewport: slot.map(|slot| slot.viewport).unwrap_or(Viewport {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }),
        pan: slot.map(|slot| slot.pan).unwrap_or_default(),
        overlay_stamp: slot.map(|slot| slot.overlay_stamp).unwrap_or(0),
        generation,
    }
}

fn decorate_frame(
    frame: &RgbaFrame,
    overlay: &HighlightOverlaySnapshot,
    pages: &[PageRenderSpace],
) -> RgbaFrame {
    if overlay.is_empty() {
        frame.clone()
    } else {
        apply_highlight_overlay(frame, overlay, pages)
    }
}

fn decorate_single_page_frame(
    doc: &dyn PdfBackend,
    page: usize,
    frame: &RgbaFrame,
    overlay: &HighlightOverlaySnapshot,
) -> (RgbaFrame, u64) {
    if overlay.is_empty() {
        return (frame.clone(), 0);
    }
    match page_render_space(doc, page, frame, 0) {
        Ok(page_space) => (decorate_frame(frame, overlay, &[page_space]), overlay.stamp),
        Err(_) => (frame.clone(), 0),
    }
}

fn page_render_space(
    doc: &dyn PdfBackend,
    page: usize,
    frame: &RgbaFrame,
    origin_x_px: u32,
) -> AppResult<PageRenderSpace> {
    let (width_pt, height_pt) = doc.page_dimensions(page)?;
    Ok(PageRenderSpace {
        page,
        origin_x_px,
        origin_y_px: 0,
        width_px: frame.width,
        height_px: frame.height,
        width_pt,
        height_pt,
    })
}
