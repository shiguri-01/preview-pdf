use std::collections::VecDeque;

use crate::config::keymap::build_default_sequence_registry;
use crate::config::{
    AppOptions, AppOptionsResolver, EventLoopPolicy, InputPolicy, RenderPolicy, ResolvedAppOptions,
    ViewPolicy, WatchPolicy, load_default_app_options,
};
use crate::error::AppResult;
use crate::extension::ExtensionHost;
use crate::input::InputHistoryService;
use crate::input::sequence::{DEFAULT_SEQUENCE_TIMEOUT, SequenceRegistry, SequenceResolver};
use crate::palette::PaletteSessionController;
use crate::presenter::{ImagePresenter, RatatuiImagePresenter};

use super::render_runtime::RenderRuntime;
use super::state::{AppState, PaletteRequest};

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
pub struct PaletteSubsystem {
    pub session: PaletteSessionController,
    pub pending_requests: VecDeque<PaletteRequest>,
}

pub struct InteractionSubsystem {
    pub extensions: ExtensionHost,
    pub palette: PaletteSubsystem,
    pub history: InputHistoryService,
    pub sequences: SequenceResolver,
}

impl Default for InteractionSubsystem {
    fn default() -> Self {
        Self::with_sequence_registry(build_default_sequence_registry())
    }
}

impl InteractionSubsystem {
    pub(crate) fn with_input_policy(policy: InputPolicy) -> Self {
        Self::with_sequence_resolver(SequenceResolver::new(
            policy.sequence_registry,
            policy.sequence_timeout,
        ))
    }

    pub(crate) fn with_sequence_registry(registry: SequenceRegistry) -> Self {
        Self::with_sequence_resolver(SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT))
    }

    fn with_sequence_resolver(resolver: SequenceResolver) -> Self {
        Self {
            extensions: ExtensionHost::default(),
            palette: PaletteSubsystem::default(),
            history: InputHistoryService::default(),
            sequences: resolver,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_sequence_registry_and_timeout(
        registry: SequenceRegistry,
        timeout: std::time::Duration,
    ) -> Self {
        Self::with_sequence_resolver(SequenceResolver::new(registry, timeout))
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunOptions {
    pub watch: bool,
}

#[derive(Default)]
pub struct AppBuilder {
    options: AppOptionsResolver,
}

impl AppBuilder {
    pub fn new() -> Self {
        Self {
            options: AppOptionsResolver::new(),
        }
    }

    pub fn replace_options(mut self, options: AppOptions) -> Self {
        self.options = AppOptionsResolver::new().apply_options(options);
        self
    }

    pub fn merge_options(mut self, options: AppOptions) -> Self {
        self.options = self.options.apply_options(options);
        self
    }

    pub fn build(self) -> AppResult<App> {
        let resolved = self.options.resolve()?;
        App::from_resolved_options(resolved)
    }
}

impl App {
    pub fn new() -> AppResult<Self> {
        let options = load_default_app_options()?;
        Self::new_with_options(options)
    }

    pub fn new_with_options(options: AppOptions) -> AppResult<Self> {
        AppBuilder::new().replace_options(options).build()
    }

    fn from_resolved_options(options: ResolvedAppOptions) -> AppResult<Self> {
        let cache = options.cache;
        let view = options.view;
        let presenter = Box::new(
            RatatuiImagePresenter::with_cache_limits_and_graphics_protocol(
                cache.l2_max_entries,
                cache.l2_memory_budget_bytes(),
                options.render.graphics_protocol,
            ),
        );
        let state = AppState {
            current_page: view.initial_page_index,
            page_layout_mode: view.initial_layout,
            spread_direction: view.spread_direction,
            spread_cover_policy: view.spread_cover,
            zoom: view.initial_zoom,
            ..AppState::default()
        };
        Ok(Self {
            state,
            render: RenderSubsystem::new(
                presenter,
                RenderRuntime::with_l1_cache_limits(
                    cache.l1_max_entries,
                    cache.l1_memory_budget_bytes(),
                ),
            ),
            interaction: InteractionSubsystem::with_input_policy(options.input),
            render_policy: options.render,
            view_policy: view,
            event_loop_policy: options.event_loop,
            watch_policy: options.watch,
        })
    }

    pub(crate) fn enable_metrics_collection(&mut self) -> AppResult<()> {
        self.render.runtime.perf_stats.reset();
        self.render.presenter.initialize_headless_for_perf()?;
        self.render.runtime.perf_stats.enable_sample_collection();
        self.render.presenter.enable_perf_sample_collection();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::App;
    use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
    use crate::config::AppOptions;
    use crate::config::{RenderOptions, ViewOptions, WatchOptions};

    #[test]
    fn new_with_options_threads_runtime_policies_to_owners() {
        let options = AppOptions {
            render: RenderOptions {
                graphics_protocol: None,
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
            },
            ..AppOptions::default()
        };

        let app = App::new_with_options(options).expect("app init");

        assert_eq!(app.render_policy, crate::config::RenderPolicy::default());
        assert_eq!(
            app.event_loop_policy,
            crate::config::EventLoopPolicy::default()
        );
        assert_eq!(
            app.interaction.sequences.timeout(),
            Duration::from_millis(1000)
        );
        assert_eq!(app.state.current_page, 2);
        assert_eq!(app.state.zoom, 1.25);
        assert_eq!(app.state.page_layout_mode, PageLayoutMode::Spread);
        assert_eq!(app.state.spread_direction, SpreadDirection::Rtl);
        assert_eq!(app.state.spread_cover_policy, SpreadCoverPolicy::Cover);
        assert!(app.watch_policy.enabled);
        assert_eq!(app.watch_policy.settle_delay, Duration::from_millis(500));
    }
}
