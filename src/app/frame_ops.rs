use std::sync::OnceLock;

use crate::backend::{PdfRect, PixelBuffer, PixelBufferPool, RgbaFrame};
use crate::highlight::{HighlightOverlaySnapshot, HighlightSpan};
use crate::presenter::{PanOffset, Viewport};
use crate::work::WorkClass;

use super::scale::resolved_cell_size_px;

static FRAME_OPS_PIXEL_POOL: OnceLock<PixelBufferPool> = OnceLock::new();

fn frame_ops_pixel_pool() -> &'static PixelBufferPool {
    FRAME_OPS_PIXEL_POOL.get_or_init(PixelBufferPool::default)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PageRenderSpace {
    pub(crate) page: usize,
    pub(crate) origin_x_px: u32,
    pub(crate) origin_y_px: u32,
    pub(crate) width_px: u32,
    pub(crate) height_px: u32,
    pub(crate) width_pt: f32,
    pub(crate) height_pt: f32,
}

pub(crate) fn apply_highlight_overlay(
    frame: &RgbaFrame,
    overlay: &HighlightOverlaySnapshot,
    pages: &[PageRenderSpace],
) -> RgbaFrame {
    if overlay.is_empty() || pages.is_empty() {
        return frame.clone();
    }

    if !overlay
        .spans
        .iter()
        .any(|span| pages.iter().any(|page| page.page == span.page))
    {
        return frame.clone();
    }

    let mut pixels = frame_ops_pixel_pool().take(frame.byte_len());
    pixels.copy_from_slice(&frame.pixels);
    for span in &overlay.spans {
        for page in pages {
            if page.page != span.page {
                continue;
            }
            draw_span(&mut pixels, frame.width, frame.height, span, *page);
        }
    }

    RgbaFrame {
        width: frame.width,
        height: frame.height,
        pixels: PixelBuffer::from_pooled_vec(pixels, frame_ops_pixel_pool()),
    }
}

pub(crate) fn prepare_presenter_frame(
    frame: &RgbaFrame,
    viewport: Viewport,
    pan: &mut PanOffset,
    cell_px: Option<(u16, u16)>,
    enable_crop: bool,
) -> (RgbaFrame, PanOffset) {
    if !enable_crop {
        *pan = PanOffset::default();
        return (frame.clone(), PanOffset::default());
    }

    let frame = crop_frame_for_viewport(frame, viewport, pan, cell_px);
    (frame, *pan)
}

pub(crate) fn crop_frame_for_viewport(
    frame: &RgbaFrame,
    viewport: Viewport,
    pan: &mut PanOffset,
    cell_px: Option<(u16, u16)>,
) -> RgbaFrame {
    let (cell_width_px, cell_height_px) = resolved_cell_size_px(cell_px);
    let target_width = (viewport.width.max(1) as u32).saturating_mul(cell_width_px as u32);
    let target_height = (viewport.height.max(1) as u32).saturating_mul(cell_height_px as u32);

    let src_width = frame.width;
    let src_height = frame.height;
    let max_x = src_width.saturating_sub(target_width);
    let max_y = src_height.saturating_sub(target_height);
    let max_cells_x = (max_x / cell_width_px as u32) as i32;
    let max_cells_y = (max_y / cell_height_px as u32) as i32;
    pan.cells_x = pan.cells_x.clamp(0, max_cells_x);
    pan.cells_y = pan.cells_y.clamp(0, max_cells_y);

    let pan_px_x = pan.cells_x.saturating_mul(cell_width_px as i32);
    let pan_px_y = pan.cells_y.saturating_mul(cell_height_px as i32);
    let origin_x = pan_px_x.clamp(0, max_x as i32) as u32;
    let origin_y = pan_px_y.clamp(0, max_y as i32) as u32;

    let copy_width = target_width.min(src_width.saturating_sub(origin_x));
    let copy_height = target_height.min(src_height.saturating_sub(origin_y));
    let out_width = copy_width.max(1);
    let out_height = copy_height.max(1);

    if origin_x == 0 && origin_y == 0 && out_width == src_width && out_height == src_height {
        return frame.clone();
    }

    let mut pixels = frame_ops_pixel_pool().take(out_width as usize * out_height as usize * 4);

    if copy_width > 0 && copy_height > 0 {
        let src_stride = src_width as usize * 4;
        let dst_stride = out_width as usize * 4;
        let copy_row_bytes = copy_width as usize * 4;
        for row in 0..copy_height as usize {
            let src_row = origin_y as usize + row;
            let dst_row = row;
            let src_start = src_row * src_stride + origin_x as usize * 4;
            let dst_start = dst_row * dst_stride;
            let src_end = src_start + copy_row_bytes;
            let dst_end = dst_start + copy_row_bytes;
            pixels[dst_start..dst_end].copy_from_slice(&frame.pixels[src_start..src_end]);
        }
    }

    RgbaFrame {
        width: out_width,
        height: out_height,
        pixels: PixelBuffer::from_pooled_vec(pixels, frame_ops_pixel_pool()),
    }
}

pub(crate) fn encode_work_class_for_completed_render(class: WorkClass) -> WorkClass {
    match class {
        WorkClass::CriticalCurrent => WorkClass::DirectionalLead,
        _ => class,
    }
}

pub(crate) fn compose_spread_frame(
    left: Option<&RgbaFrame>,
    right: Option<&RgbaFrame>,
    gap_px: u32,
) -> RgbaFrame {
    let left_width = left
        .map(|frame| frame.width)
        .or_else(|| right.map(|frame| frame.width))
        .unwrap_or(1);
    let right_width = right
        .map(|frame| frame.width)
        .or_else(|| left.map(|frame| frame.width))
        .unwrap_or(1);
    let left_height = left
        .map(|frame| frame.height)
        .or_else(|| right.map(|frame| frame.height))
        .unwrap_or(1);
    let right_height = right
        .map(|frame| frame.height)
        .or_else(|| left.map(|frame| frame.height))
        .unwrap_or(1);

    let out_width = left_width
        .saturating_add(gap_px)
        .saturating_add(right_width)
        .max(1);
    let out_height = left_height.max(right_height).max(1);
    let mut pixels = frame_ops_pixel_pool().take(out_width as usize * out_height as usize * 4);

    blit_side(&mut pixels, out_width, out_height, left, 0);
    blit_side(
        &mut pixels,
        out_width,
        out_height,
        right,
        left_width.saturating_add(gap_px),
    );

    RgbaFrame {
        width: out_width,
        height: out_height,
        pixels: PixelBuffer::from_pooled_vec(pixels, frame_ops_pixel_pool()),
    }
}

fn blit_side(
    out_pixels: &mut [u8],
    out_width: u32,
    out_height: u32,
    src: Option<&RgbaFrame>,
    offset_x: u32,
) {
    let Some(src) = src else {
        return;
    };

    let copy_width = src.width.min(out_width.saturating_sub(offset_x));
    let copy_height = src.height.min(out_height);
    if copy_width == 0 || copy_height == 0 {
        return;
    }

    let out_stride = out_width as usize * 4;
    let src_stride = src.width as usize * 4;
    let row_bytes = copy_width as usize * 4;
    for row in 0..copy_height as usize {
        let out_start = row * out_stride + offset_x as usize * 4;
        let out_end = out_start + row_bytes;
        let src_start = row * src_stride;
        let src_end = src_start + row_bytes;
        out_pixels[out_start..out_end].copy_from_slice(&src.pixels[src_start..src_end]);
    }
}

fn draw_span(
    pixels: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    span: &HighlightSpan,
    page: PageRenderSpace,
) {
    for rect in &span.rects {
        let Some((x0, y0, x1, y1)) = rect_to_pixels(*rect, page, frame_width, frame_height) else {
            continue;
        };
        fill_rect(
            pixels,
            frame_width as usize,
            x0,
            y0,
            x1,
            y1,
            span.style.fill_rgba,
        );
    }
}

fn rect_to_pixels(
    rect: PdfRect,
    page: PageRenderSpace,
    frame_width: u32,
    frame_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    if page.width_px == 0 || page.height_px == 0 || page.width_pt <= 0.0 || page.height_pt <= 0.0 {
        return None;
    }

    let scale_x = page.width_px as f32 / page.width_pt;
    let scale_y = page.height_px as f32 / page.height_pt;
    let x0 = page.origin_x_px + (rect.x0.max(0.0) * scale_x).floor().max(0.0) as u32;
    let y0 = page.origin_y_px + (rect.y0.max(0.0) * scale_y).floor().max(0.0) as u32;
    let x1 = page.origin_x_px + (rect.x1.max(0.0) * scale_x).ceil().max(0.0) as u32;
    let y1 = page.origin_y_px + (rect.y1.max(0.0) * scale_y).ceil().max(0.0) as u32;
    let x0 = x0.min(frame_width);
    let y0 = y0.min(frame_height);
    let x1 = x1.min(frame_width);
    let y1 = y1.min(frame_height);
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

fn fill_rect(
    pixels: &mut [u8],
    stride_width: usize,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    color: [u8; 4],
) {
    let alpha = color[3] as u16;
    for y in y0 as usize..y1 as usize {
        for x in x0 as usize..x1 as usize {
            let idx = (y * stride_width + x) * 4;
            for (channel, value) in color.iter().take(3).enumerate() {
                let base = pixels[idx + channel] as u16;
                let fill = *value as u16;
                pixels[idx + channel] = (((base * (255 - alpha)) + (fill * alpha)) / 255) as u8;
            }
            pixels[idx + 3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{compose_spread_frame, crop_frame_for_viewport, prepare_presenter_frame};
    use crate::backend::RgbaFrame;
    use crate::presenter::{PanOffset, Viewport};

    #[test]
    fn crop_frame_for_viewport_applies_pan_offset() {
        let mut pixels = Vec::new();
        for y in 0..4u8 {
            for x in 0..4u8 {
                pixels.extend_from_slice(&[x + y * 10, 0, 0, 255]);
            }
        }
        let frame = RgbaFrame {
            width: 4,
            height: 4,
            pixels: pixels.into(),
        };
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        };
        let mut pan = PanOffset {
            cells_x: 1,
            cells_y: 1,
        };

        let cropped = crop_frame_for_viewport(&frame, viewport, &mut pan, Some((1, 1)));

        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(cropped.pixels[0], 11);
    }

    #[test]
    fn crop_frame_for_viewport_clamps_when_target_exceeds_source() {
        let frame = RgbaFrame {
            width: 2,
            height: 2,
            pixels: vec![10, 0, 0, 255, 20, 0, 0, 255, 30, 0, 0, 255, 40, 0, 0, 255].into(),
        };
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 3,
            height: 2,
        };
        let mut pan = PanOffset::default();

        let cropped = crop_frame_for_viewport(&frame, viewport, &mut pan, Some((1, 1)));
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(cropped.pixels[0], 10);
        assert_eq!(cropped.pixels[12], 40);
        assert!(frame.pixels.ptr_eq(&cropped.pixels));
    }

    #[test]
    fn crop_frame_for_viewport_normalizes_negative_and_overflow_pan() {
        let frame = RgbaFrame {
            width: 8,
            height: 6,
            pixels: vec![180; 8 * 6 * 4].into(),
        };
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        };
        let mut pan = PanOffset {
            cells_x: -5,
            cells_y: 99,
        };

        let _ = crop_frame_for_viewport(&frame, viewport, &mut pan, Some((2, 2)));
        assert_eq!(pan.cells_x, 0);
        assert_eq!(pan.cells_y, 1);
    }

    #[test]
    fn prepare_presenter_frame_without_crop_reuses_pixel_buffer() {
        let frame = RgbaFrame {
            width: 2,
            height: 2,
            pixels: vec![7; 16].into(),
        };
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let mut pan = PanOffset {
            cells_x: 4,
            cells_y: 6,
        };

        let (prepared, pan_for_presenter) =
            prepare_presenter_frame(&frame, viewport, &mut pan, None, false);

        assert!(frame.pixels.ptr_eq(&prepared.pixels));
        assert_eq!(pan, PanOffset::default());
        assert_eq!(pan_for_presenter, PanOffset::default());
    }

    #[test]
    fn compose_spread_frame_places_left_and_right_with_gap() {
        let left = RgbaFrame {
            width: 2,
            height: 1,
            pixels: vec![1, 0, 0, 255, 2, 0, 0, 255].into(),
        };
        let right = RgbaFrame {
            width: 2,
            height: 1,
            pixels: vec![3, 0, 0, 255, 4, 0, 0, 255].into(),
        };

        let composed = compose_spread_frame(Some(&left), Some(&right), 1);
        assert_eq!(composed.width, 5);
        assert_eq!(composed.height, 1);
        assert_eq!(composed.pixels[0], 1);
        assert_eq!(composed.pixels[4], 2);
        assert_eq!(composed.pixels[8], 0);
        assert_eq!(composed.pixels[12], 3);
    }

    #[test]
    fn compose_spread_frame_keeps_blank_slot_for_missing_side() {
        let page = RgbaFrame {
            width: 2,
            height: 1,
            pixels: vec![9, 0, 0, 255, 8, 0, 0, 255].into(),
        };

        let composed = compose_spread_frame(None, Some(&page), 1);
        assert_eq!(composed.width, 5);
        assert_eq!(composed.height, 1);
        assert_eq!(composed.pixels[0], 0);
        assert_eq!(composed.pixels[12], 9);
    }
}
