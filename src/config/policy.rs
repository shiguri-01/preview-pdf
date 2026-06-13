use std::time::Duration;

use crate::app::scale::{ZOOM_MAX, ZOOM_MIN};
use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
use crate::input::keymap::build_builtin_sequence_registry;
use crate::input::sequence::{DEFAULT_SEQUENCE_TIMEOUT, SequenceRegistry};

use super::options::AppOptions;
use super::types::{CacheConfig, Config, InputConfig, RenderConfig, ViewConfig, WatchConfig};

#[derive(Debug, Clone)]
pub struct ResolvedAppOptions {
    pub render: RenderPolicy,
    pub view: ViewPolicy,
    pub event_loop: EventLoopPolicy,
    pub cache: CachePolicy,
    pub input: InputPolicy,
    pub watch: WatchPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderPolicy {
    pub worker_threads: usize,
    pub max_render_scale: f32,
}

impl Default for RenderPolicy {
    fn default() -> Self {
        let render = RenderConfig::default();
        Self {
            worker_threads: render.worker_threads,
            max_render_scale: render.max_render_scale,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventLoopPolicy {
    pub input_poll_timeout_idle: Duration,
    pub input_poll_timeout_busy: Duration,
    pub prefetch_pause_after_input: Duration,
    pub prefetch_tick_interval: Duration,
    pub pending_redraw_interval: Duration,
    pub prefetch_dispatch_budget_per_tick: usize,
}

impl Default for EventLoopPolicy {
    fn default() -> Self {
        let render = RenderConfig::default();
        Self {
            input_poll_timeout_idle: Duration::from_millis(render.input_poll_timeout_idle_ms),
            input_poll_timeout_busy: Duration::from_millis(render.input_poll_timeout_busy_ms),
            prefetch_pause_after_input: Duration::from_millis(render.prefetch_pause_ms),
            prefetch_tick_interval: Duration::from_millis(render.prefetch_tick_ms),
            pending_redraw_interval: Duration::from_millis(render.pending_redraw_interval_ms),
            prefetch_dispatch_budget_per_tick: render.prefetch_dispatch_budget_per_tick,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePolicy {
    pub l1_memory_budget_mb: usize,
    pub l2_memory_budget_mb: usize,
    pub l1_max_entries: usize,
    pub l2_max_entries: usize,
}

impl Default for CachePolicy {
    fn default() -> Self {
        let cache = CacheConfig::default();
        Self {
            l1_memory_budget_mb: cache.l1_memory_budget_mb,
            l2_memory_budget_mb: cache.l2_memory_budget_mb,
            l1_max_entries: cache.l1_max_entries,
            l2_max_entries: cache.l2_max_entries,
        }
    }
}

impl CachePolicy {
    const MEBIBYTE: usize = 1024 * 1024;

    pub fn l1_memory_budget_bytes(&self) -> usize {
        self.l1_memory_budget_mb
            .saturating_mul(Self::MEBIBYTE)
            .max(1)
    }

    pub fn l2_memory_budget_bytes(&self) -> usize {
        self.l2_memory_budget_mb
            .saturating_mul(Self::MEBIBYTE)
            .max(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewPolicy {
    pub initial_page_index: usize,
    pub initial_zoom: f32,
    pub initial_layout: PageLayoutMode,
    pub spread_direction: SpreadDirection,
    pub spread_cover: SpreadCoverPolicy,
}

impl Default for ViewPolicy {
    fn default() -> Self {
        let view = ViewConfig::default();
        Self {
            initial_page_index: view.initial_page - 1,
            initial_zoom: view.initial_zoom,
            initial_layout: view.initial_layout,
            spread_direction: view.spread_direction,
            spread_cover: view.spread_cover,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InputPolicy {
    pub sequence_timeout: Duration,
    pub sequence_registry: SequenceRegistry,
}

impl Default for InputPolicy {
    fn default() -> Self {
        Self {
            sequence_timeout: DEFAULT_SEQUENCE_TIMEOUT,
            sequence_registry: build_builtin_sequence_registry(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchPolicy {
    pub enabled: bool,
    pub poll_interval: Duration,
    pub settle_delay: Duration,
}

impl Default for WatchPolicy {
    fn default() -> Self {
        let watch = WatchConfig::default();
        Self {
            enabled: watch.enabled,
            poll_interval: Duration::from_millis(watch.poll_interval_ms),
            settle_delay: Duration::from_millis(watch.settle_delay_ms),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppOptionsResolver {
    options: AppOptions,
}

impl AppOptionsResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_options(mut self, options: AppOptions) -> Self {
        self.options = self.options.merge(options);
        self
    }

    pub fn resolve(self) -> ResolvedAppOptions {
        resolve_options(self.options)
    }
}

impl Default for ResolvedAppOptions {
    fn default() -> Self {
        AppOptionsResolver::new().resolve()
    }
}

impl From<ResolvedAppOptions> for Config {
    fn from(options: ResolvedAppOptions) -> Self {
        Self {
            render: RenderConfig {
                worker_threads: options.render.worker_threads,
                input_poll_timeout_idle_ms: options.event_loop.input_poll_timeout_idle.as_millis()
                    as u64,
                input_poll_timeout_busy_ms: options.event_loop.input_poll_timeout_busy.as_millis()
                    as u64,
                prefetch_pause_ms: options.event_loop.prefetch_pause_after_input.as_millis() as u64,
                prefetch_tick_ms: options.event_loop.prefetch_tick_interval.as_millis() as u64,
                pending_redraw_interval_ms: options.event_loop.pending_redraw_interval.as_millis()
                    as u64,
                prefetch_dispatch_budget_per_tick: options
                    .event_loop
                    .prefetch_dispatch_budget_per_tick,
                max_render_scale: options.render.max_render_scale,
            },
            cache: CacheConfig {
                l1_memory_budget_mb: options.cache.l1_memory_budget_mb,
                l2_memory_budget_mb: options.cache.l2_memory_budget_mb,
                l1_max_entries: options.cache.l1_max_entries,
                l2_max_entries: options.cache.l2_max_entries,
            },
            view: ViewConfig {
                initial_page: options.view.initial_page_index + 1,
                initial_zoom: options.view.initial_zoom,
                initial_layout: options.view.initial_layout,
                spread_direction: options.view.spread_direction,
                spread_cover: options.view.spread_cover,
            },
            input: InputConfig {
                sequence_timeout_ms: options.input.sequence_timeout.as_millis() as u64,
            },
            watch: WatchConfig {
                enabled: options.watch.enabled,
                poll_interval_ms: options.watch.poll_interval.as_millis() as u64,
                settle_delay_ms: options.watch.settle_delay.as_millis() as u64,
            },
        }
    }
}

fn resolve_options(options: AppOptions) -> ResolvedAppOptions {
    let render_defaults = RenderConfig::default();
    let cache_defaults = CacheConfig::default();
    let view_defaults = ViewConfig::default();
    let watch_defaults = WatchConfig::default();

    let worker_threads = options
        .render
        .worker_threads
        .unwrap_or(render_defaults.worker_threads)
        .max(1);
    let input_poll_timeout_idle_ms = options
        .render
        .input_poll_timeout_idle_ms
        .unwrap_or(render_defaults.input_poll_timeout_idle_ms)
        .max(1);
    let input_poll_timeout_busy_ms = options
        .render
        .input_poll_timeout_busy_ms
        .unwrap_or(render_defaults.input_poll_timeout_busy_ms)
        .max(1);
    let prefetch_pause_ms = options
        .render
        .prefetch_pause_ms
        .unwrap_or(render_defaults.prefetch_pause_ms)
        .max(1);
    let prefetch_tick_ms = options
        .render
        .prefetch_tick_ms
        .unwrap_or(render_defaults.prefetch_tick_ms)
        .max(1);
    let pending_redraw_interval_ms = options
        .render
        .pending_redraw_interval_ms
        .unwrap_or(render_defaults.pending_redraw_interval_ms)
        .max(1);
    let prefetch_dispatch_budget_per_tick = options
        .render
        .prefetch_dispatch_budget_per_tick
        .unwrap_or(render_defaults.prefetch_dispatch_budget_per_tick)
        .max(1);
    let mut max_render_scale = options
        .render
        .max_render_scale
        .unwrap_or(render_defaults.max_render_scale);
    if !max_render_scale.is_finite() || max_render_scale < 1.0 {
        max_render_scale = render_defaults.max_render_scale;
    }
    let sequence_timeout_ms = options
        .input
        .sequence_timeout_ms
        .unwrap_or(DEFAULT_SEQUENCE_TIMEOUT.as_millis() as u64)
        .max(1);
    let initial_page_index = options
        .view
        .initial_page
        .unwrap_or(view_defaults.initial_page)
        .max(1)
        - 1;
    let mut initial_zoom = options
        .view
        .initial_zoom
        .unwrap_or(view_defaults.initial_zoom);
    if !initial_zoom.is_finite() || initial_zoom <= 0.0 {
        initial_zoom = view_defaults.initial_zoom;
    }
    initial_zoom = initial_zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    let watch_poll_interval_ms = options
        .watch
        .poll_interval_ms
        .unwrap_or(watch_defaults.poll_interval_ms)
        .max(1);
    let watch_settle_delay_ms = options
        .watch
        .settle_delay_ms
        .unwrap_or(watch_defaults.settle_delay_ms)
        .max(1);

    ResolvedAppOptions {
        render: RenderPolicy {
            worker_threads,
            max_render_scale,
        },
        view: ViewPolicy {
            initial_page_index,
            initial_zoom,
            initial_layout: options
                .view
                .initial_layout
                .unwrap_or(view_defaults.initial_layout),
            spread_direction: options
                .view
                .spread_direction
                .unwrap_or(view_defaults.spread_direction),
            spread_cover: options
                .view
                .spread_cover
                .unwrap_or(view_defaults.spread_cover),
        },
        event_loop: EventLoopPolicy {
            input_poll_timeout_idle: Duration::from_millis(input_poll_timeout_idle_ms),
            input_poll_timeout_busy: Duration::from_millis(input_poll_timeout_busy_ms),
            prefetch_pause_after_input: Duration::from_millis(prefetch_pause_ms),
            prefetch_tick_interval: Duration::from_millis(prefetch_tick_ms),
            pending_redraw_interval: Duration::from_millis(pending_redraw_interval_ms),
            prefetch_dispatch_budget_per_tick,
        },
        cache: CachePolicy {
            l1_memory_budget_mb: options
                .cache
                .l1_memory_budget_mb
                .unwrap_or(cache_defaults.l1_memory_budget_mb),
            l2_memory_budget_mb: options
                .cache
                .l2_memory_budget_mb
                .unwrap_or(cache_defaults.l2_memory_budget_mb),
            l1_max_entries: options
                .cache
                .l1_max_entries
                .unwrap_or(cache_defaults.l1_max_entries),
            l2_max_entries: options
                .cache
                .l2_max_entries
                .unwrap_or(cache_defaults.l2_max_entries),
        },
        input: InputPolicy {
            sequence_timeout: Duration::from_millis(sequence_timeout_ms),
            sequence_registry: super::keymap::resolve_sequence_registry(&options.keymap),
        },
        watch: WatchPolicy {
            enabled: options.watch.enabled.unwrap_or(watch_defaults.enabled),
            poll_interval: Duration::from_millis(watch_poll_interval_ms),
            settle_delay: Duration::from_millis(watch_settle_delay_ms),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};

    use crate::config::{AppOptions, RenderOptions, ViewOptions, WatchOptions};

    use super::AppOptionsResolver;

    #[test]
    fn resolver_applies_defaults_and_sanitizes_without_file_source() {
        let options = AppOptions {
            render: RenderOptions {
                worker_threads: Some(0),
                input_poll_timeout_idle_ms: Some(0),
                input_poll_timeout_busy_ms: Some(0),
                prefetch_pause_ms: Some(0),
                prefetch_tick_ms: Some(0),
                pending_redraw_interval_ms: Some(0),
                prefetch_dispatch_budget_per_tick: Some(0),
                max_render_scale: Some(0.5),
            },
            view: ViewOptions {
                initial_page: Some(0),
                initial_zoom: Some(10.0),
                initial_layout: Some(PageLayoutMode::Spread),
                spread_direction: Some(SpreadDirection::Rtl),
                spread_cover: Some(SpreadCoverPolicy::Cover),
            },
            watch: WatchOptions {
                enabled: Some(true),
                poll_interval_ms: Some(0),
                settle_delay_ms: Some(0),
            },
            ..AppOptions::default()
        };

        let resolved = AppOptionsResolver::new().apply_options(options).resolve();

        assert_eq!(resolved.render.worker_threads, 1);
        assert_eq!(
            resolved.event_loop.input_poll_timeout_idle,
            Duration::from_millis(1)
        );
        assert_eq!(
            resolved.event_loop.input_poll_timeout_busy,
            Duration::from_millis(1)
        );
        assert_eq!(
            resolved.event_loop.prefetch_pause_after_input,
            Duration::from_millis(1)
        );
        assert_eq!(
            resolved.event_loop.prefetch_tick_interval,
            Duration::from_millis(1)
        );
        assert_eq!(
            resolved.event_loop.pending_redraw_interval,
            Duration::from_millis(1)
        );
        assert_eq!(resolved.event_loop.prefetch_dispatch_budget_per_tick, 1);
        assert_eq!(resolved.render.max_render_scale, 2.5);
        assert_eq!(resolved.view.initial_page_index, 0);
        assert_eq!(resolved.view.initial_zoom, 4.0);
        assert_eq!(resolved.view.initial_layout, PageLayoutMode::Spread);
        assert_eq!(resolved.view.spread_direction, SpreadDirection::Rtl);
        assert_eq!(resolved.view.spread_cover, SpreadCoverPolicy::Cover);
        assert!(resolved.watch.enabled);
        assert_eq!(resolved.watch.poll_interval, Duration::from_millis(1));
        assert_eq!(resolved.watch.settle_delay, Duration::from_millis(1));
    }

    #[test]
    fn resolver_merges_later_options_over_earlier_options() {
        let base = AppOptions {
            render: RenderOptions {
                worker_threads: Some(2),
                ..RenderOptions::default()
            },
            ..AppOptions::default()
        };
        let override_options = AppOptions {
            render: RenderOptions {
                worker_threads: Some(4),
                ..RenderOptions::default()
            },
            watch: WatchOptions {
                enabled: Some(false),
                ..WatchOptions::default()
            },
            ..AppOptions::default()
        };
        let base = base.merge(AppOptions {
            watch: WatchOptions {
                enabled: Some(true),
                ..WatchOptions::default()
            },
            ..AppOptions::default()
        });

        let resolved = AppOptionsResolver::new()
            .apply_options(base)
            .apply_options(override_options)
            .resolve();

        assert_eq!(resolved.render.worker_threads, 4);
        assert!(!resolved.watch.enabled);
    }
}
