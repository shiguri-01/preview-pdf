use super::geometry::{align_rect_within, center_rect_within, centered_fit_area};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use crate::backend::RgbaFrame;
use crate::presenter::l2_cache::TerminalFrameState;
use crate::presenter::{
    ImagePresenter, PanOffset, PresenterFeedback, PresenterHorizontalAlign, PresenterRenderMode,
    PresenterRenderOptions, PresenterRenderSlot, PresenterSlot, Viewport,
};
use crate::render::cache::RenderedPageKey;

fn frame() -> RgbaFrame {
    RgbaFrame {
        width: 4,
        height: 4,
        pixels: vec![200; 4 * 4 * 4].into(),
    }
}

fn render_until_ready(presenter: &mut super::RatatuiImagePresenter, area: Rect) {
    let backend = TestBackend::new(20, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let deadline = Instant::now() + Duration::from_secs(2);
    while presenter
        .state
        .last_ready_keys
        .first()
        .copied()
        .flatten()
        .is_none()
        && Instant::now() < deadline
    {
        terminal
            .draw(|frame| {
                let _ = presenter.render(frame, area, PresenterRenderOptions::default());
            })
            .expect("draw should pass");
        let _ = presenter.drain_background_events();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        presenter
            .state
            .last_ready_keys
            .first()
            .copied()
            .flatten()
            .is_some(),
        "presenter should have a ready frame for fallback"
    );
}

#[test]
fn render_pending_uses_stale_fallback_when_allowed() {
    let mut presenter = super::RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let area = Rect::new(1, 1, 12, 7);
    presenter
        .prepare(
            RenderedPageKey::new(9, 1, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            0,
            1,
        )
        .expect("first prepare should pass");
    render_until_ready(&mut presenter, area);
    presenter
        .prepare(
            RenderedPageKey::new(9, 2, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            0,
            2,
        )
        .expect("second prepare should pass");

    let backend = TestBackend::new(20, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut result = None;
    terminal
        .draw(|frame| {
            result = Some(presenter.render(
                frame,
                area,
                PresenterRenderOptions::new(true, PresenterRenderMode::Full),
            ));
        })
        .expect("draw should pass");

    let outcome = result
        .expect("render result should be captured")
        .expect("render should succeed");
    assert_eq!(outcome.feedback, PresenterFeedback::Pending);
    assert!(outcome.drew_image);
    assert!(outcome.used_stale_fallback);
    assert_eq!(outcome.slots.len(), 1);
    assert_eq!(outcome.slots[0].area, area);
    assert!(outcome.slots[0].used_stale_fallback);
}

fn render_slots_until_ready(
    presenter: &mut super::RatatuiImagePresenter,
    slots: &[PresenterRenderSlot],
) {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let deadline = Instant::now() + Duration::from_secs(2);
    while presenter.state.last_ready_keys.iter().any(Option::is_none) && Instant::now() < deadline {
        terminal
            .draw(|frame| {
                let _ = presenter.render_slots(frame, slots);
            })
            .expect("draw should pass");
        let _ = presenter.drain_background_events();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        presenter.state.last_ready_keys.iter().all(Option::is_some),
        "presenter should have ready frames for all slots"
    );
}

#[test]
fn presenter_tracks_last_drawn_area_for_stable_redraws() {
    let mut presenter = super::RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let area = Rect::new(2, 1, 12, 7);
    presenter
        .prepare(
            RenderedPageKey::new(1, 0, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            0,
            1,
        )
        .expect("prepare should pass");

    render_until_ready(&mut presenter, area);
    let first_drawn_area =
        presenter.state.last_drawn_areas[0].expect("ready render should record drawn area");

    let backend = TestBackend::new(20, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    terminal
        .draw(|frame| {
            presenter
                .render(frame, area, PresenterRenderOptions::default())
                .expect("ready redraw should pass");
        })
        .expect("draw should pass");

    assert_eq!(presenter.state.last_drawn_areas[0], Some(first_drawn_area));
}

#[test]
fn presenter_preserves_stable_ready_image_without_reblitting() {
    let mut presenter = super::RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let area = Rect::new(2, 1, 12, 7);
    presenter
        .prepare(
            RenderedPageKey::new(1, 0, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            0,
            1,
        )
        .expect("prepare should pass");

    render_until_ready(&mut presenter, area);
    let first_drawn_key = presenter.state.last_drawn_keys[0];
    let first_drawn_area = presenter.state.last_drawn_areas[0];
    presenter.clear_perf_blit_metrics();

    let options = PresenterRenderOptions {
        preserve_stable_image: true,
        ..PresenterRenderOptions::default()
    };
    let backend = TestBackend::new(20, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut outcome = None;
    terminal
        .draw(|frame| {
            outcome = Some(
                presenter
                    .render(frame, area, options)
                    .expect("stable redraw should pass"),
            );
        })
        .expect("draw should pass");

    assert!(
        outcome.expect("outcome should be captured").drew_image,
        "preserved image should still count as visible image content"
    );
    assert_eq!(presenter.state.last_drawn_keys[0], first_drawn_key);
    assert_eq!(presenter.state.last_drawn_areas[0], first_drawn_area);
    assert_eq!(presenter.perf_stats().blit_samples, 0);
}

#[test]
fn inactive_spread_slot_forgets_last_drawn_area() {
    let mut presenter = super::RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let left_area = Rect::new(0, 0, 12, 7);
    let right_area = Rect::new(15, 0, 12, 7);
    let slots = [
        PresenterRenderSlot {
            area: left_area,
            options: PresenterRenderOptions::default(),
            active: true,
            horizontal_align: PresenterHorizontalAlign::End,
        },
        PresenterRenderSlot {
            area: right_area,
            options: PresenterRenderOptions::default(),
            active: true,
            horizontal_align: PresenterHorizontalAlign::Start,
        },
    ];
    presenter
        .prepare_slots(&[
            PresenterSlot {
                cache_key: Some(RenderedPageKey::new(1, 0, 1.0)),
                frame: Some(&frame()),
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
            PresenterSlot {
                cache_key: Some(RenderedPageKey::new(1, 1, 1.0)),
                frame: Some(&frame()),
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
        ])
        .expect("slot prepare should pass");

    render_slots_until_ready(&mut presenter, &slots);
    assert!(
        presenter.state.last_drawn_areas[1].is_some(),
        "ready right slot should record a drawn area"
    );
    let right_drawn_area = presenter.state.last_drawn_areas[1].unwrap();

    let inactive_right = [
        slots[0],
        PresenterRenderSlot {
            area: Rect::default(),
            active: false,
            ..slots[1]
        },
    ];
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    terminal
        .draw(|frame| {
            for y in right_drawn_area.top()..right_drawn_area.bottom() {
                for x in right_drawn_area.left()..right_drawn_area.right() {
                    if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                        cell.set_symbol("x");
                    }
                }
            }
            presenter
                .render_slots(frame, &inactive_right)
                .expect("inactive render should pass");
        })
        .expect("draw should pass");

    let buffer = terminal.backend().buffer();
    for y in right_drawn_area.top()..right_drawn_area.bottom() {
        for x in right_drawn_area.left()..right_drawn_area.right() {
            assert_eq!(buffer[(x, y)].symbol(), " ");
        }
    }
    assert_eq!(presenter.state.last_drawn_areas[1], None);
    assert_eq!(presenter.state.last_drawn_keys[1], None);
}

#[test]
fn presenter_prepare_slots_tracks_independent_current_keys() {
    let mut presenter = super::RatatuiImagePresenter::new();
    let left_viewport = Viewport {
        x: 0,
        y: 0,
        width: 40,
        height: 24,
    };
    let right_viewport = Viewport {
        x: 42,
        y: 0,
        width: 40,
        height: 24,
    };
    let left_key = RenderedPageKey::new(1, 0, 1.0);
    let right_key = RenderedPageKey::new(1, 1, 1.0);
    let left_frame = frame();
    let right_frame = frame();
    let slots = [
        PresenterSlot {
            cache_key: Some(left_key),
            frame: Some(&left_frame),
            viewport: left_viewport,
            pan: PanOffset::default(),
            overlay_stamp: 0,
            generation: 1,
        },
        PresenterSlot {
            cache_key: Some(right_key),
            frame: Some(&right_frame),
            viewport: right_viewport,
            pan: PanOffset::default(),
            overlay_stamp: 0,
            generation: 1,
        },
    ];

    presenter
        .prepare_slots(&slots)
        .expect("slot prepare should pass");

    assert_eq!(presenter.state.current_keys.len(), 2);
    assert_eq!(
        presenter.state.current_keys[0].map(|key| key.rendered_page),
        Some(left_key)
    );
    assert_eq!(
        presenter.state.current_keys[1].map(|key| key.rendered_page),
        Some(right_key)
    );
    assert_eq!(presenter.l2_cache_len(), 2);
}

#[test]
fn presenter_prepare_slots_preserves_empty_slot_positions() {
    let mut presenter = super::RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 42,
        y: 0,
        width: 40,
        height: 24,
    };
    let right_key = RenderedPageKey::new(1, 2, 1.0);
    let right_frame = frame();
    let slots = [
        PresenterSlot {
            cache_key: None,
            frame: None,
            viewport,
            pan: PanOffset::default(),
            overlay_stamp: 0,
            generation: 1,
        },
        PresenterSlot {
            cache_key: Some(right_key),
            frame: Some(&right_frame),
            viewport,
            pan: PanOffset::default(),
            overlay_stamp: 0,
            generation: 1,
        },
    ];

    presenter
        .prepare_slots(&slots)
        .expect("slot prepare should pass");

    assert_eq!(presenter.state.current_keys.len(), 2);
    assert_eq!(presenter.state.current_keys[0], None);
    assert_eq!(
        presenter.state.current_keys[1].map(|key| key.rendered_page),
        Some(right_key)
    );
    assert_eq!(presenter.l2_cache_len(), 1);
}

#[test]
fn render_slots_ignores_inactive_slots_for_feedback() {
    let mut presenter = super::RatatuiImagePresenter::new();
    let backend = TestBackend::new(30, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut result = None;

    terminal
        .draw(|frame| {
            result = Some(presenter.render_slots(
                frame,
                &[PresenterRenderSlot {
                    area: Rect::new(0, 0, 12, 7),
                    options: PresenterRenderOptions::default(),
                    active: false,
                    horizontal_align: PresenterHorizontalAlign::Center,
                }],
            ));
        })
        .expect("draw should pass");

    let outcome = result
        .expect("render result should be captured")
        .expect("render should pass");
    assert_eq!(outcome.feedback, PresenterFeedback::None);
    assert!(!outcome.drew_image);
    assert_eq!(outcome.slots.len(), 1);
    assert_eq!(outcome.slots[0].area, Rect::new(0, 0, 12, 7));
    assert!(!outcome.slots[0].active);
}

#[test]
fn render_slots_aggregates_failed_and_pending_feedback() {
    let mut presenter = super::RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let left_key = RenderedPageKey::new(1, 0, 1.0);
    let right_key = RenderedPageKey::new(1, 1, 1.0);
    let left_frame = frame();
    let right_frame = frame();
    let slots = [
        PresenterSlot {
            cache_key: Some(left_key),
            frame: Some(&left_frame),
            viewport,
            pan: PanOffset::default(),
            overlay_stamp: 0,
            generation: 1,
        },
        PresenterSlot {
            cache_key: Some(right_key),
            frame: Some(&right_frame),
            viewport,
            pan: PanOffset::default(),
            overlay_stamp: 0,
            generation: 1,
        },
    ];
    presenter
        .prepare_slots(&slots)
        .expect("slot prepare should pass");
    let failed_key = presenter.state.current_keys[0].expect("left key should exist");
    presenter
        .state
        .l2_cache
        .set_state(&failed_key, TerminalFrameState::Failed);

    let backend = TestBackend::new(30, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut result = None;
    terminal
        .draw(|frame| {
            result = Some(presenter.render_slots(
                frame,
                &[
                    PresenterRenderSlot {
                        area: Rect::new(0, 0, 12, 7),
                        options: PresenterRenderOptions::default(),
                        active: true,
                        horizontal_align: PresenterHorizontalAlign::End,
                    },
                    PresenterRenderSlot {
                        area: Rect::new(15, 0, 12, 7),
                        options: PresenterRenderOptions::default(),
                        active: true,
                        horizontal_align: PresenterHorizontalAlign::Start,
                    },
                ],
            ));
        })
        .expect("draw should pass");

    let outcome = result
        .expect("render result should be captured")
        .expect("render should pass");
    assert_eq!(outcome.feedback, PresenterFeedback::Failed);
    assert!(!outcome.drew_image);
    assert_eq!(outcome.slots.len(), 2);
    assert_eq!(outcome.slots[0].feedback, PresenterFeedback::Failed);
    assert_eq!(outcome.slots[1].feedback, PresenterFeedback::Pending);
}

#[test]
fn render_slots_reports_stale_fallback_per_slot() {
    let mut presenter = super::RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let left_area = Rect::new(0, 0, 12, 7);
    let right_area = Rect::new(15, 0, 12, 7);
    let left_key = RenderedPageKey::new(1, 0, 1.0);
    let right_key = RenderedPageKey::new(1, 1, 1.0);
    let next_right_key = RenderedPageKey::new(1, 2, 1.0);
    let left_frame = frame();
    let right_frame = frame();
    let next_right_frame = frame();
    let render_slots = [
        PresenterRenderSlot {
            area: left_area,
            options: PresenterRenderOptions::new(true, PresenterRenderMode::Full),
            active: true,
            horizontal_align: PresenterHorizontalAlign::End,
        },
        PresenterRenderSlot {
            area: right_area,
            options: PresenterRenderOptions::new(true, PresenterRenderMode::Full),
            active: true,
            horizontal_align: PresenterHorizontalAlign::Start,
        },
    ];

    presenter
        .prepare_slots(&[
            PresenterSlot {
                cache_key: Some(left_key),
                frame: Some(&left_frame),
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
            PresenterSlot {
                cache_key: Some(right_key),
                frame: Some(&right_frame),
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 1,
            },
        ])
        .expect("initial slot prepare should pass");
    render_slots_until_ready(&mut presenter, &render_slots);

    presenter
        .prepare_slots(&[
            PresenterSlot {
                cache_key: Some(left_key),
                frame: Some(&left_frame),
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 2,
            },
            PresenterSlot {
                cache_key: Some(next_right_key),
                frame: Some(&next_right_frame),
                viewport,
                pan: PanOffset::default(),
                overlay_stamp: 0,
                generation: 2,
            },
        ])
        .expect("second slot prepare should pass");

    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut result = None;
    terminal
        .draw(|frame| {
            result = Some(presenter.render_slots(frame, &render_slots));
        })
        .expect("draw should pass");

    let outcome = result
        .expect("render result should be captured")
        .expect("render should pass");
    assert_eq!(outcome.feedback, PresenterFeedback::Pending);
    assert!(outcome.drew_image);
    assert!(outcome.used_stale_fallback);
    assert_eq!(outcome.slots.len(), 2);
    assert_eq!(outcome.slots[0].area, left_area);
    assert_eq!(outcome.slots[0].feedback, PresenterFeedback::None);
    assert!(outcome.slots[0].drew_image);
    assert!(!outcome.slots[0].used_stale_fallback);
    assert_eq!(outcome.slots[1].area, right_area);
    assert_eq!(outcome.slots[1].feedback, PresenterFeedback::Pending);
    assert!(outcome.slots[1].drew_image);
    assert!(outcome.slots[1].used_stale_fallback);
}

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
