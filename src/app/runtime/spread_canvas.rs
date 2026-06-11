use ratatui::layout::Rect;

use crate::presenter::{PanOffset, Viewport};

use super::super::scale::resolved_cell_size_px;

#[derive(Debug, Clone, Copy)]
pub(super) struct SpreadCanvasPage {
    pub(super) width: u32,
    pub(super) height: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SpreadCanvasLayoutRequest {
    pub(super) pages: [Option<SpreadCanvasPage>; 2],
    pub(super) viewport: Viewport,
    pub(super) pan: PanOffset,
    pub(super) cell_px: Option<(u16, u16)>,
    pub(super) gap_px: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SpreadCanvasClip {
    pub(super) crop_x: u32,
    pub(super) crop_y: u32,
    pub(super) crop_width: u32,
    pub(super) crop_height: u32,
    pub(super) viewport: Viewport,
    pub(super) render_area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SpreadCanvasLayout {
    pub(super) pan: PanOffset,
    pub(super) clips: [Option<SpreadCanvasClip>; 2],
}

pub(super) fn layout(request: SpreadCanvasLayoutRequest) -> SpreadCanvasLayout {
    let [left, right] = request.pages;
    let left_width = left
        .map(|page| page.width)
        .or_else(|| right.map(|page| page.width))
        .unwrap_or(1);
    let right_width = right
        .map(|page| page.width)
        .or_else(|| left.map(|page| page.width))
        .unwrap_or(1);
    let left_height = left
        .map(|page| page.height)
        .or_else(|| right.map(|page| page.height))
        .unwrap_or(1);
    let right_height = right
        .map(|page| page.height)
        .or_else(|| left.map(|page| page.height))
        .unwrap_or(1);

    let canvas_width = left_width
        .saturating_add(request.gap_px)
        .saturating_add(right_width);
    let canvas_height = left_height.max(right_height);
    let (cell_width_px, cell_height_px) = resolved_cell_size_px(request.cell_px);
    let viewport_width_px =
        u32::from(request.viewport.width.max(1)).saturating_mul(u32::from(cell_width_px));
    let viewport_height_px =
        u32::from(request.viewport.height.max(1)).saturating_mul(u32::from(cell_height_px));

    let mut pan = request.pan;
    let max_x = canvas_width.saturating_sub(viewport_width_px);
    let max_y = canvas_height.saturating_sub(viewport_height_px);
    pan.clamp_to_pixel_bounds(max_x, max_y, cell_width_px, cell_height_px);
    let (origin_x, origin_y) = pan.pixel_origin(max_x, max_y, cell_width_px, cell_height_px);

    let view = CanvasView {
        x: origin_x,
        y: origin_y,
        width: viewport_width_px,
        height: viewport_height_px,
        viewport: request.viewport,
        cell_width_px,
        cell_height_px,
    };
    let left_origin_y = left
        .map(|page| canvas_height.saturating_sub(page.height) / 2)
        .unwrap_or_default();
    let right_origin_y = right
        .map(|page| canvas_height.saturating_sub(page.height) / 2)
        .unwrap_or_default();

    SpreadCanvasLayout {
        pan,
        clips: [
            clip_page(left, view, 0, left_origin_y),
            clip_page(
                right,
                view,
                left_width.saturating_add(request.gap_px),
                right_origin_y,
            ),
        ],
    }
}

#[derive(Debug, Clone, Copy)]
struct CanvasView {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    viewport: Viewport,
    cell_width_px: u16,
    cell_height_px: u16,
}

fn clip_page(
    page: Option<SpreadCanvasPage>,
    view: CanvasView,
    page_origin_x: u32,
    page_origin_y: u32,
) -> Option<SpreadCanvasClip> {
    let page = page?;
    let page_x0 = page_origin_x;
    let page_x1 = page_origin_x.saturating_add(page.width);
    let page_y0 = page_origin_y;
    let page_y1 = page_origin_y.saturating_add(page.height);
    let view_x1 = view.x.saturating_add(view.width);
    let view_y1 = view.y.saturating_add(view.height);
    let x0 = page_x0.max(view.x);
    let y0 = page_y0.max(view.y);
    let x1 = page_x1.min(view_x1);
    let y1 = page_y1.min(view_y1);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    let crop_x = x0.saturating_sub(page_origin_x);
    let crop_y = y0.saturating_sub(page_origin_y);
    let crop_width = x1.saturating_sub(x0);
    let crop_height = y1.saturating_sub(y0);
    let render_area = Rect::new(
        view.viewport.x.saturating_add(px_to_cells_floor(
            x0.saturating_sub(view.x),
            view.cell_width_px,
        )),
        view.viewport.y.saturating_add(px_to_cells_floor(
            y0.saturating_sub(view.y),
            view.cell_height_px,
        )),
        px_to_cells_ceil(crop_width, view.cell_width_px).min(view.viewport.width),
        px_to_cells_ceil(crop_height, view.cell_height_px).min(view.viewport.height),
    );
    if render_area.width == 0 || render_area.height == 0 {
        return None;
    }

    Some(SpreadCanvasClip {
        crop_x,
        crop_y,
        crop_width,
        crop_height,
        viewport: Viewport::from(render_area),
        render_area,
    })
}

fn px_to_cells_floor(px: u32, cell_px: u16) -> u16 {
    (px / u32::from(cell_px.max(1))).min(u32::from(u16::MAX)) as u16
}

fn px_to_cells_ceil(px: u32, cell_px: u16) -> u16 {
    let cell_px = u32::from(cell_px.max(1));
    px.saturating_add(cell_px.saturating_sub(1))
        .saturating_div(cell_px)
        .min(u32::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Viewport {
        Viewport {
            x: 0,
            y: 0,
            width: 10,
            height: 5,
        }
    }

    #[test]
    fn crops_from_shared_pan_coordinate_space() {
        let layout = layout(SpreadCanvasLayoutRequest {
            pages: [
                Some(SpreadCanvasPage {
                    width: 100,
                    height: 50,
                }),
                Some(SpreadCanvasPage {
                    width: 100,
                    height: 50,
                }),
            ],
            viewport: viewport(),
            pan: PanOffset {
                cells_x: 8,
                cells_y: 0,
            },
            cell_px: Some((10, 10)),
            gap_px: 20,
        });

        assert_eq!(
            layout.clips.map(|clip| clip.map(|clip| clip.render_area)),
            [Some(Rect::new(0, 0, 2, 5)), Some(Rect::new(4, 0, 6, 5))]
        );
    }

    #[test]
    fn centers_shorter_page_vertically() {
        let layout = layout(SpreadCanvasLayoutRequest {
            pages: [
                Some(SpreadCanvasPage {
                    width: 100,
                    height: 100,
                }),
                Some(SpreadCanvasPage {
                    width: 100,
                    height: 40,
                }),
            ],
            viewport: Viewport {
                x: 0,
                y: 0,
                width: 25,
                height: 5,
            },
            pan: PanOffset {
                cells_x: 0,
                cells_y: 2,
            },
            cell_px: Some((10, 10)),
            gap_px: 20,
        });

        assert_eq!(
            layout.clips.map(|clip| clip.map(|clip| clip.render_area)),
            [Some(Rect::new(0, 0, 10, 5)), Some(Rect::new(12, 1, 10, 4))]
        );
    }

    #[test]
    fn preserves_slot_identity_when_left_page_is_offscreen() {
        let layout = layout(SpreadCanvasLayoutRequest {
            pages: [
                Some(SpreadCanvasPage {
                    width: 100,
                    height: 50,
                }),
                Some(SpreadCanvasPage {
                    width: 100,
                    height: 50,
                }),
            ],
            viewport: viewport(),
            pan: PanOffset {
                cells_x: 12,
                cells_y: 0,
            },
            cell_px: Some((10, 10)),
            gap_px: 20,
        });

        assert_eq!(
            layout.clips.map(|clip| clip.map(|clip| clip.render_area)),
            [None, Some(Rect::new(0, 0, 10, 5))]
        );
    }

    #[test]
    fn uses_partner_dimensions_for_missing_page() {
        let layout = layout(SpreadCanvasLayoutRequest {
            pages: [
                None,
                Some(SpreadCanvasPage {
                    width: 100,
                    height: 50,
                }),
            ],
            viewport: viewport(),
            pan: PanOffset {
                cells_x: 8,
                cells_y: 0,
            },
            cell_px: Some((10, 10)),
            gap_px: 20,
        });

        assert_eq!(
            layout.clips.map(|clip| clip.map(|clip| clip.render_area)),
            [None, Some(Rect::new(4, 0, 6, 5))]
        );
    }

    #[test]
    fn clamps_pan_for_oversized_canvas_without_signed_overflow() {
        let layout = layout(SpreadCanvasLayoutRequest {
            pages: [
                Some(SpreadCanvasPage {
                    width: u32::MAX,
                    height: u32::MAX,
                }),
                None,
            ],
            viewport: Viewport {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            pan: PanOffset {
                cells_x: i32::MAX,
                cells_y: i32::MAX,
            },
            cell_px: Some((1, 1)),
            gap_px: 0,
        });

        assert_eq!(
            layout.pan,
            PanOffset {
                cells_x: i32::MAX,
                cells_y: i32::MAX
            }
        );
        let clip = layout.clips[0].expect("oversized page should intersect the viewport");
        assert_eq!(clip.crop_x, i32::MAX as u32);
        assert_eq!(clip.crop_y, i32::MAX as u32);
        assert_eq!(clip.crop_width, 1);
        assert_eq!(clip.crop_height, 1);
    }
}
