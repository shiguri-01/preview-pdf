use std::collections::VecDeque;

use crate::config::Config;
use crate::config::{
    AppOptions, AppOptionsResolver, CachePolicy, EventLoopPolicy, InputPolicy, RenderPolicy,
    ResolvedAppOptions, load_default_app_options,
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
    pub(crate) event_loop_policy: EventLoopPolicy,
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

    pub fn options(mut self, options: AppOptions) -> Self {
        self.options = options;
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
        AppBuilder::new(presenter_kind).options(options).build()
    }

    fn from_resolved_options(
        presenter_kind: PresenterKind,
        options: ResolvedAppOptions,
        run_options: RunOptions,
    ) -> AppResult<Self> {
        let cache = options.cache;
        let presenter = create_presenter_with_cache_limits(
            presenter_kind,
            Some((cache.l2_max_entries, cache.l2_memory_budget_bytes())),
        )?;
        let mut state = AppState::default();
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
            event_loop_policy: options.event_loop,
            run_options,
        })
    }

    pub fn set_watch(&mut self, watch: bool) {
        self.run_options.watch = watch;
    }

    pub(crate) fn run_options(&self) -> RunOptions {
        self.run_options
    }
}

fn render_runtime_from_cache_policy(cache: CachePolicy) -> RenderRuntime {
    RenderRuntime::with_l1_cache_limits(cache.l1_max_entries, cache.l1_memory_budget_bytes())
}
