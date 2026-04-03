use std::collections::VecDeque;

use crate::config::Config;
use crate::error::AppResult;
use crate::extension::ExtensionHost;
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
}

impl RenderSubsystem {
    pub(crate) fn new(presenter: Box<dyn ImagePresenter>, runtime: RenderRuntime) -> Self {
        Self {
            presenter,
            runtime,
            viewer_has_image: false,
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
    pub sequences: SequenceSubsystem,
}

impl Default for InteractionSubsystem {
    fn default() -> Self {
        Self::with_sequence_registry(build_builtin_sequence_registry())
    }
}

impl InteractionSubsystem {
    pub(crate) fn with_sequence_registry(registry: SequenceRegistry) -> Self {
        Self {
            extensions: ExtensionSubsystem::default(),
            palette: PaletteSubsystem::default(),
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
    pub config: Config,
}

impl App {
    pub fn new(presenter_kind: PresenterKind) -> AppResult<Self> {
        let config = Config::load()?;
        Self::new_with_config(presenter_kind, config)
    }

    pub fn new_with_config(presenter_kind: PresenterKind, config: Config) -> AppResult<Self> {
        let presenter = create_presenter_with_cache_limits(
            presenter_kind,
            Some((
                config.cache.l2_max_entries,
                config.cache.l2_memory_budget_bytes(),
            )),
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
            render: RenderSubsystem::new(
                presenter,
                RenderRuntime::from_cache_config(&config.cache),
            ),
            interaction: InteractionSubsystem::default(),
            config,
        })
    }
}
