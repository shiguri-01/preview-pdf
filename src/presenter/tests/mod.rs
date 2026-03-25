use std::thread;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use tokio::runtime::Builder;

use crate::backend::RgbaFrame;
use crate::render::cache::RenderedPageKey;
use crate::render::prefetch::{PrefetchClass, PrefetchQueue, PrefetchQueueConfig};

use super::encode::{EncodeWorkerRequest, enqueue_encode_request, pop_next_encode_task};
use super::factory::create_presenter;
use super::l2_cache::{L2_MAX_ENTRIES, TerminalFrameCache, TerminalFrameKey, TerminalFrameState};
use super::ratatui::RatatuiImagePresenter;
use super::terminal_cell::cell_size_from_window_metrics;
use super::traits::{
    ImagePresenter, PanOffset, PresenterBackgroundEvent, PresenterFeedback, PresenterKind,
    PresenterRenderMode, PresenterRenderOptions, Viewport,
};

fn frame() -> RgbaFrame {
    RgbaFrame {
        width: 4,
        height: 4,
        pixels: vec![200; 4 * 4 * 4].into(),
    }
}

fn l2_key(page: usize) -> TerminalFrameKey {
    TerminalFrameKey {
        rendered_page: RenderedPageKey::new(1, page, 1.0),
        viewport: Viewport {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        },
        pan: PanOffset::default(),
    }
}

fn render_until_ready(presenter: &mut RatatuiImagePresenter, area: Rect) {
    let backend = TestBackend::new(20, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let deadline = Instant::now() + Duration::from_secs(2);
    while presenter.state.last_ready_key.is_none() && Instant::now() < deadline {
        terminal
            .draw(|frame| {
                let _ = presenter.render(frame, area, PresenterRenderOptions::default());
            })
            .expect("draw should pass");
        let _ = presenter.drain_background_events();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        presenter.state.last_ready_key.is_some(),
        "presenter should have a ready frame for fallback"
    );
}

#[test]
fn select_ratatui_presenter() {
    let presenter = create_presenter(PresenterKind::RatatuiImage)
        .expect("ratatui presenter should be selectable");
    assert_eq!(presenter.capabilities().backend_name, "ratatui-image");
}

#[test]
fn presenter_runtime_info_exposes_graphics_protocol_when_available() {
    let presenter = RatatuiImagePresenter::new();
    assert!(presenter.runtime_info().graphics_protocol.is_some());
}

#[test]
fn presenter_with_cache_limits_applies_l2_cache_limits() {
    let presenter = RatatuiImagePresenter::with_cache_limits(5, 2048);
    assert_eq!(presenter.state.l2_cache.max_entries(), 5);
    assert_eq!(presenter.state.l2_cache.memory_budget_bytes(), 2048);
}

#[test]
fn presenter_uses_l2_cache_between_same_frames() {
    let mut presenter = RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };

    presenter
        .prepare(
            RenderedPageKey::new(1, 0, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            0,
        )
        .expect("first prepare should pass");
    presenter
        .prepare(
            RenderedPageKey::new(1, 0, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            0,
        )
        .expect("second prepare should pass");

    assert_eq!(presenter.l2_cache_len(), 1);
    assert!(presenter.perf_stats().cache_hit_rate_l2 > 0.0);
}

#[test]
fn presenter_cache_key_distinguishes_pages_even_with_same_pixels() {
    let mut presenter = RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };

    presenter
        .prepare(
            RenderedPageKey::new(1, 0, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            0,
        )
        .expect("first prepare should pass");
    presenter
        .prepare(
            RenderedPageKey::new(1, 1, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            0,
        )
        .expect("second page prepare should pass");

    assert_eq!(presenter.l2_cache_len(), 2);
}

#[test]
fn presenter_renders_after_prepare() {
    let mut presenter = RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    presenter
        .prepare(
            RenderedPageKey::new(1, 0, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            0,
        )
        .expect("prepare should pass");

    let backend = TestBackend::new(20, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    for _ in 0..80 {
        terminal
            .draw(|frame| {
                let _ = presenter
                    .render(
                        frame,
                        Rect::new(1, 1, 12, 7),
                        PresenterRenderOptions::default(),
                    )
                    .expect("render should pass");
            })
            .expect("draw should pass");
        let _ = presenter.drain_background_events();
        if presenter.perf_stats().blit_samples >= 1 {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert!(presenter.perf_stats().blit_samples >= 1);
}

#[test]
fn prefetch_encode_advances_entry_to_ready() {
    let mut presenter = RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let rendered_page = RenderedPageKey::new(3, 1, 1.0);
    let key = TerminalFrameKey {
        rendered_page,
        viewport,
        pan: PanOffset::default(),
    };

    presenter
        .prefetch_encode(
            rendered_page,
            &frame(),
            viewport,
            PanOffset::default(),
            PrefetchClass::DirectionalLead,
            1,
        )
        .expect("prefetch should pass");

    let mut ready = false;
    for _ in 0..80 {
        let _ = presenter.drain_background_events();
        ready = matches!(
            presenter
                .state
                .l2_cache
                .entries
                .get(&key)
                .map(|entry| &entry.state),
            Some(TerminalFrameState::Ready(_))
        );
        if ready {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert!(ready);
}

#[test]
fn prefetch_encode_does_not_change_current_key() {
    let mut presenter = RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };
    let current = RenderedPageKey::new(1, 0, 1.0);
    let prefetch = RenderedPageKey::new(1, 1, 1.0);

    presenter
        .prepare(current, &frame(), viewport, PanOffset::default(), 0)
        .expect("prepare should pass");
    let before = presenter.state.current_key;

    presenter
        .prefetch_encode(
            prefetch,
            &frame(),
            viewport,
            PanOffset::default(),
            PrefetchClass::DirectionalLead,
            1,
        )
        .expect("prefetch should pass");

    assert_eq!(presenter.state.current_key, before);
}

#[test]
fn presenter_has_pending_work_tracks_encode_progress() {
    let mut presenter = RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let rendered_page = RenderedPageKey::new(4, 1, 1.0);

    presenter
        .prefetch_encode(
            rendered_page,
            &frame(),
            viewport,
            PanOffset::default(),
            PrefetchClass::DirectionalLead,
            1,
        )
        .expect("prefetch should pass");

    assert!(presenter.has_pending_work());

    let deadline = Instant::now() + Duration::from_secs(2);
    while presenter.has_pending_work() && Instant::now() < deadline {
        let _ = presenter.drain_background_events();
        thread::sleep(Duration::from_millis(5));
    }

    assert!(!presenter.has_pending_work());
}

#[test]
fn recv_background_event_requests_redraw_for_current_encode_completion() {
    let mut presenter = RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let area = Rect::new(1, 1, 12, 7);
    presenter
        .prepare(
            RenderedPageKey::new(11, 0, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            1,
        )
        .expect("prepare should pass");

    let backend = TestBackend::new(20, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    terminal
        .draw(|frame| {
            presenter
                .render(frame, area, PresenterRenderOptions::default())
                .expect("render should pass");
        })
        .expect("draw should pass");

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let event = runtime
        .block_on(presenter.recv_background_event())
        .expect("encode completion event should arrive");

    assert_eq!(
        event,
        PresenterBackgroundEvent::EncodeComplete {
            redraw_requested: true,
        }
    );
}

#[test]
fn render_pending_uses_stale_fallback_when_allowed() {
    let mut presenter = RatatuiImagePresenter::new();
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
}

#[test]
fn render_pending_keeps_stale_fallback_when_new_oversize_entry_arrives() {
    let mut presenter = RatatuiImagePresenter::with_cache_limits(8, 32);
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let area = Rect::new(1, 1, 12, 7);
    let oversize = frame();

    presenter
        .prepare(
            RenderedPageKey::new(9, 1, 0.25),
            &oversize,
            viewport,
            PanOffset::default(),
            1,
        )
        .expect("preview prepare should pass");
    render_until_ready(&mut presenter, area);

    presenter
        .prepare(
            RenderedPageKey::new(9, 1, 1.0),
            &oversize,
            viewport,
            PanOffset::default(),
            2,
        )
        .expect("full prepare should pass");

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
}

#[test]
fn render_pending_does_not_use_stale_fallback_when_disallowed() {
    let mut presenter = RatatuiImagePresenter::new();
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
                PresenterRenderOptions::new(false, PresenterRenderMode::Full),
            ));
        })
        .expect("draw should pass");

    let outcome = result
        .expect("render result should be captured")
        .expect("render should succeed");
    assert_eq!(outcome.feedback, PresenterFeedback::Pending);
    assert!(!outcome.drew_image);
    assert!(!outcome.used_stale_fallback);
}

#[test]
fn render_returns_failed_feedback_for_failed_current_entry() {
    let mut presenter = RatatuiImagePresenter::new();
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
            1,
        )
        .expect("prepare should pass");
    render_until_ready(&mut presenter, area);
    presenter
        .prepare(
            RenderedPageKey::new(9, 2, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            2,
        )
        .expect("second prepare should pass");
    let key = presenter
        .state
        .current_key
        .expect("current key should exist");
    if let Some(entry) = presenter.state.l2_cache.cached_mut(&key) {
        entry.state = TerminalFrameState::Failed;
    }

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
        .expect("failed entry should be reported as feedback");
    assert_eq!(outcome.feedback, PresenterFeedback::Failed);
    assert!(outcome.drew_image);
    assert!(outcome.used_stale_fallback);
}

#[test]
fn render_failed_does_not_use_stale_fallback_when_disallowed() {
    let mut presenter = RatatuiImagePresenter::new();
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
            1,
        )
        .expect("prepare should pass");
    render_until_ready(&mut presenter, area);
    presenter
        .prepare(
            RenderedPageKey::new(9, 2, 1.0),
            &frame(),
            viewport,
            PanOffset::default(),
            2,
        )
        .expect("second prepare should pass");
    let key = presenter
        .state
        .current_key
        .expect("current key should exist");
    if let Some(entry) = presenter.state.l2_cache.cached_mut(&key) {
        entry.state = TerminalFrameState::Failed;
    }

    let backend = TestBackend::new(20, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut result = None;
    terminal
        .draw(|frame| {
            result = Some(presenter.render(
                frame,
                area,
                PresenterRenderOptions::new(false, PresenterRenderMode::Full),
            ));
        })
        .expect("draw should pass");

    let outcome = result
        .expect("render result should be captured")
        .expect("render should report failed feedback");
    assert_eq!(outcome.feedback, PresenterFeedback::Failed);
    assert!(!outcome.drew_image);
    assert!(!outcome.used_stale_fallback);
}

#[test]
fn render_reports_failed_feedback_when_encode_worker_is_disconnected() {
    let mut presenter = RatatuiImagePresenter::new();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let rendered_page = RenderedPageKey::new(7, 1, 1.0);
    presenter
        .prepare(rendered_page, &frame(), viewport, PanOffset::default(), 0)
        .expect("prepare should pass");
    let key = TerminalFrameKey {
        rendered_page,
        viewport,
        pan: PanOffset::default(),
    };
    presenter.state.current_key = Some(key);
    presenter.shutdown_worker();

    let backend = TestBackend::new(20, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let mut result = None;
    terminal
        .draw(|frame| {
            result = Some(presenter.render(
                frame,
                Rect::new(1, 1, 12, 7),
                PresenterRenderOptions::default(),
            ));
        })
        .expect("draw should pass");

    let outcome = result
        .expect("render result should be captured")
        .expect("disconnected worker should be reported as feedback");
    assert_eq!(outcome.feedback, PresenterFeedback::Failed);
    assert!(!outcome.drew_image);
    assert!(matches!(
        presenter
            .state
            .l2_cache
            .entries
            .get(&key)
            .map(|entry| &entry.state),
        Some(TerminalFrameState::Failed)
    ));
}

#[test]
fn encode_queue_prioritizes_current_over_prefetch() {
    let presenter = RatatuiImagePresenter::new();
    let area = Rect::new(0, 0, 12, 7);
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let mut queue = PrefetchQueue::new(PrefetchQueueConfig::default());

    let low_key_1 = TerminalFrameKey {
        rendered_page: RenderedPageKey::new(1, 1, 1.0),
        viewport,
        pan: PanOffset::default(),
    };
    let low_key_2 = TerminalFrameKey {
        rendered_page: RenderedPageKey::new(1, 2, 1.0),
        viewport,
        pan: PanOffset::default(),
    };
    let high_key = TerminalFrameKey {
        rendered_page: RenderedPageKey::new(1, 3, 1.0),
        viewport,
        pan: PanOffset::default(),
    };

    let low_req_1 = EncodeWorkerRequest::Encode {
        key: low_key_1,
        picker: presenter.config.picker.clone(),
        frame: frame(),
        area,
        allow_upscale: false,
        class: PrefetchClass::DirectionalLead,
        generation: 1,
        enqueued_at: Instant::now(),
    };
    let low_req_2 = EncodeWorkerRequest::Encode {
        key: low_key_2,
        picker: presenter.config.picker.clone(),
        frame: frame(),
        area,
        allow_upscale: false,
        class: PrefetchClass::DirectionalLead,
        generation: 1,
        enqueued_at: Instant::now(),
    };
    let high_req = EncodeWorkerRequest::Encode {
        key: high_key,
        picker: presenter.config.picker.clone(),
        frame: frame(),
        area,
        allow_upscale: false,
        class: PrefetchClass::CriticalCurrent,
        generation: 1,
        enqueued_at: Instant::now(),
    };

    assert!(enqueue_encode_request(low_req_1, &mut queue));
    assert!(enqueue_encode_request(low_req_2, &mut queue));
    assert!(enqueue_encode_request(high_req, &mut queue));

    let first = pop_next_encode_task(&mut queue).expect("first task should exist");
    let second = pop_next_encode_task(&mut queue).expect("second task should exist");
    let third = pop_next_encode_task(&mut queue).expect("third task should exist");

    assert_eq!(first.key, high_key);
    assert_eq!(second.key, low_key_1);
    assert_eq!(third.key, low_key_2);
}

#[test]
fn encode_queue_cancels_stale_prefetch_generation() {
    let presenter = RatatuiImagePresenter::new();
    let area = Rect::new(0, 0, 12, 7);
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 12,
        height: 7,
    };
    let mut queue = PrefetchQueue::new(PrefetchQueueConfig::default());

    let stale_prefetch = EncodeWorkerRequest::Encode {
        key: TerminalFrameKey {
            rendered_page: RenderedPageKey::new(1, 1, 1.0),
            viewport,
            pan: PanOffset::default(),
        },
        picker: presenter.config.picker.clone(),
        frame: frame(),
        area,
        allow_upscale: false,
        class: PrefetchClass::DirectionalLead,
        generation: 1,
        enqueued_at: Instant::now(),
    };
    let stale_background = EncodeWorkerRequest::Encode {
        key: TerminalFrameKey {
            rendered_page: RenderedPageKey::new(1, 2, 1.0),
            viewport,
            pan: PanOffset::default(),
        },
        picker: presenter.config.picker.clone(),
        frame: frame(),
        area,
        allow_upscale: false,
        class: PrefetchClass::Background,
        generation: 1,
        enqueued_at: Instant::now(),
    };
    let current = EncodeWorkerRequest::Encode {
        key: TerminalFrameKey {
            rendered_page: RenderedPageKey::new(1, 3, 1.0),
            viewport,
            pan: PanOffset::default(),
        },
        picker: presenter.config.picker.clone(),
        frame: frame(),
        area,
        allow_upscale: false,
        class: PrefetchClass::CriticalCurrent,
        generation: 2,
        enqueued_at: Instant::now(),
    };

    assert!(enqueue_encode_request(stale_prefetch, &mut queue));
    assert!(enqueue_encode_request(stale_background, &mut queue));
    assert!(enqueue_encode_request(current, &mut queue));

    let first = pop_next_encode_task(&mut queue).expect("current should remain");
    assert_eq!(first.key.rendered_page.page, 3);
    assert!(pop_next_encode_task(&mut queue).is_none());
}

#[test]
fn l2_cached_mut_does_not_touch_lru_order() {
    let mut cache = TerminalFrameCache::default();
    for page in 0..L2_MAX_ENTRIES {
        let _ = cache.insert(l2_key(page), frame(), 1, false, None);
    }

    let oldest = l2_key(0);
    assert!(cache.cached_mut(&oldest).is_some());
    let _ = cache.insert(l2_key(L2_MAX_ENTRIES), frame(), 1, false, None);

    assert!(cache.entries.peek(&oldest).is_none());
    assert!(cache.entries.peek(&l2_key(1)).is_some());
}

#[test]
fn l2_insert_at_capacity_keeps_memory_accounting_consistent() {
    let mut cache = TerminalFrameCache::default();
    for page in 0..L2_MAX_ENTRIES {
        let _ = cache.insert(l2_key(page), frame(), 16, false, None);
    }
    let _ = cache.insert(l2_key(L2_MAX_ENTRIES), frame(), 20, false, None);

    let expected = (L2_MAX_ENTRIES - 1) * 16 + 20;
    assert_eq!(cache.len(), L2_MAX_ENTRIES);
    assert_eq!(cache.memory_bytes, expected);
}

#[test]
fn l2_insert_keeps_pending_frame_buffer_shared() {
    let mut cache = TerminalFrameCache::default();
    let key = l2_key(0);
    let source = frame();
    let _ = cache.insert(key, source.clone(), source.byte_len(), false, None);

    let stored_pixels = match cache.cached_mut(&key).map(|entry| &entry.state) {
        Some(TerminalFrameState::PendingFrame(frame)) => &frame.pixels,
        _ => panic!("expected pending frame"),
    };
    assert!(source.pixels.ptr_eq(stored_pixels));
}

#[test]
fn l2_oversize_insert_without_override_preserves_existing_entries() {
    let mut cache = TerminalFrameCache::new(8, 32);
    let kept = l2_key(0);
    let oversize = l2_key(1);
    let _ = cache.insert(kept, frame(), 16, false, None);

    let inserted = cache.insert(oversize, frame(), 64, false, None);
    assert!(!inserted);
    assert!(cache.cached_mut(&kept).is_some());
    assert!(cache.cached_mut(&oversize).is_none());
}

#[test]
fn l2_oversize_insert_with_override_keeps_single_entry() {
    let mut cache = TerminalFrameCache::new(8, 32);
    let kept = l2_key(0);
    let oversize = l2_key(1);
    let _ = cache.insert(kept, frame(), 16, false, None);

    let inserted = cache.insert(oversize, frame(), 64, true, None);
    assert!(inserted);
    assert!(cache.cached_mut(&kept).is_none());
    assert!(cache.cached_mut(&oversize).is_some());
    assert_eq!(cache.len(), 1);
}

#[test]
fn l2_oversize_insert_with_protected_key_keeps_visible_entry() {
    let mut cache = TerminalFrameCache::new(8, 32);
    let visible = l2_key(0);
    let oversize = l2_key(1);
    let _ = cache.insert(visible, frame(), 16, false, None);

    let inserted = cache.insert(oversize, frame(), 64, true, Some(visible));
    assert!(inserted);
    assert!(cache.cached_mut(&visible).is_some());
    assert!(cache.cached_mut(&oversize).is_some());
    assert_eq!(cache.len(), 2);
    assert!(cache.memory_bytes > cache.memory_budget_bytes());
}

#[test]
fn l2_oversize_insert_with_protected_key_respects_single_entry_limit() {
    let mut cache = TerminalFrameCache::new(1, 32);
    let visible = l2_key(0);
    let oversize = l2_key(1);
    let _ = cache.insert(visible, frame(), 16, false, None);

    let inserted = cache.insert(oversize, frame(), 64, true, Some(visible));
    assert!(inserted);
    assert!(cache.cached_mut(&visible).is_none());
    assert!(cache.cached_mut(&oversize).is_some());
    assert_eq!(cache.len(), 1);
}

#[test]
fn l2_non_oversize_insert_does_not_evict_single_oversize_entry() {
    let mut cache = TerminalFrameCache::new(8, 32);
    let oversize = l2_key(1);
    let prefetch = l2_key(2);

    assert!(cache.insert(oversize, frame(), 64, true, None));
    assert_eq!(cache.len(), 1);
    assert!(cache.cached_mut(&oversize).is_some());

    let inserted_prefetch = cache.insert(prefetch, frame(), 16, false, None);
    assert!(!inserted_prefetch);
    assert_eq!(cache.len(), 1);
    assert!(cache.cached_mut(&oversize).is_some());
    assert!(cache.cached_mut(&prefetch).is_none());
}

#[test]
fn cell_size_from_window_metrics_divides_pixels_by_cells() {
    let cell = cell_size_from_window_metrics(1920, 1080, 240, 60);
    assert_eq!(cell, Some((8, 18)));
}

#[test]
fn cell_size_from_window_metrics_rejects_invalid_inputs() {
    assert_eq!(cell_size_from_window_metrics(0, 1080, 240, 60), None);
    assert_eq!(cell_size_from_window_metrics(1920, 1080, 0, 60), None);
    assert_eq!(cell_size_from_window_metrics(10, 10, 240, 60), None);
}
