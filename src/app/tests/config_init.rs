use std::time::Duration;

use crate::app::{App, AppBuilder, PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
use crate::config::{
    AppOptions, CacheOptions, Config, InputOptions, RenderOptions, ViewOptions, WatchOptions,
};
use crate::presenter::PresenterKind;

#[test]
fn new_with_config_applies_l1_cache_limits() {
    let mut config = Config::default();
    config.cache.l1_max_entries = 7;
    config.cache.l1_memory_budget_mb = 2;

    let app = App::new_with_config(PresenterKind::RatatuiImage, config.clone()).expect("app init");

    assert_eq!(app.render.runtime.l1_cache.max_entries(), 7);
    assert_eq!(
        app.render.runtime.l1_cache.memory_budget_bytes(),
        config.cache.l1_memory_budget_bytes()
    );
}

#[test]
fn new_with_options_applies_l1_cache_limits_without_file_config() {
    let options = AppOptions {
        cache: CacheOptions {
            l1_max_entries: Some(9),
            l1_memory_budget_mb: Some(3),
            ..CacheOptions::default()
        },
        ..AppOptions::default()
    };

    let app = App::new_with_options(PresenterKind::RatatuiImage, options).expect("app init");

    assert_eq!(app.render.runtime.l1_cache.max_entries(), 9);
    assert_eq!(
        app.render.runtime.l1_cache.memory_budget_bytes(),
        3 * 1024 * 1024
    );
}

#[test]
fn app_builder_applies_multiple_option_patches() {
    let cache_options = AppOptions {
        cache: CacheOptions {
            l1_max_entries: Some(11),
            ..CacheOptions::default()
        },
        ..AppOptions::default()
    };
    let render_options = AppOptions {
        render: RenderOptions {
            worker_threads: Some(2),
            ..RenderOptions::default()
        },
        ..AppOptions::default()
    };

    let app = AppBuilder::new(PresenterKind::RatatuiImage)
        .merge_options(cache_options)
        .merge_options(render_options)
        .build()
        .expect("app init");

    assert_eq!(app.render.runtime.l1_cache.max_entries(), 11);
    assert_eq!(app.render_policy.worker_threads, 2);
}

#[test]
fn new_with_options_threads_runtime_policies_to_owners() {
    let options = AppOptions {
        render: RenderOptions {
            worker_threads: Some(5),
            input_poll_timeout_idle_ms: Some(17),
            input_poll_timeout_busy_ms: Some(9),
            prefetch_pause_ms: Some(130),
            prefetch_tick_ms: Some(11),
            pending_redraw_interval_ms: Some(41),
            prefetch_dispatch_budget_per_tick: Some(8),
            max_render_scale: Some(3.0),
        },
        input: InputOptions {
            sequence_timeout_ms: Some(250),
        },
        view: ViewOptions {
            initial_page: Some(3),
            initial_zoom: Some(1.25),
            initial_layout: Some(PageLayoutMode::Spread),
            spread_direction: Some(SpreadDirection::Rtl),
            spread_cover: Some(SpreadCoverPolicy::Cover),
        },
        watch: WatchOptions {
            enabled: Some(true),
            poll_interval_ms: Some(125),
            settle_delay_ms: Some(375),
        },
        ..AppOptions::default()
    };

    let app = App::new_with_options(PresenterKind::RatatuiImage, options).expect("app init");

    assert_eq!(app.render_policy.worker_threads, 5);
    assert_eq!(app.render_policy.max_render_scale, 3.0);
    assert_eq!(
        app.event_loop_policy.input_poll_timeout_idle,
        Duration::from_millis(17)
    );
    assert_eq!(
        app.event_loop_policy.input_poll_timeout_busy,
        Duration::from_millis(9)
    );
    assert_eq!(
        app.event_loop_policy.prefetch_pause_after_input,
        Duration::from_millis(130)
    );
    assert_eq!(
        app.event_loop_policy.prefetch_tick_interval,
        Duration::from_millis(11)
    );
    assert_eq!(
        app.event_loop_policy.pending_redraw_interval,
        Duration::from_millis(41)
    );
    assert_eq!(app.event_loop_policy.prefetch_dispatch_budget_per_tick, 8);
    assert_eq!(
        app.interaction.sequences.resolver.timeout(),
        Duration::from_millis(250)
    );
    assert_eq!(app.state.current_page, 2);
    assert_eq!(app.state.zoom, 1.25);
    assert_eq!(app.state.page_layout_mode, PageLayoutMode::Spread);
    assert_eq!(app.state.spread_direction, SpreadDirection::Rtl);
    assert_eq!(app.state.spread_cover_policy, SpreadCoverPolicy::Cover);
    assert!(app.run_options().watch);
    assert!(app.watch_policy.enabled);
    assert_eq!(app.watch_policy.poll_interval, Duration::from_millis(125));
    assert_eq!(app.watch_policy.settle_delay, Duration::from_millis(375));
}

#[test]
fn set_watch_overrides_configured_watch_enabled() {
    let options = AppOptions {
        watch: WatchOptions {
            enabled: Some(true),
            ..WatchOptions::default()
        },
        ..AppOptions::default()
    };

    let mut app = App::new_with_options(PresenterKind::RatatuiImage, options).expect("app init");
    assert!(app.run_options().watch);
    assert!(app.watch_policy.enabled);

    app.set_watch(false);

    assert!(!app.run_options().watch);
    assert!(!app.watch_policy.enabled);
}
