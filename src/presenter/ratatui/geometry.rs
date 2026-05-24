use ratatui::layout::Rect;

use super::super::image_ops::{fit_downscale_dimensions, fit_resize_dimensions};
use super::super::traits::PresenterHorizontalAlign;

pub(super) fn centered_fit_area(
    image_width_px: u32,
    image_height_px: u32,
    font_size: (u16, u16),
    area: Rect,
) -> Rect {
    aligned_fit_area(
        image_width_px,
        image_height_px,
        font_size,
        area,
        PresenterHorizontalAlign::Center,
        false,
    )
}

pub(super) fn aligned_fit_area(
    image_width_px: u32,
    image_height_px: u32,
    font_size: (u16, u16),
    area: Rect,
    horizontal_align: PresenterHorizontalAlign,
    allow_upscale: bool,
) -> Rect {
    if area.width == 0 || area.height == 0 {
        return area;
    }

    let cell_width_px = u32::from(font_size.0.max(1));
    let cell_height_px = u32::from(font_size.1.max(1));
    let max_width_px = u32::from(area.width).saturating_mul(cell_width_px);
    let max_height_px = u32::from(area.height).saturating_mul(cell_height_px);

    let fit_dimensions = if allow_upscale {
        fit_resize_dimensions(
            image_width_px,
            image_height_px,
            max_width_px,
            max_height_px,
            true,
        )
    } else {
        fit_downscale_dimensions(image_width_px, image_height_px, max_width_px, max_height_px)
    };
    let (fit_width_px, fit_height_px) = fit_dimensions.unwrap_or((image_width_px, image_height_px));

    let width_cells = px_to_cells(fit_width_px, cell_width_px, area.width);
    let height_cells = px_to_cells(fit_height_px, cell_height_px, area.height);
    align_rect_within(area, width_cells, height_cells, horizontal_align)
}

fn px_to_cells(px: u32, cell_px: u32, max_cells: u16) -> u16 {
    let cells = px.saturating_add(cell_px.saturating_sub(1)) / cell_px.max(1);
    cells.max(1).min(u32::from(max_cells)) as u16
}

#[cfg(test)]
pub(super) fn center_rect_within(area: Rect, width: u16, height: u16) -> Rect {
    align_rect_within(area, width, height, PresenterHorizontalAlign::Center)
}

pub(super) fn align_rect_within(
    area: Rect,
    width: u16,
    height: u16,
    horizontal_align: PresenterHorizontalAlign,
) -> Rect {
    let width = width.max(1).min(area.width);
    let height = height.max(1).min(area.height);
    let spare_width = area.width.saturating_sub(width);
    let x = match horizontal_align {
        PresenterHorizontalAlign::Start => area.x,
        PresenterHorizontalAlign::Center => area.x + spare_width / 2,
        PresenterHorizontalAlign::End => area.x + spare_width,
    };
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presenter::PresenterHorizontalAlign;
    use ratatui::layout::Rect;

    #[test]
    fn center_rect_within_places_rect_in_the_middle() {
        let area = Rect::new(10, 5, 20, 10);
        let centered = center_rect_within(area, 8, 4);
        assert_eq!(centered, Rect::new(16, 8, 8, 4));
    }
    #[test]
    fn align_rect_within_can_pin_to_horizontal_edges() {
        let area = Rect::new(10, 5, 20, 10);

        assert_eq!(
            align_rect_within(area, 8, 4, PresenterHorizontalAlign::Start),
            Rect::new(10, 8, 8, 4)
        );
        assert_eq!(
            align_rect_within(area, 8, 4, PresenterHorizontalAlign::End),
            Rect::new(22, 8, 8, 4)
        );
    }
    #[test]
    fn centered_fit_area_keeps_aspect_and_centers() {
        let area = Rect::new(0, 0, 40, 20);
        let fit = centered_fit_area(2000, 1000, (10, 20), area);
        assert_eq!(fit, Rect::new(0, 5, 40, 10));
    }
}
