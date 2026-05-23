use ratatui::layout::Rect;
use ratatui::widgets::Clear;

use crate::app::state::VisiblePageSlots;
use crate::presenter::{
    PresenterHorizontalAlign, PresenterRenderMode, PresenterRenderOptions, PresenterRenderOutcome,
    PresenterRenderSlot, Viewport,
};
use crate::render::cache::RenderedPageKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SpreadSlotAreas {
    pub(super) left: Rect,
    pub(super) gap: Rect,
    pub(super) right: Rect,
}

impl SpreadSlotAreas {
    pub(super) fn page_slots(
        self,
        doc_id: u64,
        visible_pages: VisiblePageSlots,
        scale: f32,
    ) -> [(Option<RenderedPageKey>, Viewport); 2] {
        [
            (
                visible_pages
                    .left_page
                    .map(|page| RenderedPageKey::new(doc_id, page, scale)),
                self.left.into(),
            ),
            (
                visible_pages
                    .right_page
                    .map(|page| RenderedPageKey::new(doc_id, page, scale)),
                self.right.into(),
            ),
        ]
    }

    pub(super) fn render_slots_for_pages(
        self,
        visible_pages: VisiblePageSlots,
        options: PresenterRenderOptions,
    ) -> Vec<PresenterRenderSlot> {
        vec![
            PresenterRenderSlot {
                area: self.left,
                options,
                active: visible_pages.left_page.is_some(),
                horizontal_align: PresenterHorizontalAlign::End,
            },
            PresenterRenderSlot {
                area: self.right,
                options,
                active: visible_pages.right_page.is_some(),
                horizontal_align: PresenterHorizontalAlign::Start,
            },
        ]
    }

    pub(super) fn clear_gap(self, frame: &mut ratatui::Frame<'_>) {
        if self.gap.width > 0 && self.gap.height > 0 {
            frame.render_widget(Clear, self.gap);
        }
    }
}

pub(super) fn clear_pending_spread_regions(
    frame: &mut ratatui::Frame<'_>,
    slot_areas: SpreadSlotAreas,
    outcome: &PresenterRenderOutcome,
) {
    slot_areas.clear_gap(frame);
    for slot in &outcome.slots {
        if !slot.active && slot.area.width > 0 && slot.area.height > 0 {
            frame.render_widget(Clear, slot.area);
        }
    }
}

pub(super) fn split_spread_slot_areas(area: Rect, gap_cells: u16) -> SpreadSlotAreas {
    let gap = gap_cells.min(area.width);
    let content_width = area.width.saturating_sub(gap);
    let left_width = content_width / 2;
    let right_width = content_width.saturating_sub(left_width);
    let right_x = area.x.saturating_add(left_width).saturating_add(gap);
    let gap_x = area.x.saturating_add(left_width);
    SpreadSlotAreas {
        left: Rect::new(area.x, area.y, left_width, area.height),
        gap: Rect::new(gap_x, area.y, gap, area.height),
        right: Rect::new(right_x, area.y, right_width, area.height),
    }
}

pub(super) fn render_areas_to_slots(
    render_areas: [Option<Rect>; 2],
    render_mode: PresenterRenderMode,
) -> Vec<PresenterRenderSlot> {
    let options = PresenterRenderOptions::new(false, render_mode);
    render_areas
        .into_iter()
        .enumerate()
        .map(|(index, area)| PresenterRenderSlot {
            area: area.unwrap_or_default(),
            options,
            active: area.is_some(),
            horizontal_align: if index == 0 {
                PresenterHorizontalAlign::End
            } else {
                PresenterHorizontalAlign::Start
            },
        })
        .collect()
}

pub(super) fn format_page_target(page: usize) -> String {
    format!("p.{}", page + 1)
}

pub(super) fn format_loading_target(slots: VisiblePageSlots) -> String {
    match slots.trailing_page {
        Some(trailing) => format!("pp.{}-{}", slots.anchor_page + 1, trailing + 1),
        None => format_page_target(slots.anchor_page),
    }
}

pub(super) fn format_render_target(slots: VisiblePageSlots) -> String {
    match slots.trailing_page {
        Some(trailing) => format!("pp.{}-{}", slots.anchor_page + 1, trailing + 1),
        None => format!("p.{}", slots.anchor_page + 1),
    }
}
