use super::candidate::PaletteCandidateId;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaletteOpenOptions {
    pub initial_input: String,
    pub initial_selection_id: Option<PaletteCandidateId>,
}

impl PaletteOpenOptions {
    pub fn input(input: impl Into<String>) -> Self {
        Self {
            initial_input: input.into(),
            initial_selection_id: None,
        }
    }

    pub fn input_with_selection(
        input: impl Into<String>,
        selection_id: impl Into<PaletteCandidateId>,
    ) -> Self {
        Self {
            initial_input: input.into(),
            initial_selection_id: Some(selection_id.into()),
        }
    }
}
