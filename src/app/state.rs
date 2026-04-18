use crate::error::AppError;
use crate::palette::{PaletteKind, PaletteOpenPayload};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageLayoutMode {
    #[default]
    Single,
    Spread,
}

impl PageLayoutMode {
    pub fn id(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Spread => "spread",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpreadDirection {
    #[default]
    Ltr,
    Rtl,
}

impl SpreadDirection {
    pub fn id(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisiblePageSlots {
    pub anchor_page: usize,
    pub trailing_page: Option<usize>,
    pub left_page: Option<usize>,
    pub right_page: Option<usize>,
}

impl VisiblePageSlots {
    pub fn existing_pages(self) -> [Option<usize>; 2] {
        [Some(self.anchor_page), self.trailing_page]
    }

    pub fn label(self, page_count: usize) -> String {
        let total = page_count.max(1);
        match self.trailing_page {
            Some(trailing) => format!("{}-{}", self.anchor_page + 1, trailing + 1),
            None => format!("{}/{}", self.anchor_page + 1, total),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Palette,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteRequest {
    Open {
        kind: PaletteKind,
        payload: Option<PaletteOpenPayload>,
    },
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeLevel {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub level: NoticeLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoticeAction {
    Keep,
    Clear,
    Show { level: NoticeLevel, message: String },
}

impl NoticeAction {
    pub fn warning(message: impl Into<String>) -> Self {
        Self::Show {
            level: NoticeLevel::Warning,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::Show {
            level: NoticeLevel::Error,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheHandle {
    pub name: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheRefs {
    pub l1_rendered_pages: Option<CacheHandle>,
    pub l2_terminal_frames: Option<CacheHandle>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub current_page: usize,
    pub page_layout_mode: PageLayoutMode,
    pub spread_direction: SpreadDirection,
    pub zoom: f32,
    pub pan_x: i32,
    pub pan_y: i32,
    pub help_scroll: usize,
    pub debug_status_visible: bool,
    pub mode: Mode,
    pub notice: Option<Notice>,
    pub caches: CacheRefs,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_page: 0,
            page_layout_mode: PageLayoutMode::Single,
            spread_direction: SpreadDirection::Ltr,
            zoom: 1.0,
            pan_x: 0,
            pan_y: 0,
            help_scroll: 0,
            debug_status_visible: false,
            mode: Mode::Normal,
            notice: None,
            caches: CacheRefs::default(),
        }
    }
}

impl AppState {
    const RENDER_NOTICE_PREFIX: &str = "Could not render";

    pub fn apply_notice_action(&mut self, action: NoticeAction) {
        match action {
            NoticeAction::Keep => {}
            NoticeAction::Clear => self.notice = None,
            NoticeAction::Show { level, message } => self.notice = Some(Notice { level, message }),
        }
    }

    pub fn set_notice(&mut self, level: NoticeLevel, message: impl Into<String>) {
        self.notice = Some(Notice {
            level,
            message: message.into(),
        });
    }

    pub fn set_warning_notice(&mut self, message: impl Into<String>) {
        self.set_notice(NoticeLevel::Warning, message);
    }

    pub fn set_error_notice(&mut self, message: impl Into<String>) {
        self.set_notice(NoticeLevel::Error, message);
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    pub fn reset_help_scroll(&mut self) {
        self.help_scroll = 0;
    }

    pub fn scroll_help_by(&mut self, delta: isize) {
        if delta >= 0 {
            self.help_scroll = self.help_scroll.saturating_add(delta as usize);
        } else {
            self.help_scroll = self.help_scroll.saturating_sub(delta.unsigned_abs());
        }
    }

    pub fn clear_render_notice(&mut self) {
        if self.notice.as_ref().is_some_and(|notice| {
            notice.level == NoticeLevel::Error
                && notice.message.starts_with(Self::RENDER_NOTICE_PREFIX)
        }) {
            self.notice = None;
        }
    }

    pub fn page_step(&self) -> usize {
        match self.page_layout_mode {
            PageLayoutMode::Single => 1,
            PageLayoutMode::Spread => 2,
        }
    }

    pub fn normalize_page_for_layout(&self, page: usize, page_count: usize) -> usize {
        if page_count == 0 {
            return 0;
        }

        let clamped = page.min(page_count - 1);
        match self.page_layout_mode {
            PageLayoutMode::Single => clamped,
            PageLayoutMode::Spread => clamped.saturating_sub(clamped % 2),
        }
    }

    pub fn normalize_current_page(&mut self, page_count: usize) {
        self.current_page = self.normalize_page_for_layout(self.current_page, page_count);
    }

    pub fn visible_page_slots(&self, page_count: usize) -> VisiblePageSlots {
        self.visible_page_slots_for_page(self.current_page, page_count)
    }

    pub fn visible_page_slots_for_page(&self, page: usize, page_count: usize) -> VisiblePageSlots {
        if page_count == 0 {
            return VisiblePageSlots {
                anchor_page: 0,
                trailing_page: None,
                left_page: Some(0),
                right_page: None,
            };
        }

        let anchor_page = self.normalize_page_for_layout(page, page_count);
        if self.page_layout_mode == PageLayoutMode::Single {
            return VisiblePageSlots {
                anchor_page,
                trailing_page: None,
                left_page: Some(anchor_page),
                right_page: None,
            };
        }

        let trailing_page = (anchor_page + 1 < page_count).then_some(anchor_page + 1);
        let (left_page, right_page) = match self.spread_direction {
            SpreadDirection::Ltr => (Some(anchor_page), trailing_page),
            SpreadDirection::Rtl => (trailing_page, Some(anchor_page)),
        };
        VisiblePageSlots {
            anchor_page,
            trailing_page,
            left_page,
            right_page,
        }
    }

    pub fn presenter_layout_tag(&self, has_trailing_page: bool) -> u16 {
        match (
            self.page_layout_mode,
            self.spread_direction,
            has_trailing_page,
        ) {
            (PageLayoutMode::Single, _, _) => 0,
            (PageLayoutMode::Spread, SpreadDirection::Ltr, true) => 1,
            (PageLayoutMode::Spread, SpreadDirection::Rtl, true) => 2,
            (PageLayoutMode::Spread, SpreadDirection::Ltr, false) => 3,
            (PageLayoutMode::Spread, SpreadDirection::Rtl, false) => 4,
        }
    }
}

pub fn notice_action_for_error(err: AppError) -> NoticeAction {
    match err {
        AppError::InvalidArgument(message)
        | AppError::Unsupported(message)
        | AppError::Unimplemented(message) => NoticeAction::warning(message),
        other => NoticeAction::error(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{NoticeAction, NoticeLevel, notice_action_for_error};
    use crate::error::AppError;

    #[test]
    fn notice_action_for_invalid_argument_is_warning() {
        assert_eq!(
            notice_action_for_error(AppError::invalid_argument("bad command")),
            NoticeAction::Show {
                level: NoticeLevel::Warning,
                message: "bad command".to_string(),
            }
        );
    }

    #[test]
    fn notice_action_for_io_error_is_error() {
        let err = AppError::io_with_context(io::Error::other("boom"), "disk failed");
        assert_eq!(
            notice_action_for_error(err),
            NoticeAction::Show {
                level: NoticeLevel::Error,
                message: "I/O error: disk failed".to_string(),
            }
        );
    }
}
