use fast_image_resize as fr;
use image::{DynamicImage, RgbaImage};
use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

use crate::backend::RgbaFrame;
use crate::error::{AppError, AppResult};

pub(crate) const SIMD_DOWNSCALE_FILTER: fr::FilterType = fr::FilterType::CatmullRom;

pub(crate) fn create_protocol_with_picker(
    picker: &Picker,
    frame: RgbaFrame,
) -> AppResult<StatefulProtocol> {
    let image = RgbaImage::from_raw(frame.width, frame.height, frame.into_pixels_vec()).ok_or(
        AppError::invalid_argument("rgba frame pixels length does not match dimensions"),
    )?;
    let image = DynamicImage::ImageRgba8(image);
    Ok(picker.new_resize_protocol(image))
}

pub(crate) fn resize_frame_for_area(
    frame: RgbaFrame,
    area: Rect,
    cell_px: (u16, u16),
    allow_upscale: bool,
) -> AppResult<RgbaFrame> {
    let max_width = u32::from(area.width.max(1)).saturating_mul(u32::from(cell_px.0.max(1)));
    let max_height = u32::from(area.height.max(1)).saturating_mul(u32::from(cell_px.1.max(1)));

    let Some((dst_width, dst_height)) = fit_resize_dimensions(
        frame.width,
        frame.height,
        max_width,
        max_height,
        allow_upscale,
    ) else {
        return Ok(frame);
    };

    resize_frame_simd(frame, dst_width, dst_height)
}

pub(crate) fn fit_downscale_dimensions(
    src_width: u32,
    src_height: u32,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32)> {
    fit_resize_dimensions(src_width, src_height, max_width, max_height, false)
}

pub(crate) fn fit_resize_dimensions(
    src_width: u32,
    src_height: u32,
    max_width: u32,
    max_height: u32,
    allow_upscale: bool,
) -> Option<(u32, u32)> {
    if src_width == 0 || src_height == 0 || max_width == 0 || max_height == 0 {
        return None;
    }
    if !allow_upscale && src_width <= max_width && src_height <= max_height {
        return None;
    }

    let width_limited = (max_width as u64).saturating_mul(src_height as u64)
        <= (max_height as u64).saturating_mul(src_width as u64);

    if width_limited {
        let dst_width = max_width.max(1);
        let dst_height =
            ((src_height as u64).saturating_mul(dst_width as u64) / src_width as u64).max(1) as u32;
        let dst_height = dst_height.min(max_height.max(1));
        if dst_width == src_width && dst_height == src_height {
            return None;
        }
        Some((dst_width, dst_height))
    } else {
        let dst_height = max_height.max(1);
        let dst_width = ((src_width as u64).saturating_mul(dst_height as u64) / src_height as u64)
            .max(1) as u32;
        let dst_width = dst_width.min(max_width.max(1));
        if dst_width == src_width && dst_height == src_height {
            return None;
        }
        Some((dst_width, dst_height))
    }
}

fn resize_frame_simd(frame: RgbaFrame, dst_width: u32, dst_height: u32) -> AppResult<RgbaFrame> {
    if frame.width == dst_width && frame.height == dst_height {
        return Ok(frame);
    }

    let RgbaFrame {
        width: src_width,
        height: src_height,
        pixels,
    } = frame;
    let pixels = resize_rgba_bytes_simd(&pixels, src_width, src_height, dst_width, dst_height)?;

    Ok(RgbaFrame {
        width: dst_width,
        height: dst_height,
        pixels: pixels.into(),
    })
}

fn resize_rgba_bytes_simd(
    src_pixels: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> AppResult<Vec<u8>> {
    let src = fr::images::ImageRef::new(src_width, src_height, src_pixels, fr::PixelType::U8x4)
        .map_err(|_| {
            AppError::invalid_argument("rgba frame pixels length does not match dimensions")
        })?;

    let mut dst = fr::images::Image::new(dst_width, dst_height, fr::PixelType::U8x4);
    let mut resizer = fr::Resizer::new();
    let options =
        fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Convolution(SIMD_DOWNSCALE_FILTER));

    resizer
        .resize(&src, &mut dst, &options)
        .map_err(|_| AppError::unsupported("failed to resize frame with SIMD"))?;

    Ok(dst.into_vec())
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use crate::backend::RgbaFrame;

    use super::{fit_downscale_dimensions, fit_resize_dimensions, resize_frame_for_area};

    #[test]
    fn fit_downscale_dimensions_returns_none_when_source_fits() {
        let dims = fit_downscale_dimensions(640, 480, 1280, 720);
        assert_eq!(dims, None);
    }

    #[test]
    fn fit_downscale_dimensions_preserves_aspect_ratio() {
        let dims = fit_downscale_dimensions(2400, 3200, 960, 640);
        assert_eq!(dims, Some((480, 640)));
    }

    #[test]
    fn fit_resize_dimensions_upscales_when_allowed() {
        let dims = fit_resize_dimensions(400, 200, 1200, 900, true);
        assert_eq!(dims, Some((1200, 600)));
    }

    #[test]
    fn fit_resize_dimensions_skips_upscale_when_disallowed() {
        let dims = fit_resize_dimensions(400, 200, 1200, 900, false);
        assert_eq!(dims, None);
    }

    #[test]
    fn resize_frame_for_area_downscales_shared_frame_without_touching_source() {
        let pixels: Vec<u8> = (0..4 * 4 * 4).map(|i| i as u8).collect();
        let source = RgbaFrame {
            width: 4,
            height: 4,
            pixels: pixels.clone().into(),
        };
        let shared = source.clone();

        let resized = resize_frame_for_area(source.clone(), Rect::new(0, 0, 2, 2), (1, 1), false)
            .expect("resize should succeed");

        assert_eq!(resized.width, 2);
        assert_eq!(resized.height, 2);
        assert_eq!(source.width, 4);
        assert_eq!(source.height, 4);
        assert_eq!(&source.pixels[..], pixels.as_slice());
        assert!(source.pixels.ptr_eq(&shared.pixels));
        assert!(!resized.pixels.ptr_eq(&source.pixels));
    }
}
