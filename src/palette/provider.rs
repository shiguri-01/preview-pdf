use super::candidate::PaletteCandidate;
use super::effect::{PaletteSubmitEffect, PaletteTabEffect};
use super::kind::PaletteKind;
use crate::app::{AppState, PageLayoutMode, SpreadCoverPolicy};
use crate::error::AppResult;
use crate::extension::ExtensionUiSnapshot;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaletteAppSnapshot {
    pub current_page: usize,
    pub page_layout_mode: PageLayoutMode,
    pub spread_cover_policy: SpreadCoverPolicy,
}

impl From<&AppState> for PaletteAppSnapshot {
    fn from(app: &AppState) -> Self {
        Self {
            current_page: app.current_page,
            page_layout_mode: app.page_layout_mode,
            spread_cover_policy: app.spread_cover_policy,
        }
    }
}

pub struct PaletteContext<'a> {
    pub app: PaletteAppSnapshot,
    pub extensions: &'a ExtensionUiSnapshot,
    pub input: &'a str,
}

pub trait PaletteProvider: Send + Sync {
    fn kind(&self) -> PaletteKind;
    fn title(&self, ctx: &PaletteContext<'_>) -> String;
    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>>;
    fn on_tab(
        &self,
        _ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteTabEffect> {
        Ok(PaletteTabEffect::Noop)
    }
    fn on_submit(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteSubmitEffect>;
    fn assistive_text(
        &self,
        _ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        None
    }
    /// Returns whether a changed input should reset the selected candidate to the first item.
    ///
    /// Defaults to `false` so providers that do not filter or reorder candidates by input keep
    /// their selection stable while typing.
    fn reset_selection_on_input_change(&self) -> bool {
        false
    }
    /// Returns the initially selected candidate index within `candidates`.
    ///
    /// Defaults to `None`, which keeps the session controller's normal first-item selection.
    fn initial_selected_candidate(
        &self,
        _ctx: &PaletteContext<'_>,
        _candidates: &[PaletteCandidate],
    ) -> Option<usize> {
        None
    }
}
