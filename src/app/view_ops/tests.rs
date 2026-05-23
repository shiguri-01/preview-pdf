use std::path::{Path, PathBuf};

use super::spread::{
    SpreadSlotAreas, format_loading_target, format_render_target, render_areas_to_slots,
    split_spread_slot_areas,
};
use super::viewer_outcome::{
    ViewerDisplayDecision, decide_viewer_display, normalize_render_outcome, pending_spread_outcome,
    presenter_render_options, render_failure_message, spread_loading_overlays, sync_render_notice,
};
use super::{InitialPreviewPlan, compute_initial_preview_plan, resolve_layout_dimensions};
use crate::app::{AppState, PageLayoutMode, VisiblePageSlots};
use crate::backend::{PdfBackend, RgbaFrame, TextPage};
use crate::presenter::{
    PresenterFeedback, PresenterHorizontalAlign, PresenterRenderMode, PresenterRenderOutcome,
    PresenterSlotOutcome,
};
use crate::render::cache::RenderedPageKey;
use ratatui::layout::Rect;

struct DimPdf {
    path: PathBuf,
    dims: Vec<(f32, f32)>,
}

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

impl DimPdf {
    fn new(dims: Vec<(f32, f32)>) -> Self {
        Self {
            path: PathBuf::from("dims.pdf"),
            dims,
        }
    }
}

impl PdfBackend for DimPdf {
    fn path(&self) -> &Path {
        &self.path
    }

    fn doc_id(&self) -> u64 {
        1
    }

    fn page_count(&self) -> usize {
        self.dims.len()
    }

    fn page_dimensions(&self, page: usize) -> crate::error::AppResult<(f32, f32)> {
        self.dims
            .get(page)
            .copied()
            .ok_or(crate::error::AppError::invalid_argument("out of range"))
    }

    fn render_page(&self, _page: usize, _scale: f32) -> crate::error::AppResult<RgbaFrame> {
        Ok(RgbaFrame {
            width: 1,
            height: 1,
            pixels: vec![0_u8; 4].into(),
        })
    }

    fn extract_text(&self, _page: usize) -> crate::error::AppResult<String> {
        Ok(String::new())
    }

    fn extract_positioned_text(&self, _page: usize) -> crate::error::AppResult<TextPage> {
        Ok(TextPage {
            width_pt: 612.0,
            height_pt: 792.0,
            glyphs: Vec::new(),
            dropped_glyphs: 0,
        })
    }

    fn extract_outline(&self) -> crate::error::AppResult<Vec<crate::backend::OutlineNode>> {
        Ok(Vec::new())
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
        PresenterSlotOutcome::active(Rect::new(0, 1, 10, 8), true, PresenterFeedback::None, false),
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
        PresenterSlotOutcome::active(Rect::new(0, 1, 10, 8), true, PresenterFeedback::None, false),
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

#[test]
fn resolve_layout_dimensions_uses_blank_partner_width_for_tail_spread() {
    let pdf = DimPdf::new(vec![(200.0, 300.0)]);
    let slots = VisiblePageSlots {
        anchor_page: 0,
        trailing_page: None,
        left_page: Some(0),
        right_page: None,
    };

    let single = resolve_layout_dimensions(&pdf, PageLayoutMode::Single, slots);
    let spread = resolve_layout_dimensions(&pdf, PageLayoutMode::Spread, slots);

    assert_eq!(single, (200.0, 300.0));
    assert_eq!(spread, (400.0, 300.0));
}

#[test]
fn resolve_layout_dimensions_uses_both_pages_when_trailing_exists() {
    let pdf = DimPdf::new(vec![(200.0, 300.0), (180.0, 280.0)]);
    let slots = VisiblePageSlots {
        anchor_page: 0,
        trailing_page: Some(1),
        left_page: Some(0),
        right_page: Some(1),
    };

    let spread = resolve_layout_dimensions(&pdf, PageLayoutMode::Spread, slots);
    assert_eq!(spread, (400.0, 300.0));
}

#[test]
fn split_spread_slot_areas_preserves_gap_and_stable_widths() {
    let slots = split_spread_slot_areas(Rect::new(10, 2, 41, 20), 3);

    assert_eq!(slots.left, Rect::new(10, 2, 19, 20));
    assert_eq!(slots.gap, Rect::new(29, 2, 3, 20));
    assert_eq!(slots.right, Rect::new(32, 2, 19, 20));
    assert_eq!(slots.right.x - (slots.left.x + slots.left.width), 3);
}

#[test]
fn render_areas_to_slots_preserves_offscreen_spread_slot_positions() {
    let right_area = Rect::new(4, 2, 12, 8);

    let slots = render_areas_to_slots([None, Some(right_area)], PresenterRenderMode::Full);

    assert_eq!(slots.len(), 2);
    assert!(!slots[0].active);
    assert_eq!(slots[0].area, Rect::default());
    assert_eq!(slots[0].horizontal_align, PresenterHorizontalAlign::End);
    assert!(slots[1].active);
    assert_eq!(slots[1].area, right_area);
    assert_eq!(slots[1].horizontal_align, PresenterHorizontalAlign::Start);
}

#[test]
fn compute_initial_preview_plan_uses_lower_scale_on_cold_start() {
    let slots = VisiblePageSlots {
        anchor_page: 0,
        trailing_page: None,
        left_page: Some(0),
        right_page: None,
    };

    let preview = compute_initial_preview_plan(7, slots, PageLayoutMode::Single, 1.0);

    assert_eq!(
        preview,
        Some(InitialPreviewPlan {
            scale: 0.25,
            page_keys: vec![RenderedPageKey::new(7, 0, 0.25)],
            presenter_key: RenderedPageKey::new(7, 0, 0.25),
        })
    );
}

#[test]
fn compute_initial_preview_plan_includes_both_spread_pages() {
    let slots = VisiblePageSlots {
        anchor_page: 0,
        trailing_page: Some(1),
        left_page: Some(0),
        right_page: Some(1),
    };

    let preview = compute_initial_preview_plan(7, slots, PageLayoutMode::Spread, 1.0);

    assert_eq!(
        preview,
        Some(InitialPreviewPlan {
            scale: 0.25,
            page_keys: vec![
                RenderedPageKey::new(7, 0, 0.25),
                RenderedPageKey::new(7, 1, 0.25),
            ],
            presenter_key: RenderedPageKey::new(7, 0, 0.25),
        })
    );
}

#[test]
fn compute_initial_preview_plan_handles_tail_spread() {
    let slots = VisiblePageSlots {
        anchor_page: 2,
        trailing_page: None,
        left_page: Some(2),
        right_page: None,
    };

    let preview = compute_initial_preview_plan(7, slots, PageLayoutMode::Spread, 1.0);

    assert_eq!(
        preview,
        Some(InitialPreviewPlan {
            scale: 0.25,
            page_keys: vec![RenderedPageKey::new(7, 2, 0.25)],
            presenter_key: RenderedPageKey::new(7, 2, 0.25),
        })
    );
}

#[test]
fn loading_target_formats_single_page_with_p_prefix() {
    let label = format_loading_target(VisiblePageSlots {
        anchor_page: 11,
        trailing_page: None,
        left_page: Some(11),
        right_page: None,
    });

    assert_eq!(label, "p.12");
}

#[test]
fn loading_target_formats_spread_with_pp_prefix() {
    let label = format_loading_target(VisiblePageSlots {
        anchor_page: 11,
        trailing_page: Some(12),
        left_page: Some(11),
        right_page: Some(12),
    });

    assert_eq!(label, "pp.12-13");
}

#[test]
fn render_target_uses_error_label_convention() {
    let label = format_render_target(VisiblePageSlots {
        anchor_page: 11,
        trailing_page: Some(12),
        left_page: Some(11),
        right_page: Some(12),
    });

    assert_eq!(label, "pp.12-13");
}
