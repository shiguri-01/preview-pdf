use std::time::Duration;

use figment::{Figment, providers::Serialized};

use crate::app::scale::{ZOOM_MAX, ZOOM_MIN};
use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
use crate::error::{AppError, AppResult};
use crate::input::sequence::{DEFAULT_SEQUENCE_TIMEOUT, SequenceRegistry};

use super::keymap::{KeymapOptions, build_default_sequence_registry};
use super::options::AppOptions;

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
    pub graphics_protocol: Option<crate::presenter::GraphicsProtocol>,
    pub worker_threads: usize,
    pub max_render_scale: f32,
}

impl Default for RenderPolicy {
    fn default() -> Self {
        Self {
            graphics_protocol: None,
            worker_threads: 3,
            max_render_scale: 2.5,
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
        Self {
            input_poll_timeout_idle: Duration::from_millis(16),
            input_poll_timeout_busy: Duration::from_millis(8),
            prefetch_pause_after_input: Duration::from_millis(120),
            prefetch_tick_interval: Duration::from_millis(8),
            pending_redraw_interval: Duration::from_millis(33),
            prefetch_dispatch_budget_per_tick: 6,
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
        Self {
            l1_memory_budget_mb: 512,
            l2_memory_budget_mb: 64,
            l1_max_entries: 128,
            l2_max_entries: 96,
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
        Self {
            initial_page_index: 0,
            initial_zoom: 1.0,
            initial_layout: PageLayoutMode::Single,
            spread_direction: SpreadDirection::Ltr,
            spread_cover: SpreadCoverPolicy::Paired,
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
            sequence_registry: build_default_sequence_registry(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchPolicy {
    pub enabled: bool,
    pub settle_delay: Duration,
}

impl Default for WatchPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            settle_delay: Duration::from_millis(500),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppOptionsResolver {
    sources: Figment,
    keymap: KeymapOptions,
}

impl AppOptionsResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_options(mut self, mut options: AppOptions) -> Self {
        self.keymap = self.keymap.merge(std::mem::take(&mut options.keymap));
        self.sources = self.sources.merge(Serialized::defaults(options));
        self
    }

    pub fn resolve(self) -> AppResult<ResolvedAppOptions> {
        let mut options: AppOptions = self.sources.extract().map_err(|source| {
            AppError::invalid_argument(format!("resolving configuration: {source}"))
        })?;
        options.keymap = self.keymap;
        Ok(resolve_options(options))
    }
}

impl Default for ResolvedAppOptions {
    fn default() -> Self {
        resolve_options(AppOptions::default())
    }
}

fn resolve_options(options: AppOptions) -> ResolvedAppOptions {
    let render_defaults = RenderPolicy::default();
    let cache_defaults = CachePolicy::default();
    let view_defaults = ViewPolicy::default();
    let watch_defaults = WatchPolicy::default();

    let worker_threads = options
        .render
        .worker_threads
        .unwrap_or(render_defaults.worker_threads)
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
        .unwrap_or(view_defaults.initial_page_index + 1)
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
    let watch_settle_delay_ms = options
        .watch
        .settle_delay_ms
        .unwrap_or(watch_defaults.settle_delay.as_millis() as u64)
        .max(1);

    ResolvedAppOptions {
        render: RenderPolicy {
            graphics_protocol: options
                .render
                .graphics_protocol
                .or(render_defaults.graphics_protocol),
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
        event_loop: EventLoopPolicy::default(),
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
            settle_delay: Duration::from_millis(watch_settle_delay_ms),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
    use crate::input::sequence::DEFAULT_SEQUENCE_TIMEOUT;

    use crate::config::{AppOptions, CacheOptions, RenderOptions, ViewOptions, WatchOptions};
    use crate::presenter::GraphicsProtocol;

    use super::AppOptionsResolver;

    #[test]
    fn no_option_sources_resolve_to_builtin_defaults() {
        let resolved = AppOptionsResolver::new()
            .resolve()
            .expect("no sources should resolve");

        assert_eq!(resolved.render, super::RenderPolicy::default());
        assert_eq!(resolved.view, super::ViewPolicy::default());
        assert_eq!(resolved.event_loop, super::EventLoopPolicy::default());
        assert_eq!(resolved.cache, super::CachePolicy::default());
        assert_eq!(resolved.watch, super::WatchPolicy::default());
        assert_eq!(resolved.input.sequence_timeout, DEFAULT_SEQUENCE_TIMEOUT);
    }

    #[test]
    fn absent_options_preserve_earlier_values_but_explicit_zero_false_and_auto_override() {
        let resolved = AppOptionsResolver::new()
            .apply_options(AppOptions {
                render: RenderOptions {
                    graphics_protocol: Some(GraphicsProtocol::Kitty),
                    worker_threads: Some(7),
                    ..RenderOptions::default()
                },
                cache: CacheOptions {
                    l1_max_entries: Some(42),
                    ..CacheOptions::default()
                },
                watch: WatchOptions {
                    enabled: Some(true),
                    settle_delay_ms: Some(300),
                },
                ..AppOptions::default()
            })
            .apply_options(AppOptions {
                render: RenderOptions {
                    graphics_protocol: Some(GraphicsProtocol::Auto),
                    worker_threads: Some(4),
                    ..RenderOptions::default()
                },
                watch: WatchOptions {
                    enabled: Some(false),
                    settle_delay_ms: Some(0),
                },
                ..AppOptions::default()
            })
            .apply_options(AppOptions::default())
            .resolve()
            .expect("layered options should resolve");

        assert_eq!(
            resolved.render.graphics_protocol,
            Some(GraphicsProtocol::Auto)
        );
        assert_eq!(resolved.render.worker_threads, 4);
        assert_eq!(resolved.cache.l1_max_entries, 42);
        assert!(!resolved.watch.enabled);
        assert_eq!(resolved.watch.settle_delay, Duration::from_millis(1));
    }

    #[test]
    fn non_finite_values_reach_policy_sanitization() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let resolved = AppOptionsResolver::new()
                .apply_options(AppOptions {
                    render: RenderOptions {
                        max_render_scale: Some(value),
                        ..RenderOptions::default()
                    },
                    view: ViewOptions {
                        initial_zoom: Some(value),
                        ..ViewOptions::default()
                    },
                    ..AppOptions::default()
                })
                .resolve()
                .expect("non-finite options should resolve before sanitization");

            assert_eq!(resolved.render.max_render_scale, 2.5);
            assert_eq!(resolved.view.initial_zoom, 1.0);
        }
    }

    #[test]
    fn keymap_layers_preserve_unrelated_bindings_and_apply_later_overrides_and_unbinds() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        use crate::command::Command;
        use crate::config::keymap::{KeymapOptions, KeymapPreset, parse_keymap_binding};
        use crate::extension::ExtensionUiSnapshot;
        use crate::input::sequence::{KeyBindingContext, SequenceResolution, SequenceResolver};

        let binding = |key, command| {
            parse_keymap_binding("normal", key, command).expect("binding should be valid")
        };
        let resolved = AppOptionsResolver::new()
            .apply_options(AppOptions {
                keymap: KeymapOptions {
                    preset: Some(KeymapPreset::None),
                    bindings: vec![
                        binding("x", Some("next-page")),
                        binding("y", Some("next-page")),
                        binding("z", Some("next-page")),
                    ],
                },
                ..AppOptions::default()
            })
            .apply_options(AppOptions {
                keymap: KeymapOptions {
                    preset: None,
                    bindings: vec![binding("x", Some("prev-page")), binding("z", None)],
                },
                ..AppOptions::default()
            })
            .apply_options(AppOptions::default())
            .resolve()
            .expect("keymap layers should resolve");
        let mut resolver = SequenceResolver::new(
            resolved.input.sequence_registry,
            resolved.input.sequence_timeout,
        );
        let extensions = ExtensionUiSnapshot::default();
        for (key, expected) in [
            ('x', SequenceResolution::Dispatch(Command::PrevPage)),
            ('y', SequenceResolution::Dispatch(Command::NextPage)),
            ('z', SequenceResolution::Noop),
            ('j', SequenceResolution::Noop),
        ] {
            assert_eq!(
                resolver.handle_key_in_context(
                    KeyBindingContext::normal(&extensions),
                    KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE),
                ),
                expected,
                "binding for {key}"
            );
        }
    }

    #[test]
    fn resolver_applies_defaults_and_sanitizes_without_file_source() {
        let options = AppOptions {
            render: RenderOptions {
                graphics_protocol: None,
                worker_threads: Some(0),
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
                settle_delay_ms: Some(0),
            },
            ..AppOptions::default()
        };

        let resolved = AppOptionsResolver::new()
            .apply_options(options)
            .resolve()
            .expect("options should resolve");

        assert_eq!(resolved.render.worker_threads, 1);
        assert_eq!(resolved.event_loop, super::EventLoopPolicy::default());
        assert_eq!(resolved.render.max_render_scale, 2.5);
        assert_eq!(resolved.render.graphics_protocol, None);
        assert_eq!(resolved.view.initial_page_index, 0);
        assert_eq!(resolved.view.initial_zoom, 4.0);
        assert_eq!(resolved.view.initial_layout, PageLayoutMode::Spread);
        assert_eq!(resolved.view.spread_direction, SpreadDirection::Rtl);
        assert_eq!(resolved.view.spread_cover, SpreadCoverPolicy::Cover);
        assert!(resolved.watch.enabled);
        assert_eq!(resolved.watch.settle_delay, Duration::from_millis(1));
    }
}
