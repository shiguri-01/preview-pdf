use crate::presenter::Viewport;

use super::constants::{DEFAULT_CELL_SIZE_PX, MIN_RENDER_SCALE, SCALE_QUANTUM};

pub(crate) fn zoom_eq(left: f32, right: f32) -> bool {
    (left - right).abs() <= 0.0005
}

pub(crate) fn scale_eq(left: f32, right: f32) -> bool {
    (left - right).abs() <= 0.0005
}

pub(crate) fn select_input_poll_timeout(
    render_busy: bool,
    presenter_busy: bool,
    idle_timeout: std::time::Duration,
    busy_timeout: std::time::Duration,
) -> std::time::Duration {
    if render_busy || presenter_busy {
        busy_timeout
    } else {
        idle_timeout
    }
}

pub(crate) fn resolved_cell_size_px(cell_px: Option<(u16, u16)>) -> (u16, u16) {
    match cell_px {
        Some((width, height)) if width > 0 && height > 0 => (width, height),
        _ => (DEFAULT_CELL_SIZE_PX.0 as u16, DEFAULT_CELL_SIZE_PX.1 as u16),
    }
}

pub(crate) fn compute_render_scale(
    viewport: Viewport,
    cell_px: Option<(u16, u16)>,
    page_width_pt: f32,
    page_height_pt: f32,
    max_render_scale: f32,
) -> f32 {
    if !page_width_pt.is_finite()
        || !page_height_pt.is_finite()
        || page_width_pt <= 0.0
        || page_height_pt <= 0.0
    {
        return MIN_RENDER_SCALE;
    }

    let (cell_width_px, cell_height_px) = resolved_cell_size_px(cell_px);
    let (cell_width_px, cell_height_px) = (cell_width_px as f32, cell_height_px as f32);

    let viewport_width_px = viewport.width.max(1) as f32 * cell_width_px;
    let viewport_height_px = viewport.height.max(1) as f32 * cell_height_px;
    let fit_scale = (viewport_width_px / page_width_pt).min(viewport_height_px / page_height_pt);
    if !fit_scale.is_finite() || fit_scale <= 0.0 {
        return MIN_RENDER_SCALE;
    }

    let adaptive_scale = if fit_scale < 1.0 {
        (1.0 / fit_scale).sqrt()
    } else {
        fit_scale
    };

    let effective_max = max_render_scale.max(MIN_RENDER_SCALE);
    adaptive_scale.clamp(MIN_RENDER_SCALE, effective_max)
}

pub(crate) fn compute_scale(zoom: f32, render_scale: f32) -> f32 {
    quantize_scale(zoom * render_scale)
}

pub(crate) fn quantize_scale(scale: f32) -> f32 {
    if !scale.is_finite() || scale <= 0.0 {
        return MIN_RENDER_SCALE;
    }

    ((scale / SCALE_QUANTUM).round() * SCALE_QUANTUM).max(SCALE_QUANTUM)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::presenter::Viewport;

    use super::{
        compute_render_scale, compute_scale, quantize_scale, scale_eq, select_input_poll_timeout,
    };

    const DEFAULT_MAX_RENDER_SCALE: f32 = 2.5;

    #[test]
    fn render_scale_uses_viewport_and_page_dimensions() {
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 220,
            height: 70,
        };

        let render_scale = compute_render_scale(
            viewport,
            Some((10, 20)),
            612.0,
            792.0,
            DEFAULT_MAX_RENDER_SCALE,
        );
        assert!((render_scale - 1.77).abs() < 0.02);

        let scale = compute_scale(1.0, render_scale);
        assert!(scale_eq(scale, 1.75));
    }

    #[test]
    fn render_scale_falls_back_when_cell_size_missing() {
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };

        let render_scale =
            compute_render_scale(viewport, None, 300.0, 300.0, DEFAULT_MAX_RENDER_SCALE);
        assert!((render_scale - 1.60).abs() < 0.02);
        assert!(scale_eq(quantize_scale(1.83), 1.85));
    }

    #[test]
    fn input_poll_timeout_is_idle_without_pending_work() {
        assert_eq!(
            select_input_poll_timeout(
                false,
                false,
                Duration::from_millis(16),
                Duration::from_millis(8)
            ),
            Duration::from_millis(16)
        );
    }

    #[test]
    fn input_poll_timeout_is_busy_when_render_or_encode_is_pending() {
        assert_eq!(
            select_input_poll_timeout(
                true,
                false,
                Duration::from_millis(16),
                Duration::from_millis(8)
            ),
            Duration::from_millis(8)
        );
        assert_eq!(
            select_input_poll_timeout(
                false,
                true,
                Duration::from_millis(16),
                Duration::from_millis(8)
            ),
            Duration::from_millis(8)
        );
        assert_eq!(
            select_input_poll_timeout(
                true,
                true,
                Duration::from_millis(16),
                Duration::from_millis(8)
            ),
            Duration::from_millis(8)
        );
    }

    #[test]
    fn render_scale_upsamples_when_viewport_is_smaller_than_page() {
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };

        let render_scale = compute_render_scale(
            viewport,
            Some((10, 20)),
            612.0,
            792.0,
            DEFAULT_MAX_RENDER_SCALE,
        );
        assert!(render_scale > 1.20 && render_scale < 1.35);
        assert!(scale_eq(compute_scale(1.0, render_scale), 1.30));
    }

    #[test]
    fn render_scale_is_capped_by_preferred_max() {
        // Large viewport forces a high adaptive scale (e.g. Kitty would use 2.5).
        // For Sixel the cap is 1.5, so the result must not exceed 1.5.
        let viewport = Viewport {
            x: 0,
            y: 0,
            width: 800,
            height: 200,
        };
        let sixel_cap: f32 = 1.5;
        let scale = compute_render_scale(viewport, Some((10, 20)), 612.0, 792.0, sixel_cap);
        assert!(
            scale <= sixel_cap + f32::EPSILON,
            "scale {scale} exceeded cap {sixel_cap}"
        );

        // Halfblocks cap = 1.0
        let halfblocks_cap: f32 = 1.0;
        let scale = compute_render_scale(viewport, Some((10, 20)), 612.0, 792.0, halfblocks_cap);
        assert!(
            scale <= halfblocks_cap + f32::EPSILON,
            "scale {scale} exceeded cap {halfblocks_cap}"
        );
    }
}
