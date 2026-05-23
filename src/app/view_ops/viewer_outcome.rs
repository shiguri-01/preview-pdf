use ratatui::widgets::Clear;

use crate::app::state::{AppState, VisiblePageSlots};
use crate::presenter::{
    PresenterFeedback, PresenterRenderMode, PresenterRenderOptions, PresenterRenderOutcome,
    PresenterSlotOutcome,
};
use crate::ui;

use super::spread::{SpreadSlotAreas, format_page_target};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ViewerDisplayDecision {
    pub(super) clear: bool,
    pub(super) show_loading: bool,
    pub(super) show_error: bool,
}

pub(super) fn decide_viewer_display(
    outcome: &PresenterRenderOutcome,
    viewer_has_image: bool,
) -> ViewerDisplayDecision {
    let clear = !outcome.drew_image && !viewer_has_image;
    let mut show_loading = false;
    let mut show_error = false;
    match outcome.feedback {
        PresenterFeedback::None => {
            if clear {
                show_loading = true;
            }
        }
        PresenterFeedback::Pending => show_loading = true,
        PresenterFeedback::Failed => show_error = true,
    }
    ViewerDisplayDecision {
        clear,
        show_loading,
        show_error,
    }
}

pub(super) fn normalize_render_outcome(
    render_mode: PresenterRenderMode,
    mut outcome: PresenterRenderOutcome,
) -> PresenterRenderOutcome {
    match render_mode {
        PresenterRenderMode::InitialPreview => {
            outcome.feedback = PresenterFeedback::Pending;
            for slot in &mut outcome.slots {
                if slot.active && slot.feedback == PresenterFeedback::None {
                    slot.feedback = PresenterFeedback::Pending;
                }
            }
            outcome
        }
        PresenterRenderMode::Full => outcome,
    }
}

pub(super) fn pending_spread_outcome(
    slot_areas: SpreadSlotAreas,
    visible_pages: VisiblePageSlots,
    feedback: PresenterFeedback,
) -> PresenterRenderOutcome {
    PresenterRenderOutcome::aggregate_slots(vec![
        match visible_pages.left_page {
            Some(_) => PresenterSlotOutcome::active(slot_areas.left, false, feedback, false),
            None => PresenterSlotOutcome::inactive(slot_areas.left),
        },
        match visible_pages.right_page {
            Some(_) => PresenterSlotOutcome::active(slot_areas.right, false, feedback, false),
            None => PresenterSlotOutcome::inactive(slot_areas.right),
        },
    ])
}

pub(super) fn spread_loading_overlays(
    outcome: &PresenterRenderOutcome,
    visible_pages: VisiblePageSlots,
) -> Vec<(ratatui::layout::Rect, String)> {
    let pages = [visible_pages.left_page, visible_pages.right_page];
    outcome
        .slots
        .iter()
        .zip(pages)
        .filter_map(|(slot, page)| {
            (slot.active && slot.feedback == PresenterFeedback::Pending)
                .then_some((slot.area, format_page_target(page?)))
        })
        .collect()
}

pub(super) fn draw_spread_loading_overlays(
    frame: &mut ratatui::Frame<'_>,
    outcome: &PresenterRenderOutcome,
    visible_pages: VisiblePageSlots,
) {
    for (area, label) in spread_loading_overlays(outcome, visible_pages) {
        ui::draw_loading_overlay(frame, area, &label);
    }
}

pub(super) fn draw_viewer_outcome(
    frame: &mut ratatui::Frame<'_>,
    image_area: ratatui::layout::Rect,
    outcome: &PresenterRenderOutcome,
    loading_label: &str,
    render_target: Option<&str>,
    viewer_has_image: bool,
    allow_loading_overlay: bool,
) {
    let decision = decide_viewer_display(outcome, viewer_has_image);
    if decision.clear {
        frame.render_widget(Clear, image_area);
    }
    if allow_loading_overlay && decision.show_loading {
        ui::draw_loading_overlay(frame, image_area, loading_label);
    }
    if decision.show_error {
        let message = render_failure_message(render_target);
        ui::draw_error_overlay(frame, image_area, &message);
    }
}

pub(super) fn render_failure_message(render_target: Option<&str>) -> String {
    match render_target {
        Some(target) => format!("Could not render {target}."),
        None => "Could not render the current page.".to_string(),
    }
}

pub(super) fn sync_render_notice(
    state: &mut AppState,
    render_failed: bool,
    render_feedback: PresenterFeedback,
    render_target: &str,
) {
    if render_failed || render_feedback == PresenterFeedback::Failed {
        state.set_error_notice(render_failure_message(Some(render_target)));
        return;
    }
    state.clear_render_notice();
}

pub(super) fn presenter_render_options(
    viewer_has_image: bool,
    render_mode: PresenterRenderMode,
    image_occluded: bool,
    force_image_redraw: bool,
) -> PresenterRenderOptions {
    let mut options = PresenterRenderOptions::new(viewer_has_image, render_mode);
    options.preserve_stable_image = true;
    options.force_image_redraw = force_image_redraw || (image_occluded && !viewer_has_image);
    options
}
