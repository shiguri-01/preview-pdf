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
    let view_defaults = ViewPolicy::default();
    let watch_defaults = WatchPolicy::default();

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
    ResolvedAppOptions {
        render: RenderPolicy {
            graphics_protocol: options
                .render
                .graphics_protocol
                .or(render_defaults.graphics_protocol),
            worker_threads: render_defaults.worker_threads,
            max_render_scale: render_defaults.max_render_scale,
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
        input: InputPolicy {
            sequence_timeout: DEFAULT_SEQUENCE_TIMEOUT,
            sequence_registry: super::keymap::resolve_sequence_registry(&options.keymap),
        },
        watch: WatchPolicy {
            enabled: options.watch.enabled.unwrap_or(watch_defaults.enabled),
            settle_delay: watch_defaults.settle_delay,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
    use crate::input::sequence::DEFAULT_SEQUENCE_TIMEOUT;

    use crate::config::{AppOptions, RenderOptions, ViewOptions, WatchOptions};
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
        assert_eq!(resolved.watch, super::WatchPolicy::default());
        assert_eq!(resolved.input.sequence_timeout, DEFAULT_SEQUENCE_TIMEOUT);
    }

    #[test]
    fn option_layers_preserve_supported_values_and_apply_later_overrides() {
        let resolved = AppOptionsResolver::new()
            .apply_options(AppOptions {
                render: RenderOptions {
                    graphics_protocol: Some(GraphicsProtocol::Kitty),
                },
                watch: WatchOptions {
                    enabled: Some(true),
                },
                ..AppOptions::default()
            })
            .apply_options(AppOptions {
                render: RenderOptions {
                    graphics_protocol: Some(GraphicsProtocol::Auto),
                },
                watch: WatchOptions {
                    enabled: Some(false),
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
        assert!(!resolved.watch.enabled);
        assert_eq!(resolved.watch.settle_delay, Duration::from_millis(500));
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
            },
            ..AppOptions::default()
        };

        let resolved = AppOptionsResolver::new()
            .apply_options(options)
            .resolve()
            .expect("options should resolve");

        assert_eq!(resolved.render.worker_threads, 3);
        assert_eq!(resolved.event_loop, super::EventLoopPolicy::default());
        assert_eq!(resolved.render.max_render_scale, 2.5);
        assert_eq!(resolved.render.graphics_protocol, None);
        assert_eq!(resolved.view.initial_page_index, 0);
        assert_eq!(resolved.view.initial_zoom, 4.0);
        assert_eq!(resolved.view.initial_layout, PageLayoutMode::Spread);
        assert_eq!(resolved.view.spread_direction, SpreadDirection::Rtl);
        assert_eq!(resolved.view.spread_cover, SpreadCoverPolicy::Cover);
        assert!(resolved.watch.enabled);
        assert_eq!(resolved.watch.settle_delay, Duration::from_millis(500));
    }
}
