use crate::extension::{
    HistoryPaletteProvider, OutlinePaletteProvider, SearchPaletteProvider,
    SearchResultsPaletteProvider,
};

use super::providers::CommandPaletteProvider;
use super::{PaletteKind, PaletteProvider};

pub fn provider(kind: PaletteKind) -> &'static dyn PaletteProvider {
    match kind {
        PaletteKind::Command => &CommandPaletteProvider,
        PaletteKind::Search => &SearchPaletteProvider,
        PaletteKind::SearchResults => &SearchResultsPaletteProvider,
        PaletteKind::History => &HistoryPaletteProvider,
        PaletteKind::Outline => &OutlinePaletteProvider,
    }
}

#[cfg(test)]
mod tests {
    use crate::extension::ExtensionUiSnapshot;
    use crate::palette::{PaletteAppSnapshot, PaletteContext, PaletteKind};

    use super::provider;

    #[test]
    fn get_returns_provider_for_all_palette_kinds() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app: PaletteAppSnapshot::default(),
            extensions: &extensions,
            input: "",
        };

        assert_eq!(provider(PaletteKind::Command).kind(), PaletteKind::Command);
        assert_eq!(provider(PaletteKind::Search).kind(), PaletteKind::Search);
        assert_eq!(
            provider(PaletteKind::SearchResults).kind(),
            PaletteKind::SearchResults
        );
        assert_eq!(provider(PaletteKind::History).kind(), PaletteKind::History);
        assert_eq!(provider(PaletteKind::Outline).kind(), PaletteKind::Outline);
        assert!(!provider(PaletteKind::Command).title(&ctx).is_empty());
    }
}
