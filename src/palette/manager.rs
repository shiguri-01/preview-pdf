use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use crate::app::AppState;
use crate::error::AppResult;

use super::kind::PaletteKind;
use super::matcher::{CandidateMatcher, ContainsMatcher};
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
        kind: PaletteKind,
        seed: Option<String>,
    ) -> AppResult<()> {
        let provider = registry.get(kind);

        let input = Input::new(provider.initial_input(seed.as_deref()));

        let ctx = PaletteContext {
            app,
            kind,
            input: input.value(),
            seed: seed.as_deref(),
        };
        let title = provider.title(&ctx);
        let candidates = provider.list(&ctx)?;
        let input_mode = provider.input_mode();
        let visible = self.visible_candidates(input_mode, input.value(), &candidates);
        let selected = 0;
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
        app: &AppState,
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
                let ctx = PaletteContext {
                    app,
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
                self.rebuild(registry, app)?;
                return Ok(PaletteKeyResult::Consumed { redraw: true });
            }
            KeyCode::Enter => {
                let selected = selected_candidate(session);
                let provider = registry.get(session.kind);
                let ctx = PaletteContext {
                    app,
                    kind: session.kind,
                    input: session.input.value(),
                    seed: session.seed.as_deref(),
                };
                let effect = provider.on_submit(&ctx, selected)?;
                return Ok(PaletteKeyResult::Submit(PaletteSubmitAction {
                    session_id: session.id,
                    effect,
                }));
            }
            _ => {}
        }

        session.input.handle_event(&Event::Key(key));
        self.rebuild(registry, app)?;
        Ok(PaletteKeyResult::Consumed { redraw: true })
    }

    pub fn view(&self) -> Option<PaletteView> {
        let session = self.active.as_ref()?;
        let mut items = Vec::new();
        for (idx_in_visible, candidate_idx) in session.visible.iter().enumerate() {
            if let Some(candidate) = session.candidates.get(*candidate_idx) {
                items.push(PaletteItemView {
                    label: candidate.label.clone(),
                    detail: candidate.detail.clone(),
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

    fn rebuild(&mut self, registry: &PaletteRegistry, app: &AppState) -> AppResult<()> {
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
            kind,
            input: &input_text,
            seed: seed.as_deref(),
        };

        let title = provider.title(&ctx);
        let candidates = provider.list(&ctx)?;
        let visible = self.visible_candidates(input_mode, &input_text, &candidates);
        let selected = if visible.is_empty() {
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

fn selected_candidate(session: &PaletteSession) -> Option<&PaletteCandidate> {
    selected_candidate_for(&session.candidates, &session.visible, session.selected)
}

fn selected_candidate_for<'a>(
    candidates: &'a [PaletteCandidate],
    visible: &[usize],
    selected: usize,
) -> Option<&'a PaletteCandidate> {
    visible.get(selected).and_then(|idx| candidates.get(*idx))
}
