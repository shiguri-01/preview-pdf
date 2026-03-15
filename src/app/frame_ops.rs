use std::sync::OnceLock;

use crate::backend::{PixelBuffer, PixelBufferPool, RgbaFrame};
use crate::presenter::{PanOffset, Viewport};
use crate::render::prefetch::PrefetchClass;
use crate::render::scheduler::RenderPriority;

use super::scale::resolved_cell_size_px;

static FRAME_OPS_PIXEL_POOL: OnceLock<PixelBufferPool> = OnceLock::new();

fn frame_ops_pixel_pool() -> &'static PixelBufferPool {
    FRAME_OPS_PIXEL_POOL.get_or_init(PixelBufferPool::default)
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

pub(crate) fn prefetch_class_for_completed_task(priority: RenderPriority) -> PrefetchClass {
    match priority {
        RenderPriority::CriticalCurrent => PrefetchClass::DirectionalLead,
        _ => priority.to_prefetch_class(),
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
