use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

use crate::app::{Notice, NoticeLevel, PageLayoutMode, VisiblePageSlots};

use super::layout::UiLayout;
use super::{border, error_text, primary_text, warning_text};

const MIN_FILENAME_ELISION_WIDTH: usize = 7;

#[derive(Debug, Clone, PartialEq)]
pub struct ChromeViewState {
    pub visible_pages: VisiblePageSlots,
    pub page_presentation: PageLayoutMode,
    pub zoom: f32,
    pub notice: Option<Notice>,
}

#[allow(clippy::too_many_arguments)]
pub fn draw_chrome(
    frame: &mut Frame<'_>,
    layout: UiLayout,
    chrome: &ChromeViewState,
    file_name: &str,
    page_count: usize,
    extension_status_segments: &[String],
) {
    let status_text = build_status_text(
        chrome,
        file_name,
        page_count,
        extension_status_segments,
        layout.status.width as usize,
    );
    let primary = if let Some(notice) = chrome.notice.as_ref() {
        Paragraph::new(stylize_notice_line(notice, layout.status.width as usize))
            .style(primary_text())
            .wrap(Wrap { trim: true })
    } else {
        Paragraph::new(stylize_status_line(&status_text))
            .style(primary_text())
            .wrap(Wrap { trim: true })
    };
    frame.render_widget(primary, layout.status);
}

fn build_status_text(
    chrome: &ChromeViewState,
    file_name: &str,
    page_count: usize,
    extension_status_segments: &[String],
    max_width: usize,
) -> String {
    let page_total = page_count.max(1);
    let base = format!(
        "{} | zoom {:.2}x",
        format_page_segment(chrome, page_total),
        chrome.zoom
    );
    let sep = " | ";

    if max_width == 0 {
        return String::new();
    }

    if display_width(&base) >= max_width {
        return base.unicode_truncate(max_width).0.trim_end().to_string();
    }

    let ext = extension_status_segments
        .iter()
        .rev()
        .find(|s| !s.is_empty())
        .map(String::as_str);

    if let Some(ext_text) = ext {
        let fixed_with_ext = display_width(&base) + display_width(sep) + display_width(ext_text);
        if fixed_with_ext <= max_width {
            let with_filename_fixed = fixed_with_ext + display_width(sep);
            if with_filename_fixed < max_width {
                let filename_budget = max_width - with_filename_fixed;
                let filename = format_filename_segment(file_name, filename_budget);
                if !filename.is_empty() {
                    return format!("{base}{sep}{filename}{sep}{ext_text}");
                }
            }
            return format!("{base}{sep}{ext_text}");
        }
    }

    let fixed_with_filename = display_width(&base) + display_width(sep);
    if fixed_with_filename < max_width {
        let filename_budget = max_width - fixed_with_filename;
        let filename = format_filename_segment(file_name, filename_budget);
        if !filename.is_empty() {
            return format!("{base}{sep}{filename}");
        }
        return base;
    }

    base.unicode_truncate(max_width).0.trim_end().to_string()
}

fn format_page_segment(chrome: &ChromeViewState, page_total: usize) -> String {
    let slots = chrome.visible_pages;
    let page_width = page_total.to_string().len();
    match chrome.page_presentation {
        PageLayoutMode::Single => {
            let page_now = slots.anchor_page.saturating_add(1).min(page_total);
            format!("p.{:>page_width$}/{:>page_width$}", page_now, page_total)
        }
        PageLayoutMode::Spread => match slots.trailing_page {
            Some(trailing) => format!(
                "pp.{:>page_width$}-{:>page_width$}/{:>page_width$}",
                slots.anchor_page + 1,
                trailing + 1,
                page_total
            ),
            None => format!(
                "pp.{:>page_width$}/{:>page_width$}",
                slots.anchor_page + 1,
                page_total
            ),
        },
    }
}

fn stylize_status_line(text: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (idx, part) in text.split(" | ").enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" | ".to_string(), border()));
        }
        spans.push(Span::styled(part.to_string(), primary_text()));
    }
    Line::from(spans)
}

fn stylize_notice_line(notice: &Notice, max_width: usize) -> Line<'static> {
    let label = match notice.level {
        NoticeLevel::Warning => "notice",
        NoticeLevel::Error => "error",
    };
    let accent = match notice.level {
        NoticeLevel::Warning => warning_text(),
        NoticeLevel::Error => error_text(),
    };
    let text = format!("{label}: {}", notice.message);
    Line::from(vec![Span::styled(
        text.unicode_truncate(max_width).0.to_string(),
        accent,
    )])
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn format_filename_segment(input: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if display_width(input) <= max_width {
        return input.to_string();
    }
    if max_width < MIN_FILENAME_ELISION_WIDTH {
        return String::new();
    }

    elide_middle_by_width(input, max_width)
}

fn elide_middle_by_width(input: &str, max_width: usize) -> String {
    const ELLIPSIS: &str = "…";
    let ellipsis_width = display_width(ELLIPSIS);
    if max_width == 0 {
        return String::new();
    }
    if display_width(input) <= max_width {
        return input.to_string();
    }
    if max_width <= ellipsis_width {
        return input.unicode_truncate(max_width).0.to_string();
    }

    let content_budget = max_width - ellipsis_width;
    let suffix_budget = (content_budget.saturating_mul(2) / 3).max(1);
    let (suffix, suffix_width) = input.unicode_truncate_start(suffix_budget);
    let prefix_budget = content_budget.saturating_sub(suffix_width);
    let prefix_limit = input.len().saturating_sub(suffix.len());
    let (prefix, _) = input[..prefix_limit].unicode_truncate(prefix_budget);

    if prefix.is_empty() {
        let (kept_suffix, _) = suffix.unicode_truncate(content_budget);
        return format!("{ELLIPSIS}{kept_suffix}");
    }

    format!("{prefix}{ELLIPSIS}{suffix}")
}

#[cfg(test)]
mod tests {
    use crate::app::{AppState, Notice, NoticeLevel, PageLayoutMode, SpreadCoverPolicy};

    use super::{
        ChromeViewState, build_status_text, display_width, format_filename_segment,
        stylize_notice_line,
    };

    fn chrome_from_app(app: &AppState, page_count: usize) -> ChromeViewState {
        ChromeViewState {
            visible_pages: app.visible_page_slots(page_count),
            page_presentation: app.page_presentation_for_slots(app.visible_page_slots(page_count)),
            zoom: app.zoom,
            notice: app.notice.clone(),
        }
    }

    #[test]
    fn build_status_text_includes_page_zoom_and_file() {
        let app = AppState {
            current_page: 2,
            zoom: 1.5,
            ..AppState::default()
        };

        let text = build_status_text(&chrome_from_app(&app, 10), "sample.pdf", 10, &[], 80);
        assert_eq!(text, "p. 3/10 | zoom 1.50x | sample.pdf");
    }

    #[test]
    fn stylize_notice_line_formats_and_truncates_messages() {
        for (name, message, max_width, expected) in [
            (
                "severity prefix",
                "render failed",
                80,
                "error: render failed",
            ),
            ("emoji grapheme truncation", "👨‍👩‍👧‍👦 failed", 9, "error: 👨‍👩‍👧‍👦"),
        ] {
            let line = stylize_notice_line(
                &Notice {
                    level: NoticeLevel::Error,
                    message: message.to_string(),
                },
                max_width,
            );
            assert_eq!(line.to_string(), expected, "{name}");
        }
    }

    #[test]
    fn format_filename_segment_handles_fit_drop_and_elision() {
        for (name, input, max_width, expected) in [
            ("short name fits", "a.pdf", 5, "a.pdf"),
            (
                "long name below elision threshold at width five",
                "very-long-document-name.pdf",
                5,
                "",
            ),
            (
                "long name below elision threshold at width six",
                "very-long-document-name.pdf",
                6,
                "",
            ),
            (
                "long name elides at threshold",
                "very-long-document-name.pdf",
                7,
                "ve….pdf",
            ),
            (
                "elision preserves combining characters and emoji",
                "e\u{301}bcdef👨‍👩‍👧‍👦.pdf",
                10,
                "e\u{301}bc…👨‍👩‍👧‍👦.pdf",
            ),
        ] {
            assert_eq!(
                format_filename_segment(input, max_width),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn build_status_text_uses_last_non_empty_extension_segment() {
        let app = AppState::default();
        let text = build_status_text(
            &chrome_from_app(&app, 5),
            "sample.pdf",
            5,
            &[
                String::from("SEARCH 2/10"),
                String::new(),
                String::from("HISTORY 1/3"),
            ],
            120,
        );
        assert_eq!(text, "p.1/5 | zoom 1.00x | sample.pdf | HISTORY 1/3");
    }

    #[test]
    fn build_status_text_elides_filename_in_middle_on_tight_width() {
        let app = AppState::default();
        let text = build_status_text(
            &chrome_from_app(&app, 7),
            "very-long-document-name.pdf",
            7,
            &[],
            28,
        );
        assert!(text.starts_with("p.1/7 | zoom 1.00x |"));
        assert!(display_width(&text) <= 28);
    }

    #[test]
    fn build_status_text_drops_filename_before_extension() {
        let app = AppState::default();
        let text = build_status_text(
            &chrome_from_app(&app, 7),
            "very-long-document-name.pdf",
            7,
            &[String::from("SEARCH 10/100")],
            38,
        );
        assert_eq!(text, "p.1/7 | zoom 1.00x | SEARCH 10/100");
    }

    #[test]
    fn build_status_text_handles_very_narrow_width() {
        let app = AppState::default();
        let text = build_status_text(
            &chrome_from_app(&app, 10),
            "sample.pdf",
            10,
            &[String::from("SEARCH 1/1")],
            8,
        );
        assert_eq!(text, "p. 1/10");
    }

    #[test]
    fn build_status_text_keeps_page_segment_width_constant() {
        let app9 = AppState {
            current_page: 8,
            ..AppState::default()
        };
        let app10 = AppState {
            current_page: 9,
            ..AppState::default()
        };
        let text9 = build_status_text(&chrome_from_app(&app9, 120), "sample.pdf", 120, &[], 120);
        let text10 = build_status_text(&chrome_from_app(&app10, 120), "sample.pdf", 120, &[], 120);
        assert_eq!(display_width(&text9), display_width(&text10));
        assert!(text9.starts_with("p.  9/120 | zoom 1.00x"));
        assert!(text10.starts_with("p. 10/120 | zoom 1.00x"));
    }

    #[test]
    fn build_status_text_skips_empty_elided_filename_segment() {
        let app = AppState::default();
        let expected = "p.1/7 | zoom 1.00x | SEARCH 10/100";
        let target_width = display_width(expected) + display_width(" | ") + 1;
        let text = build_status_text(
            &chrome_from_app(&app, 7),
            "漢字.pdf",
            7,
            &[String::from("SEARCH 10/100")],
            target_width,
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn build_status_text_uses_spread_page_segment() {
        let app = AppState {
            current_page: 2,
            page_layout_mode: PageLayoutMode::Spread,
            ..AppState::default()
        };
        let text = build_status_text(&chrome_from_app(&app, 10), "sample.pdf", 10, &[], 120);
        assert_eq!(text, "pp. 3- 4/10 | zoom 1.00x | sample.pdf");
    }

    #[test]
    fn build_status_text_uses_single_page_segment_for_cover_solo_page() {
        let app = AppState {
            current_page: 0,
            page_layout_mode: PageLayoutMode::Spread,
            spread_cover_policy: SpreadCoverPolicy::Cover,
            ..AppState::default()
        };

        let text = build_status_text(&chrome_from_app(&app, 10), "sample.pdf", 10, &[], 120);
        assert_eq!(text, "p. 1/10 | zoom 1.00x | sample.pdf");
    }
}
