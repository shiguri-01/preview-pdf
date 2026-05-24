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

#[cfg(test)]
mod tests {
    use super::super::spread::SpreadSlotAreas;
    use super::*;
    use crate::app::{AppState, VisiblePageSlots};
    use crate::presenter::{
        PresenterFeedback, PresenterRenderMode, PresenterRenderOutcome, PresenterSlotOutcome,
    };
    use ratatui::layout::Rect;

    fn render_outcome(
        drew_image: bool,
        feedback: PresenterFeedback,
        used_stale_fallback: bool,
    ) -> PresenterRenderOutcome {
        PresenterRenderOutcome {
            drew_image,
            feedback,
            used_stale_fallback,
            slots: Vec::new(),
        }
    }

    #[test]
    fn display_decision_clears_when_no_image_drawn() {
        let outcome = render_outcome(false, PresenterFeedback::None, false);
        let decision = decide_viewer_display(&outcome, false);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: true,
                show_loading: true,
                show_error: false,
            }
        );
    }
    #[test]
    fn normalize_render_outcome_keeps_loading_feedback_for_initial_preview() {
        let outcome = normalize_render_outcome(
            PresenterRenderMode::InitialPreview,
            PresenterRenderOutcome {
                drew_image: true,
                feedback: PresenterFeedback::None,
                used_stale_fallback: true,
                slots: vec![PresenterSlotOutcome::active(
                    Rect::new(2, 3, 10, 5),
                    true,
                    PresenterFeedback::None,
                    true,
                )],
            },
        );

        assert!(outcome.drew_image);
        assert_eq!(outcome.feedback, PresenterFeedback::Pending);
        assert!(outcome.used_stale_fallback);
        assert_eq!(outcome.slots[0].feedback, PresenterFeedback::Pending);
    }
    #[test]
    fn normalize_render_outcome_keeps_full_feedback_unchanged() {
        let outcome = normalize_render_outcome(
            PresenterRenderMode::Full,
            PresenterRenderOutcome {
                drew_image: true,
                feedback: PresenterFeedback::Failed,
                used_stale_fallback: true,
                slots: Vec::new(),
            },
        );

        assert!(outcome.drew_image);
        assert_eq!(outcome.feedback, PresenterFeedback::Failed);
        assert!(outcome.used_stale_fallback);
    }
    #[test]
    fn display_decision_overlays_loading_on_pending_stale_fallback() {
        let outcome = render_outcome(true, PresenterFeedback::Pending, true);
        let decision = decide_viewer_display(&outcome, true);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: false,
                show_loading: true,
                show_error: false,
            }
        );
    }
    #[test]
    fn display_decision_overlays_loading_on_pending_fresh_image() {
        let outcome = render_outcome(true, PresenterFeedback::Pending, false);
        let decision = decide_viewer_display(&outcome, true);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: false,
                show_loading: true,
                show_error: false,
            }
        );
    }
    #[test]
    fn display_decision_overlays_error_on_failed_image() {
        let outcome = render_outcome(true, PresenterFeedback::Failed, true);
        let decision = decide_viewer_display(&outcome, true);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: false,
                show_loading: false,
                show_error: true,
            }
        );
    }
    #[test]
    fn display_decision_clears_and_loading_for_pending_without_image() {
        let outcome = render_outcome(false, PresenterFeedback::Pending, false);
        let decision = decide_viewer_display(&outcome, false);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: true,
                show_loading: true,
                show_error: false,
            }
        );
    }
    #[test]
    fn display_decision_clears_and_error_for_failed_without_image() {
        let outcome = render_outcome(false, PresenterFeedback::Failed, false);
        let decision = decide_viewer_display(&outcome, false);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: true,
                show_loading: false,
                show_error: true,
            }
        );
    }
    #[test]
    fn display_decision_overlays_loading_when_pending_without_drawn_image() {
        let outcome = render_outcome(false, PresenterFeedback::Pending, false);
        let decision = decide_viewer_display(&outcome, true);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: false,
                show_loading: true,
                show_error: false,
            }
        );
    }
    #[test]
    fn spread_loading_overlays_selects_pending_slots_with_page_labels() {
        let left_area = Rect::new(0, 1, 10, 8);
        let right_area = Rect::new(12, 1, 10, 8);
        let visible_pages = VisiblePageSlots {
            anchor_page: 10,
            trailing_page: Some(11),
            left_page: Some(10),
            right_page: Some(11),
        };
        let outcome = PresenterRenderOutcome::aggregate_slots(vec![
            PresenterSlotOutcome::active(left_area, false, PresenterFeedback::Pending, false),
            PresenterSlotOutcome::active(right_area, true, PresenterFeedback::Pending, true),
        ]);

        assert_eq!(
            spread_loading_overlays(&outcome, visible_pages),
            vec![
                (left_area, "p.11".to_string()),
                (right_area, "p.12".to_string())
            ]
        );
    }
    #[test]
    fn pending_spread_outcome_uses_slot_loading_areas_from_first_pending_frame() {
        let slot_areas = SpreadSlotAreas {
            left: Rect::new(0, 1, 10, 8),
            gap: Rect::new(10, 1, 2, 8),
            right: Rect::new(12, 1, 10, 8),
        };
        let visible_pages = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: Some(1),
            left_page: Some(0),
            right_page: Some(1),
        };

        let outcome = pending_spread_outcome(slot_areas, visible_pages, PresenterFeedback::Pending);

        assert!(!outcome.drew_image);
        assert_eq!(outcome.feedback, PresenterFeedback::Pending);
        assert_eq!(
            spread_loading_overlays(&outcome, visible_pages),
            vec![
                (slot_areas.left, "p.1".to_string()),
                (slot_areas.right, "p.2".to_string())
            ]
        );
    }
    #[test]
    fn spread_loading_overlays_ignores_fresh_ready_slots() {
        let visible_pages = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: Some(1),
            left_page: Some(0),
            right_page: Some(1),
        };
        let outcome = PresenterRenderOutcome::aggregate_slots(vec![
            PresenterSlotOutcome::active(
                Rect::new(0, 1, 10, 8),
                true,
                PresenterFeedback::None,
                false,
            ),
            PresenterSlotOutcome::active(
                Rect::new(12, 1, 10, 8),
                true,
                PresenterFeedback::None,
                false,
            ),
        ]);

        assert!(spread_loading_overlays(&outcome, visible_pages).is_empty());
    }
    #[test]
    fn spread_loading_overlays_ignores_inactive_tail_slot() {
        let visible_pages = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };
        let outcome = PresenterRenderOutcome::aggregate_slots(vec![
            PresenterSlotOutcome::active(
                Rect::new(0, 1, 10, 8),
                true,
                PresenterFeedback::None,
                false,
            ),
            PresenterSlotOutcome::inactive(Rect::new(12, 1, 10, 8)),
        ]);

        assert!(spread_loading_overlays(&outcome, visible_pages).is_empty());
        assert_eq!(outcome.feedback, PresenterFeedback::None);
    }
    #[test]
    fn sync_render_notice_clears_stale_render_error_after_success() {
        let mut app = AppState::default();
        app.set_error_notice("Could not render p.12.");

        sync_render_notice(&mut app, false, PresenterFeedback::None, "p.12");

        assert!(app.notice.is_none());
    }
    #[test]
    fn sync_render_notice_clears_stale_render_error_while_pending() {
        let mut app = AppState::default();
        app.set_error_notice("Could not render p.12.");

        sync_render_notice(&mut app, false, PresenterFeedback::Pending, "p.12");

        assert!(app.notice.is_none());
    }
    #[test]
    fn sync_render_notice_preserves_non_render_notice() {
        let mut app = AppState::default();
        app.set_error_notice("search failed: backend failed");

        sync_render_notice(&mut app, false, PresenterFeedback::None, "p.12");

        assert_eq!(
            app.notice.as_ref().map(|notice| notice.message.as_str()),
            Some("search failed: backend failed")
        );
    }
    #[test]
    fn render_failure_message_uses_single_page_label() {
        assert_eq!(
            render_failure_message(Some("p.12")),
            "Could not render p.12."
        );
    }
    #[test]
    fn render_failure_message_uses_spread_label() {
        assert_eq!(
            render_failure_message(Some("pp.12-13")),
            "Could not render pp.12-13."
        );
    }
    #[test]
    fn render_failure_message_falls_back_to_current_page() {
        assert_eq!(
            render_failure_message(None),
            "Could not render the current page."
        );
    }
    #[test]
    fn presenter_render_options_derive_stale_fallback_from_viewer_image_state() {
        let with_image = presenter_render_options(true, PresenterRenderMode::Full, false, false);
        let without_image =
            presenter_render_options(false, PresenterRenderMode::InitialPreview, false, false);

        assert!(with_image.allow_stale_fallback);
        assert!(!without_image.allow_stale_fallback);
        assert!(with_image.preserve_stable_image);
        assert!(!with_image.force_image_redraw);
        assert_eq!(with_image.render_mode, PresenterRenderMode::Full);
        assert_eq!(
            without_image.render_mode,
            PresenterRenderMode::InitialPreview
        );
    }
    #[test]
    fn presenter_render_options_force_redraw_after_occlusion() {
        let after_overlay = presenter_render_options(true, PresenterRenderMode::Full, false, true);
        assert!(after_overlay.force_image_redraw);
    }
}
