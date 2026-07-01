use tui_input::{Input, InputRequest};

use crate::app::AppState;
use crate::error::AppResult;
use crate::extension::ExtensionUiSnapshot;
use crate::input::InputHistorySnapshot;

use super::candidate::{PaletteCandidate, PaletteCandidateId};
use super::effect::{PaletteSubmitAction, PaletteTabEffect};
use super::kind::PaletteKind;
use super::matcher::{CandidateMatcher, ContainsMatcher};
use super::provider::{PaletteAppSnapshot, PaletteContext, PaletteInputMode};
use super::registry::PaletteProviderRef;
use super::registry::PaletteRegistry;
use super::request::PaletteOpenOptions;
use super::view::{PaletteItemView, PaletteView};

#[derive(Debug)]
struct PaletteSession {
    id: u64,
    kind: PaletteKind,
    title: String,
    input_mode: PaletteInputMode,
    input: Input,
    candidates: Vec<PaletteCandidate>,
    visible: Vec<usize>,
    selected: usize,
    assistive_text: Option<String>,
    input_history: PaletteInputHistoryNavigator,
}

#[derive(Debug, Default)]
struct PaletteInputHistoryNavigator {
    snapshot: InputHistorySnapshot,
    cursor: Option<usize>,
    draft_input: Option<String>,
}

impl PaletteInputHistoryNavigator {
    fn new(snapshot: InputHistorySnapshot) -> Self {
        Self {
            snapshot,
            cursor: None,
            draft_input: None,
        }
    }

    fn clear_navigation(&mut self) {
        self.cursor = None;
        self.draft_input = None;
    }

    fn recall(&mut self, current_input: &str, older: bool) -> Option<String> {
        let entries = self.snapshot.entries();
        if entries.is_empty() {
            return None;
        }

        let next_cursor = if older {
            match self.cursor {
                Some(cursor) => Some(cursor.saturating_sub(1)),
                None => {
                    self.draft_input = Some(current_input.to_string());
                    Some(entries.len() - 1)
                }
            }
        } else {
            match self.cursor {
                Some(cursor) if cursor + 1 < entries.len() => Some(cursor + 1),
                Some(_) => None,
                None => return None,
            }
        };

        self.cursor = next_cursor;
        let next_value = next_cursor
            .map(|cursor| entries[cursor].clone())
            .or_else(|| self.draft_input.take())
            .unwrap_or_default();

        (next_value != current_input).then_some(next_value)
    }
}

pub struct PaletteSessionController {
    next_session_id: u64,
    active: Option<PaletteSession>,
    matcher: Box<dyn CandidateMatcher>,
}

impl Default for PaletteSessionController {
    fn default() -> Self {
        Self {
            next_session_id: 1,
            active: None,
            matcher: Box::new(ContainsMatcher),
        }
    }
}

impl PaletteSessionController {
    pub fn open(
        &mut self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
        kind: PaletteKind,
        options: PaletteOpenOptions,
        input_history: Option<InputHistorySnapshot>,
    ) -> AppResult<()> {
        let provider = registry.get(kind);

        let input = Input::new(options.initial_input.clone());
        let app = PaletteAppSnapshot::from(app);

        let ctx = PaletteContext {
            app,
            extensions,
            kind,
            input: input.value(),
        };
        let title = provider.title(&ctx);
        let candidates = provider.list(&ctx)?;
        let input_mode = provider.input_mode();
        let visible = self.visible_candidates(input_mode, input.value(), &candidates);
        let selected =
            initial_selection_from_id(options.initial_selection_id.as_ref(), &candidates, &visible)
                .or_else(|| initial_visible_selection(&provider, &ctx, &candidates, &visible))
                .unwrap_or(0);
        let selected_candidate = selected_candidate_for(&candidates, &visible, selected);
        let assistive_text = provider.assistive_text(&ctx, selected_candidate);

        self.active = Some(PaletteSession {
            id: self.take_session_id(),
            kind,
            title,
            input_mode,
            input,
            candidates,
            visible,
            selected,
            assistive_text,
            input_history: PaletteInputHistoryNavigator::new(input_history.unwrap_or_default()),
        });
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.active.is_some()
    }

    pub fn active_kind(&self) -> Option<PaletteKind> {
        self.active.as_ref().map(|session| session.kind)
    }

    pub fn active_input_is_empty(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|session| session.input.value().is_empty())
    }

    pub fn close(&mut self) -> bool {
        self.active.take().is_some()
    }

    pub fn close_if_matches(&mut self, session_id: u64) -> bool {
        let Some(session) = &self.active else {
            return false;
        };
        if session.id != session_id {
            return false;
        }
        self.active.take();
        true
    }

    pub fn submit(
        &self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
    ) -> AppResult<Option<PaletteSubmitAction>> {
        let Some(session) = self.active.as_ref() else {
            return Ok(None);
        };

        let selected = selected_candidate(session);
        let provider = registry.get(session.kind);
        let app_snapshot = PaletteAppSnapshot::from(app);
        let ctx = PaletteContext {
            app: app_snapshot,
            extensions,
            kind: session.kind,
            input: session.input.value(),
        };
        let effect = provider.on_submit(&ctx, selected)?;
        Ok(Some(PaletteSubmitAction {
            session_id: session.id,
            effect,
        }))
    }

    pub fn complete(
        &mut self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
    ) -> AppResult<bool> {
        let Some(session) = self.active.as_mut() else {
            return Ok(false);
        };

        let provider = registry.get(session.kind);
        let selected = selected_candidate(session);
        let previous_input = session.input.value().to_string();
        let app_snapshot = PaletteAppSnapshot::from(app);
        let ctx = PaletteContext {
            app: app_snapshot,
            extensions,
            kind: session.kind,
            input: session.input.value(),
        };
        match provider.on_tab(&ctx, selected)? {
            PaletteTabEffect::Noop => return Ok(false),
            PaletteTabEffect::SetInput {
                value,
                move_cursor_to_end,
            } => {
                let cursor = if move_cursor_to_end {
                    value.chars().count()
                } else {
                    session.input.cursor().min(value.chars().count())
                };
                if session.input.value() == value && session.input.cursor() == cursor {
                    return Ok(false);
                }
                session.input_history.clear_navigation();
                session.input = Input::new(value).with_cursor(cursor);
            }
        }
        self.rebuild(registry, app, extensions, Some(previous_input.as_str()))?;
        Ok(true)
    }

    pub fn select_previous(&mut self) -> bool {
        let previous = self.active.as_ref().map(|session| session.selected);
        self.select_prev();
        self.active.as_ref().map(|session| session.selected) != previous
    }

    pub fn select_next_item(&mut self) -> bool {
        let previous = self.active.as_ref().map(|session| session.selected);
        self.select_next();
        self.active.as_ref().map(|session| session.selected) != previous
    }

    pub fn recall_history(
        &mut self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
        older: bool,
    ) -> AppResult<bool> {
        let Some(session) = self.active.as_ref() else {
            return Ok(false);
        };
        if !session.kind.supports_input_history() {
            return Ok(false);
        }
        let previous_input = session.input.value().to_string();
        let changed = self.recall_input_history(older);
        if changed {
            self.rebuild(registry, app, extensions, Some(previous_input.as_str()))?;
        }
        Ok(changed)
    }

    pub fn insert_text(
        &mut self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
        text: &str,
    ) -> AppResult<bool> {
        self.apply_text_requests(
            registry,
            app,
            extensions,
            text.chars().map(InputRequest::InsertChar),
        )
    }

    pub fn edit_input(
        &mut self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
        request: InputRequest,
    ) -> AppResult<bool> {
        self.apply_text_request(registry, app, extensions, request)
    }

    pub fn view(&self) -> Option<PaletteView> {
        let session = self.active.as_ref()?;
        let mut items = Vec::new();
        for (idx_in_visible, candidate_idx) in session.visible.iter().enumerate() {
            if let Some(candidate) = session.candidates.get(*candidate_idx) {
                items.push(PaletteItemView {
                    label: candidate.label().to_vec(),
                    detail: candidate.detail().to_vec(),
                    selected: idx_in_visible == session.selected,
                });
            }
        }
        Some(PaletteView {
            title: session.title.clone(),
            kind: session.kind,
            input: session.input.value().to_string(),
            cursor: session.input.visual_cursor(),
            assistive_text: session.assistive_text.clone(),
            selected_idx: session.selected,
            items,
        })
    }

    fn rebuild(
        &mut self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
        previous_input: Option<&str>,
    ) -> AppResult<()> {
        let Some(existing) = self.active.as_ref() else {
            return Ok(());
        };
        let kind = existing.kind;
        let input_mode = existing.input_mode;
        let input_text = existing.input.value().to_string();
        let current_selected = existing.selected;

        let provider = registry.get(kind);
        let app = PaletteAppSnapshot::from(app);
        let ctx = PaletteContext {
            app,
            extensions,
            kind,
            input: &input_text,
        };

        let title = provider.title(&ctx);
        let candidates = provider.list(&ctx)?;
        let visible = self.visible_candidates(input_mode, &input_text, &candidates);
        let input_changed = previous_input.is_some_and(|input| input != input_text);
        let reset_selection = input_changed && provider.reset_selection_on_input_change();
        let selected = if reset_selection || visible.is_empty() {
            0
        } else {
            current_selected.min(visible.len().saturating_sub(1))
        };
        let selected_candidate = selected_candidate_for(&candidates, &visible, selected);
        let assistive_text = provider.assistive_text(&ctx, selected_candidate);

        let Some(session) = self.active.as_mut() else {
            return Ok(());
        };
        session.title = title;
        session.candidates = candidates;
        session.visible = visible;
        session.selected = selected;
        session.assistive_text = assistive_text;
        Ok(())
    }

    fn visible_candidates(
        &self,
        input_mode: PaletteInputMode,
        input: &str,
        candidates: &[PaletteCandidate],
    ) -> Vec<usize> {
        match input_mode {
            PaletteInputMode::FilterCandidates => self.matcher.select(input, candidates),
            PaletteInputMode::FreeText | PaletteInputMode::Custom => {
                (0..candidates.len()).collect()
            }
        }
    }

    fn select_prev(&mut self) {
        let Some(session) = self.active.as_mut() else {
            return;
        };
        if session.visible.is_empty() {
            session.selected = 0;
            return;
        }
        if session.selected > 0 {
            session.selected -= 1;
        }
    }

    fn select_next(&mut self) {
        let Some(session) = self.active.as_mut() else {
            return;
        };
        if session.visible.is_empty() {
            session.selected = 0;
            return;
        }
        if session.selected + 1 < session.visible.len() {
            session.selected += 1;
        }
    }

    fn take_session_id(&mut self) -> u64 {
        let id = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        id
    }

    fn recall_input_history(&mut self, older: bool) -> bool {
        let Some(session) = self.active.as_mut() else {
            return false;
        };
        let Some(next_value) = session.input_history.recall(session.input.value(), older) else {
            return false;
        };
        session.input = Input::new(next_value);
        true
    }

    fn apply_text_request(
        &mut self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
        request: InputRequest,
    ) -> AppResult<bool> {
        self.apply_text_requests(registry, app, extensions, [request])
    }

    fn apply_text_requests<I>(
        &mut self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
        requests: I,
    ) -> AppResult<bool>
    where
        I: IntoIterator<Item = InputRequest>,
    {
        let Some(session) = self.active.as_mut() else {
            return Ok(false);
        };

        let previous_input = session.input.value().to_string();
        let mut changed = false;
        let mut value_changed = false;
        for request in requests {
            if let Some(state_changed) = session.input.handle(request) {
                changed = true;
                value_changed |= state_changed.value;
            }
        }
        if !changed {
            return Ok(false);
        }
        if value_changed {
            session.input_history.clear_navigation();
            self.rebuild(registry, app, extensions, Some(previous_input.as_str()))?;
        }
        Ok(true)
    }
}

fn selected_candidate(session: &PaletteSession) -> Option<&PaletteCandidate> {
    selected_candidate_for(&session.candidates, &session.visible, session.selected)
}

fn initial_visible_selection(
    provider: &PaletteProviderRef<'_>,
    ctx: &PaletteContext<'_>,
    candidates: &[PaletteCandidate],
    visible: &[usize],
) -> Option<usize> {
    let selected_candidate_idx = provider.initial_selected_candidate(ctx, candidates)?;
    visible
        .iter()
        .position(|candidate_idx| *candidate_idx == selected_candidate_idx)
}

fn initial_selection_from_id(
    id: Option<&PaletteCandidateId>,
    candidates: &[PaletteCandidate],
    visible: &[usize],
) -> Option<usize> {
    let id = id?;
    visible
        .iter()
        .position(|candidate_idx| candidates.get(*candidate_idx).is_some_and(|c| c.id() == id))
}

fn selected_candidate_for<'a>(
    candidates: &'a [PaletteCandidate],
    visible: &[usize],
    selected: usize,
) -> Option<&'a PaletteCandidate> {
    visible.get(selected).and_then(|idx| candidates.get(*idx))
}

#[cfg(test)]
mod tests {
    use tui_input::InputRequest;

    use crate::{
        app::AppState,
        extension::ExtensionUiSnapshot,
        palette::{PaletteKind, PaletteRegistry},
    };

    use super::PaletteSessionController;

    #[test]
    fn operations_without_active_session_report_noop() {
        let registry = PaletteRegistry::default();
        let mut session = PaletteSessionController::default();
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();

        assert!(!session.is_open());
        assert_eq!(session.active_kind(), None);
        assert!(!session.active_input_is_empty());
        assert!(!session.close());
        assert!(!session.close_if_matches(1));
        assert!(!session.select_previous());
        assert!(!session.select_next_item());
        assert!(
            !session
                .complete(&registry, &app, &extensions)
                .expect("completion without active session should succeed")
        );
        assert!(
            !session
                .recall_history(&registry, &app, &extensions, true)
                .expect("history recall without active session should succeed")
        );
        assert!(
            !session
                .insert_text(&registry, &app, &extensions, "x")
                .expect("text insert without active session should succeed")
        );
        assert!(
            !session
                .edit_input(&registry, &app, &extensions, InputRequest::GoToPrevChar)
                .expect("input edit without active session should succeed")
        );
        assert!(
            session
                .submit(&registry, &app, &extensions)
                .expect("submit without active session should succeed")
                .is_none()
        );
        assert!(session.view().is_none());
    }

    #[test]
    fn common_selection_operations_report_noop_at_boundaries() {
        let registry = PaletteRegistry::default();
        let mut session = PaletteSessionController::default();
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();

        session
            .open(
                &registry,
                &app,
                &extensions,
                PaletteKind::Search,
                crate::palette::PaletteOpenOptions::default(),
                None,
            )
            .expect("search palette should open");

        assert!(!session.select_previous());
        assert!(
            !session
                .complete(&registry, &app, &extensions)
                .expect("no-op completion should succeed")
        );
        assert!(
            !session
                .edit_input(&registry, &app, &extensions, InputRequest::GoToPrevChar)
                .expect("no-op cursor movement should succeed")
        );
    }
}
