use std::collections::VecDeque;

use crate::palette::PaletteKind;

const INPUT_HISTORY_CAPACITY: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputHistoryRecord {
    Command(String),
    SearchQuery(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputHistorySnapshot {
    entries: Vec<String>,
}

impl InputHistorySnapshot {
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    #[cfg(test)]
    pub(crate) fn from_entries(entries: &[&str]) -> Self {
        Self {
            entries: entries.iter().map(|entry| (*entry).to_string()).collect(),
        }
    }
}

#[derive(Debug, Default)]
pub struct InputHistoryService {
    commands: HistoryRing,
    search_queries: HistoryRing,
}

impl InputHistoryService {
    pub fn record(&mut self, record: InputHistoryRecord) {
        match record {
            InputHistoryRecord::Command(command) => self.commands.record(command),
            InputHistoryRecord::SearchQuery(query) => self.search_queries.record(query),
        }
    }

    pub fn snapshot_for_palette(&self, kind: PaletteKind) -> Option<InputHistorySnapshot> {
        match kind {
            PaletteKind::Command => Some(self.command_snapshot()),
            PaletteKind::Search => Some(self.search_snapshot()),
            PaletteKind::History | PaletteKind::Outline => None,
        }
    }

    pub fn command_snapshot(&self) -> InputHistorySnapshot {
        self.commands.snapshot()
    }

    pub fn search_snapshot(&self) -> InputHistorySnapshot {
        self.search_queries.snapshot()
    }

    pub fn last_command(&self) -> Option<&str> {
        self.commands.last()
    }
}

#[derive(Debug, Default)]
struct HistoryRing {
    entries: VecDeque<String>,
}

impl HistoryRing {
    fn record(&mut self, value: String) {
        let normalized = value.trim().to_string();
        if normalized.is_empty() {
            return;
        }

        if let Some(idx) = self.entries.iter().position(|entry| entry == &normalized) {
            self.entries.remove(idx);
        }

        if self.entries.len() >= INPUT_HISTORY_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(normalized);
    }

    fn snapshot(&self) -> InputHistorySnapshot {
        InputHistorySnapshot {
            entries: self.entries.iter().cloned().collect(),
        }
    }

    fn last(&self) -> Option<&str> {
        self.entries.back().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use crate::palette::PaletteKind;

    use super::{InputHistoryRecord, InputHistoryService};

    #[test]
    fn command_history_is_mru_deduplicated() {
        let mut history = InputHistoryService::default();

        history.record(InputHistoryRecord::Command("next-page".to_string()));
        history.record(InputHistoryRecord::Command("prev-page".to_string()));
        history.record(InputHistoryRecord::Command("next-page".to_string()));

        let snapshot = history.command_snapshot();
        assert_eq!(
            snapshot.entries(),
            &["prev-page".to_string(), "next-page".to_string()]
        );
        assert_eq!(history.last_command(), Some("next-page"));
    }

    #[test]
    fn empty_entries_are_ignored() {
        let mut history = InputHistoryService::default();

        history.record(InputHistoryRecord::Command("   ".to_string()));
        history.record(InputHistoryRecord::SearchQuery(String::new()));

        assert!(history.command_snapshot().entries().is_empty());
        assert!(history.search_snapshot().entries().is_empty());
    }

    #[test]
    fn palette_snapshots_only_exist_for_command_and_search() {
        let history = InputHistoryService::default();

        assert!(history.snapshot_for_palette(PaletteKind::Command).is_some());
        assert!(history.snapshot_for_palette(PaletteKind::Search).is_some());
        assert!(history.snapshot_for_palette(PaletteKind::History).is_none());
        assert!(history.snapshot_for_palette(PaletteKind::Outline).is_none());
    }
}
