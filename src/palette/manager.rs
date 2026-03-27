use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use crate::app::AppState;
use crate::error::{AppError, AppResult};
use crate::extension::ExtensionUiSnapshot;

use super::kind::PaletteKind;
use super::matcher::{CandidateMatcher, ContainsMatcher};
use super::registry::PaletteProviderRef;
use super::registry::PaletteRegistry;
use super::types::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteItemView, PaletteKeyResult,
    PaletteSubmitAction, PaletteTabEffect, PaletteView,
};

#[derive(Debug)]
struct PaletteSession {
    id: u64,
    kind: PaletteKind,
    seed: Option<String>,
    title: String,
    input_mode: PaletteInputMode,
    input: Input,
    candidates: Vec<PaletteCandidate>,
    visible: Vec<usize>,
    selected: usize,
    assistive_text: Option<String>,
}

pub struct PaletteManager {
    next_session_id: u64,
    active: Option<PaletteSession>,
    matcher: Box<dyn CandidateMatcher>,
}

impl Default for PaletteManager {
    fn default() -> Self {
        Self {
            next_session_id: 1,
            active: None,
            matcher: Box::new(ContainsMatcher),
        }
    }
}

impl PaletteManager {
    pub fn open(
        &mut self,
        registry: &PaletteRegistry,
        app: &AppState,
        extensions: &ExtensionUiSnapshot,
        kind: PaletteKind,
        seed: Option<String>,
    ) -> AppResult<()> {
        let provider = registry.get(kind);

        let input = Input::new(provider.initial_input(seed.as_deref()));

        let ctx = PaletteContext {
            app,
            extensions,
            kind,
            input: input.value(),
            seed: seed.as_deref(),
        };
        let title = provider.title(&ctx);
        let candidates = provider.list(&ctx)?;
        let input_mode = provider.input_mode();
        let visible = self.visible_candidates(input_mode, input.value(), &candidates);
        let selected =
            initial_visible_selection(&provider, &ctx, &candidates, &visible).unwrap_or(0);
        let selected_candidate = selected_candidate_for(&candidates, &visible, selected);
        let assistive_text = provider.assistive_text(&ctx, selected_candidate);

        self.active = Some(PaletteSession {
            id: self.take_session_id(),
            kind,
            seed,
            title,
            input_mode,
            input,
            candidates,
            visible,
            selected,
            assistive_text,
        });
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.active.is_some()
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

    pub fn handle_key(
        &mut self,
        registry: &PaletteRegistry,
        app: &mut AppState,
        extensions: &ExtensionUiSnapshot,
        key: KeyEvent,
    ) -> AppResult<PaletteKeyResult> {
        let Some(session) = self.active.as_mut() else {
            return Ok(PaletteKeyResult::Consumed { redraw: false });
        };

        match key.code {
            KeyCode::Esc => {
                return Ok(PaletteKeyResult::CloseRequested {
                    session_id: session.id,
                });
            }
            KeyCode::Up => {
                self.select_prev();
                return Ok(PaletteKeyResult::Consumed { redraw: true });
            }
            KeyCode::Down => {
                self.select_next();
                return Ok(PaletteKeyResult::Consumed { redraw: true });
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.select_prev();
                return Ok(PaletteKeyResult::Consumed { redraw: true });
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.select_next();
                return Ok(PaletteKeyResult::Consumed { redraw: true });
            }
            KeyCode::Tab => {
                let provider = registry.get(session.kind);
                let selected = selected_candidate(session);
                let previous_input = session.input.value().to_string();
                let ctx = PaletteContext {
                    app,
                    extensions,
                    kind: session.kind,
                    input: session.input.value(),
                    seed: session.seed.as_deref(),
                };
                match provider.on_tab(&ctx, selected)? {
                    PaletteTabEffect::Noop => {}
                    PaletteTabEffect::SetInput {
                        value,
                        move_cursor_to_end: _move_cursor_to_end,
                    } => {
                        session.input = Input::new(value);
                    }
                }
                self.rebuild(registry, app, extensions, Some(previous_input.as_str()))?;
                return Ok(PaletteKeyResult::Consumed { redraw: true });
            }
            KeyCode::Enter => {
                let selected = selected_candidate(session);
                let provider = registry.get(session.kind);
                let ctx = PaletteContext {
                    app,
                    extensions,
                    kind: session.kind,
                    input: session.input.value(),
                    seed: session.seed.as_deref(),
                };
                let effect = match provider.on_submit(&ctx, selected) {
                    Ok(effect) => effect,
                    Err(err) => {
                        apply_palette_submit_error_notice(app, err);
                        return Ok(PaletteKeyResult::Consumed { redraw: true });
                    }
                };
                return Ok(PaletteKeyResult::Submit(PaletteSubmitAction {
                    session_id: session.id,
                    effect,
                }));
            }
            _ => {}
        }

        let previous_input = session.input.value().to_string();
        session.input.handle_event(&Event::Key(key));
        self.rebuild(registry, app, extensions, Some(previous_input.as_str()))?;
        Ok(PaletteKeyResult::Consumed { redraw: true })
    }

    pub fn view(&self) -> Option<PaletteView> {
        let session = self.active.as_ref()?;
        let mut items = Vec::new();
        for (idx_in_visible, candidate_idx) in session.visible.iter().enumerate() {
            if let Some(candidate) = session.candidates.get(*candidate_idx) {
                items.push(PaletteItemView {
                    left: candidate.left.clone(),
                    right: candidate.right.clone(),
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
        let seed = existing.seed.clone();
        let input_mode = existing.input_mode;
        let input_text = existing.input.value().to_string();
        let current_selected = existing.selected;

        let provider = registry.get(kind);
        let ctx = PaletteContext {
            app,
            extensions,
            kind,
            input: &input_text,
            seed: seed.as_deref(),
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
}

fn apply_palette_submit_error_notice(app: &mut AppState, err: AppError) {
    match err {
        AppError::InvalidArgument(message)
        | AppError::Unsupported(message)
        | AppError::Unimplemented(message) => app.set_warning_notice(message),
        other => app.set_error_notice(other.to_string()),
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

fn selected_candidate_for<'a>(
    candidates: &'a [PaletteCandidate],
    visible: &[usize],
    selected: usize,
) -> Option<&'a PaletteCandidate> {
    visible.get(selected).and_then(|idx| candidates.get(*idx))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::{
        app::AppState,
        extension::ExtensionUiSnapshot,
        palette::{PaletteKind, PaletteRegistry},
    };

    use super::PaletteManager;

    #[test]
    fn command_palette_resets_selection_when_input_changes() {
        let registry = PaletteRegistry::default();
        let mut manager = PaletteManager::default();
        let mut app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();

        manager
            .open(&registry, &app, &extensions, PaletteKind::Command, None)
            .expect("command palette should open");

        let initial_view = manager.view().expect("palette should be visible");
        assert!(initial_view.items.len() > 1);

        manager
            .handle_key(
                &registry,
                &mut app,
                &extensions,
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            )
            .expect("selection move should succeed");
        let selected_view = manager.view().expect("palette should be visible");
        assert_eq!(selected_view.selected_idx, 1);

        manager
            .handle_key(
                &registry,
                &mut app,
                &extensions,
                KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
            )
            .expect("typing should succeed");
        let filtered_view = manager.view().expect("palette should be visible");
        assert_eq!(filtered_view.selected_idx, 0);
        assert_eq!(filtered_view.input, "p");
    }

    #[test]
    fn search_palette_keeps_selection_when_input_changes() {
        let registry = PaletteRegistry::default();
        let mut manager = PaletteManager::default();
        let mut app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();

        manager
            .open(&registry, &app, &extensions, PaletteKind::Search, None)
            .expect("search palette should open");

        manager
            .handle_key(
                &registry,
                &mut app,
                &extensions,
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            )
            .expect("selection move should succeed");
        let selected_view = manager.view().expect("palette should be visible");
        assert_eq!(selected_view.selected_idx, 1);

        manager
            .handle_key(
                &registry,
                &mut app,
                &extensions,
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            )
            .expect("typing should succeed");
        let updated_view = manager.view().expect("palette should be visible");
        assert_eq!(updated_view.selected_idx, 1);
        assert_eq!(updated_view.input, "a");
    }

    #[test]
    fn history_palette_selects_current_candidate_on_open() {
        let registry = PaletteRegistry::default();
        let mut manager = PaletteManager::default();
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();

        manager
            .open(
                &registry,
                &app,
                &extensions,
                PaletteKind::History,
                Some("f:5,Search: later|c:4|b:3,Search: earlier".to_string()),
            )
            .expect("history palette should open");

        let view = manager.view().expect("palette should be visible");
        assert_eq!(view.selected_idx, 1);
        assert!(view.items[1].selected);
    }
}
