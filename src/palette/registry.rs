use crate::extension::{
    HistoryPaletteProvider, OutlinePaletteProvider, SearchPaletteProvider,
    SearchResultsPaletteProvider,
};

use super::providers::CommandPaletteProvider;
use super::{PaletteKind, PaletteProvider};

pub struct PaletteRegistry {
    command: CommandPaletteProvider,
    search: SearchPaletteProvider,
    search_results: SearchResultsPaletteProvider,
    history: HistoryPaletteProvider,
    outline: OutlinePaletteProvider,
}

impl Default for PaletteRegistry {
    fn default() -> Self {
        Self {
            command: CommandPaletteProvider,
            search: SearchPaletteProvider,
            search_results: SearchResultsPaletteProvider,
            history: HistoryPaletteProvider,
            outline: OutlinePaletteProvider,
        }
    }
}

impl PaletteRegistry {
    pub fn get(&self, kind: PaletteKind) -> &dyn PaletteProvider {
        match kind {
            PaletteKind::Command => &self.command,
            PaletteKind::Search => &self.search,
            PaletteKind::SearchResults => &self.search_results,
            PaletteKind::History => &self.history,
            PaletteKind::Outline => &self.outline,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::extension::ExtensionUiSnapshot;
    use crate::palette::{PaletteAppSnapshot, PaletteContext, PaletteKind};

    use super::PaletteRegistry;

    #[test]
    fn get_returns_provider_for_all_palette_kinds() {
        let registry = PaletteRegistry::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app: PaletteAppSnapshot::default(),
            extensions: &extensions,
            input: "",
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
            registry.get(PaletteKind::SearchResults).kind(),
            PaletteKind::SearchResults
        );
        assert_eq!(
            registry.get(PaletteKind::History).kind(),
            PaletteKind::History
        );
        assert_eq!(
            registry.get(PaletteKind::Outline).kind(),
            PaletteKind::Outline
        );
        assert!(!registry.get(PaletteKind::Command).title(&ctx).is_empty());
    }
}
