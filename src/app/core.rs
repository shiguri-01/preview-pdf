use std::collections::VecDeque;

use crate::config::Config;
use crate::error::AppResult;
use crate::extension::ExtensionHost;
use crate::palette::{PaletteManager, PaletteRegistry};
use crate::presenter::{ImagePresenter, PresenterKind, create_presenter_with_cache_limits};

use super::runtime::RenderRuntime;
use super::state::{AppState, CacheHandle, PaletteRequest};

pub struct RenderSubsystem {
    pub presenter: Box<dyn ImagePresenter>,
    pub runtime: RenderRuntime,
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

#[derive(Default)]
pub struct InteractionSubsystem {
    pub extensions: ExtensionSubsystem,
    pub palette: PaletteSubsystem,
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
            render: RenderSubsystem {
                presenter,
                runtime: RenderRuntime::from_cache_config(&config.cache),
            },
            interaction: InteractionSubsystem::default(),
            config,
        })
    }
}
