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
    let image = RgbaImage::from_raw(frame.width, frame.height, frame.pixels_to_vec()).ok_or(
        AppError::invalid_argument("rgba frame pixels length does not match dimensions"),
    )?;
    let image = DynamicImage::ImageRgba8(image);
    Ok(picker.new_resize_protocol(image))
}

pub(crate) fn downscale_frame_for_area(
    frame: RgbaFrame,
    area: Rect,
    cell_px: (u16, u16),
) -> AppResult<RgbaFrame> {
    let max_width = u32::from(area.width.max(1)).saturating_mul(u32::from(cell_px.0.max(1)));
    let max_height = u32::from(area.height.max(1)).saturating_mul(u32::from(cell_px.1.max(1)));

    let Some((dst_width, dst_height)) =
        fit_downscale_dimensions(frame.width, frame.height, max_width, max_height)
    else {
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
    if src_width == 0 || src_height == 0 || max_width == 0 || max_height == 0 {
        return None;
    }
    if src_width <= max_width && src_height <= max_height {
        return None;
    }

    let width_limited = (max_width as u64).saturating_mul(src_height as u64)
        <= (max_height as u64).saturating_mul(src_width as u64);

    if width_limited {
        let dst_width = max_width.max(1);
        let dst_height =
            ((src_height as u64).saturating_mul(dst_width as u64) / src_width as u64).max(1) as u32;
        Some((dst_width, dst_height.min(max_height.max(1))))
    } else {
        let dst_height = max_height.max(1);
        let dst_width = ((src_width as u64).saturating_mul(dst_height as u64) / src_height as u64)
            .max(1) as u32;
        Some((dst_width.min(max_width.max(1)), dst_height))
    }
}

fn resize_frame_simd(frame: RgbaFrame, dst_width: u32, dst_height: u32) -> AppResult<RgbaFrame> {
    if frame.width == dst_width && frame.height == dst_height {
        return Ok(frame);
    }

    let src = fr::images::Image::from_vec_u8(
        frame.width,
        frame.height,
        frame.pixels_to_vec(),
        fr::PixelType::U8x4,
    )
    .map_err(|_| {
        AppError::invalid_argument("rgba frame pixels length does not match dimensions")
    })?;

    let mut dst = fr::images::Image::new(dst_width, dst_height, fr::PixelType::U8x4);
    let mut resizer = fr::Resizer::new();
    let options =
        fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Convolution(SIMD_DOWNSCALE_FILTER));

    resizer
        .resize(&src, &mut dst, &options)
        .map_err(|_| AppError::unsupported("failed to downscale frame with SIMD"))?;

    Ok(RgbaFrame {
        width: dst_width,
        height: dst_height,
        pixels: dst.into_vec().into(),
    })
}
