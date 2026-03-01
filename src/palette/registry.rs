use crate::error::AppResult;
use crate::extension::{HistoryPaletteProvider, SearchPaletteProvider};

use super::providers::CommandPaletteProvider;
use super::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PaletteProvider,
    PaletteSubmitEffect, PaletteTabEffect,
};

pub struct PaletteRegistry {
    command: CommandPaletteProvider,
    search: SearchPaletteProvider,
    history: HistoryPaletteProvider,
}

pub enum PaletteProviderRef<'a> {
    Command(&'a CommandPaletteProvider),
    Search(&'a SearchPaletteProvider),
    History(&'a HistoryPaletteProvider),
}

impl Default for PaletteRegistry {
    fn default() -> Self {
        Self {
            command: CommandPaletteProvider,
            search: SearchPaletteProvider,
            history: HistoryPaletteProvider,
        }
    }
}

impl PaletteRegistry {
    pub fn get(&self, kind: PaletteKind) -> PaletteProviderRef<'_> {
        match kind {
            PaletteKind::Command => PaletteProviderRef::Command(&self.command),
            PaletteKind::Search => PaletteProviderRef::Search(&self.search),
            PaletteKind::History => PaletteProviderRef::History(&self.history),
        }
    }
}

impl<'a> PaletteProviderRef<'a> {
    pub fn kind(&self) -> PaletteKind {
        match self {
            Self::Command(provider) => provider.kind(),
            Self::Search(provider) => provider.kind(),
            Self::History(provider) => provider.kind(),
        }
    }

    pub fn title(&self, ctx: &PaletteContext<'_>) -> String {
        match self {
            Self::Command(provider) => provider.title(ctx),
            Self::Search(provider) => provider.title(ctx),
            Self::History(provider) => provider.title(ctx),
        }
    }

    pub fn input_mode(&self) -> PaletteInputMode {
        match self {
            Self::Command(provider) => provider.input_mode(),
            Self::Search(provider) => provider.input_mode(),
            Self::History(provider) => provider.input_mode(),
        }
    }

    pub fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        match self {
            Self::Command(provider) => provider.list(ctx),
            Self::Search(provider) => provider.list(ctx),
            Self::History(provider) => provider.list(ctx),
        }
    }

    pub fn on_tab(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteTabEffect> {
        match self {
            Self::Command(provider) => provider.on_tab(ctx, selected),
            Self::Search(provider) => provider.on_tab(ctx, selected),
            Self::History(provider) => provider.on_tab(ctx, selected),
        }
    }

    pub fn on_submit(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteSubmitEffect> {
        match self {
            Self::Command(provider) => provider.on_submit(ctx, selected),
            Self::Search(provider) => provider.on_submit(ctx, selected),
            Self::History(provider) => provider.on_submit(ctx, selected),
        }
    }

    pub fn assistive_text(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        match self {
            Self::Command(provider) => provider.assistive_text(ctx, selected),
            Self::Search(provider) => provider.assistive_text(ctx, selected),
            Self::History(provider) => provider.assistive_text(ctx, selected),
        }
    }

    pub fn initial_input(&self, seed: Option<&str>) -> String {
        match self {
            Self::Command(provider) => provider.initial_input(seed),
            Self::Search(provider) => provider.initial_input(seed),
            Self::History(provider) => provider.initial_input(seed),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::palette::{PaletteContext, PaletteKind};

    use super::PaletteRegistry;

    #[test]
    fn get_returns_provider_for_all_palette_kinds() {
        let registry = PaletteRegistry::default();
        let ctx = PaletteContext {
            app: &crate::app::AppState::default(),
            kind: PaletteKind::Command,
            input: "",
            seed: None,
        };

        assert_eq!(
            registry.get(PaletteKind::Command).kind(),
            PaletteKind::Command
        );
        assert_eq!(
            registry.get(PaletteKind::Search).kind(),
            PaletteKind::Search
        );
        assert_eq!(
            registry.get(PaletteKind::History).kind(),
            PaletteKind::History
        );
        assert!(!registry.get(PaletteKind::Command).title(&ctx).is_empty());
    }
}
