use std::collections::VecDeque;

use crate::config::Config;
use crate::config::{
    AppOptions, AppOptionsResolver, CachePolicy, EventLoopPolicy, InputPolicy, RenderPolicy,
    ResolvedAppOptions, ViewPolicy, WatchPolicy, load_default_app_options,
};
use crate::error::AppResult;
use crate::extension::ExtensionHost;
use crate::input::InputHistoryService;
use crate::input::keymap::build_builtin_sequence_registry;
use crate::input::sequence::{DEFAULT_SEQUENCE_TIMEOUT, SequenceRegistry, SequenceResolver};
use crate::palette::{PaletteManager, PaletteRegistry};
use crate::presenter::{ImagePresenter, PresenterKind, create_presenter_with_cache_limits};

use super::runtime::RenderRuntime;
use super::state::{AppState, CacheHandle, PaletteRequest};

pub struct RenderSubsystem {
    pub presenter: Box<dyn ImagePresenter>,
    pub runtime: RenderRuntime,
    pub viewer_has_image: bool,
    pub image_occluded_last_frame: bool,
}

impl RenderSubsystem {
    pub(crate) fn new(presenter: Box<dyn ImagePresenter>, runtime: RenderRuntime) -> Self {
        Self {
            presenter,
            runtime,
            viewer_has_image: false,
            image_occluded_last_frame: false,
        }
    }
}

#[derive(Default)]
pub struct ExtensionSubsystem {
    pub host: ExtensionHost,
}

#[derive(Default)]
pub struct PaletteSubsystem {
    pub registry: PaletteRegistry,
    pub manager: PaletteManager,
    pub pending_requests: VecDeque<PaletteRequest>,
}

pub struct SequenceSubsystem {
    pub resolver: SequenceResolver,
}

pub struct InteractionSubsystem {
    pub extensions: ExtensionSubsystem,
    pub palette: PaletteSubsystem,
    pub history: InputHistoryService,
    pub sequences: SequenceSubsystem,
}

impl Default for InteractionSubsystem {
    fn default() -> Self {
        Self::with_sequence_registry(build_builtin_sequence_registry())
    }
}

impl InteractionSubsystem {
    pub(crate) fn with_input_policy(policy: InputPolicy) -> Self {
        Self {
            extensions: ExtensionSubsystem::default(),
            palette: PaletteSubsystem::default(),
            history: InputHistoryService::default(),
            sequences: SequenceSubsystem {
                resolver: SequenceResolver::new(policy.sequence_registry, policy.sequence_timeout),
            },
        }
    }

    pub(crate) fn with_sequence_registry(registry: SequenceRegistry) -> Self {
        Self {
            extensions: ExtensionSubsystem::default(),
            palette: PaletteSubsystem::default(),
            history: InputHistoryService::default(),
            sequences: SequenceSubsystem {
                resolver: SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn with_sequence_registry_and_timeout(
        registry: SequenceRegistry,
        timeout: std::time::Duration,
    ) -> Self {
        Self {
            extensions: ExtensionSubsystem::default(),
            palette: PaletteSubsystem::default(),
            history: InputHistoryService::default(),
            sequences: SequenceSubsystem {
                resolver: SequenceResolver::new(registry, timeout),
            },
        }
    }
}

pub struct App {
    pub state: AppState,
    pub render: RenderSubsystem,
    pub interaction: InteractionSubsystem,
    pub(crate) render_policy: RenderPolicy,
    pub(crate) view_policy: ViewPolicy,
    pub(crate) event_loop_policy: EventLoopPolicy,
    pub(crate) watch_policy: WatchPolicy,
    run_options: RunOptions,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunOptions {
    pub watch: bool,
}

pub struct AppBuilder {
    presenter_kind: PresenterKind,
    options: AppOptions,
    run_options: RunOptions,
}

impl AppBuilder {
    pub fn new(presenter_kind: PresenterKind) -> Self {
        Self {
            presenter_kind,
            options: AppOptions::default(),
            run_options: RunOptions::default(),
        }
    }

    pub fn replace_options(mut self, options: AppOptions) -> Self {
        self.options = options;
        self
    }

    pub fn merge_options(mut self, options: AppOptions) -> Self {
        self.options = self.options.merge(options);
        self
    }

    pub fn run_options(mut self, run_options: RunOptions) -> Self {
        self.run_options = run_options;
        self
    }

    pub fn build(self) -> AppResult<App> {
        let resolved = AppOptionsResolver::new()
            .apply_options(self.options)
            .resolve();
        App::from_resolved_options(self.presenter_kind, resolved, self.run_options)
    }
}

impl App {
    pub fn new(presenter_kind: PresenterKind) -> AppResult<Self> {
        let options = load_default_app_options()?;
        Self::new_with_options(presenter_kind, options)
    }

    pub fn new_with_config(presenter_kind: PresenterKind, config: Config) -> AppResult<Self> {
        Self::new_with_options(presenter_kind, AppOptions::from(config))
    }

    pub fn new_with_options(presenter_kind: PresenterKind, options: AppOptions) -> AppResult<Self> {
        AppBuilder::new(presenter_kind)
            .replace_options(options)
            .build()
    }

    fn from_resolved_options(
        presenter_kind: PresenterKind,
        options: ResolvedAppOptions,
        run_options: RunOptions,
    ) -> AppResult<Self> {
        let cache = options.cache;
        let view = options.view;
        let watch = options.watch;
        let presenter = create_presenter_with_cache_limits(
            presenter_kind,
            Some((cache.l2_max_entries, cache.l2_memory_budget_bytes())),
        )?;
        let mut state = AppState {
            current_page: view.initial_page_index,
            page_layout_mode: view.initial_layout,
            spread_direction: view.spread_direction,
            spread_cover_policy: view.spread_cover,
            zoom: view.initial_zoom,
            ..AppState::default()
        };
        state.caches.l1_rendered_pages = Some(CacheHandle {
            name: "l1-rendered-pages",
        });
        if presenter.capabilities().supports_l2_cache {
            state.caches.l2_terminal_frames = Some(CacheHandle {
                name: "l2-terminal-frames",
            });
        }

        Ok(Self {
            state,
            render: RenderSubsystem::new(presenter, render_runtime_from_cache_policy(cache)),
            interaction: InteractionSubsystem::with_input_policy(options.input),
            render_policy: options.render,
            view_policy: view,
            event_loop_policy: options.event_loop,
            watch_policy: watch,
            run_options: RunOptions {
                watch: run_options.watch || watch.enabled,
            },
        })
    }

    pub fn set_watch(&mut self, watch: bool) {
        self.run_options.watch = watch;
        self.watch_policy.enabled = watch;
    }

    pub(crate) fn run_options(&self) -> RunOptions {
        self.run_options
    }
}

fn render_runtime_from_cache_policy(cache: CachePolicy) -> RenderRuntime {
    RenderRuntime::with_l1_cache_limits(cache.l1_max_entries, cache.l1_memory_budget_bytes())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
    use crate::config::{CacheOptions, InputOptions, RenderOptions, ViewOptions, WatchOptions};
    use crate::presenter::PresenterKind;

    use super::{App, AppBuilder};
    use crate::config::{AppOptions, Config};

    #[test]
    fn new_with_config_applies_l1_cache_limits() {
        let mut config = Config::default();
        config.cache.l1_max_entries = 7;
        config.cache.l1_memory_budget_mb = 2;

        let app =
            App::new_with_config(PresenterKind::RatatuiImage, config.clone()).expect("app init");

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

        let mut app =
            App::new_with_options(PresenterKind::RatatuiImage, options).expect("app init");
        assert!(app.run_options().watch);
        assert!(app.watch_policy.enabled);

        app.set_watch(false);

        assert!(!app.run_options().watch);
        assert!(!app.watch_policy.enabled);
    }
}
