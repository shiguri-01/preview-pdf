use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::{AppState, Notice, NoticeLevel, PageLayoutMode};

use super::layout::UiLayout;
use super::{border, error_text, primary_text, warning_text};

const MIN_FILENAME_ELISION_WIDTH: usize = 7;

#[allow(clippy::too_many_arguments)]
pub fn draw_chrome(
    frame: &mut Frame<'_>,
    layout: UiLayout,
    app: &AppState,
    file_name: &str,
    page_count: usize,
    presenter_label: &str,
    graphics_protocol: Option<&str>,
    extension_status_segments: &[String],
) {
    let status_text = build_status_text(
        app,
        file_name,
        page_count,
        extension_status_segments,
        layout.status.width as usize,
    );
    let primary = if let Some(notice) = app.notice.as_ref() {
        Paragraph::new(stylize_notice_line(notice, layout.status.width as usize))
            .style(primary_text())
            .wrap(Wrap { trim: true })
    } else {
        Paragraph::new(stylize_status_line(&status_text))
            .style(primary_text())
            .wrap(Wrap { trim: true })
    };
    if app.debug_status_visible && layout.status.height >= 2 {
        let top =
            ratatui::layout::Rect::new(layout.status.x, layout.status.y, layout.status.width, 1);
        frame.render_widget(primary, top);

        let presenter_path_text = build_presenter_path_text(
            presenter_label,
            graphics_protocol,
            layout.status.width as usize,
        );
        let bottom = ratatui::layout::Rect::new(
            layout.status.x,
            layout.status.y + 1,
            layout.status.width,
            layout.status.height.saturating_sub(1).max(1),
        );
        let debug = Paragraph::new(presenter_path_text)
            .style(primary_text())
            .wrap(Wrap { trim: true });
        frame.render_widget(debug, bottom);
        return;
    }

    frame.render_widget(primary, layout.status);
}

fn build_status_text(
    app: &AppState,
    file_name: &str,
    page_count: usize,
    extension_status_segments: &[String],
    max_width: usize,
) -> String {
    let page_total = page_count.max(1);
    let base = format!(
        "{} | zoom {:.2}x",
        format_page_segment(app, page_total),
        app.zoom
    );
    let sep = " | ";

    if max_width == 0 {
        return String::new();
    }

    if display_width(&base) >= max_width {
        return trim_trailing_whitespace(truncate_right_by_width(&base, max_width));
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

    trim_trailing_whitespace(truncate_right_by_width(&base, max_width))
}

fn format_page_segment(app: &AppState, page_total: usize) -> String {
    let slots = app.visible_page_slots(page_total);
    let page_width = page_total.to_string().len();
    match app.page_layout_mode {
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

fn build_presenter_path_text(
    presenter_label: &str,
    graphics_protocol: Option<&str>,
    max_width: usize,
) -> String {
    let protocol = graphics_protocol.unwrap_or("-");
    let text = format!("presenter={presenter_label}(proto={protocol})");
    truncate_right_by_width(&text, max_width)
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
    let text = truncate_right_by_width(&format!("{label}: {}", notice.message), max_width);
    Line::from(vec![Span::styled(text, accent)])
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn trim_trailing_whitespace(input: String) -> String {
    input.trim_end().to_string()
}

fn truncate_right_by_width(input: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if display_width(input) <= max_width {
        return input.to_string();
    }
    let mut out = String::new();
    let mut width = 0;
    for ch in input.chars() {
        let ch_width = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
        if width + ch_width > max_width {
            break;
        }
        width += ch_width;
        out.push(ch);
    }
    out
}

fn suffix_by_width(input: &str, max_width: usize) -> (&str, usize) {
    if max_width == 0 {
        return ("", 0);
    }
    let mut width = 0;
    let mut start = input.len();
    for (idx, ch) in input.char_indices().rev() {
        let ch_width = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
        if width + ch_width > max_width {
            break;
        }
        width += ch_width;
        start = idx;
    }
    (&input[start..], width)
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
        return truncate_right_by_width(input, max_width);
    }

    let content_budget = max_width - ellipsis_width;
    let suffix_budget = (content_budget.saturating_mul(2) / 3).max(1);
    let (suffix, suffix_width) = suffix_by_width(input, suffix_budget);
    let prefix_budget = content_budget.saturating_sub(suffix_width);
    let prefix_limit = input.len().saturating_sub(suffix.len());
    let prefix = truncate_right_by_width(&input[..prefix_limit], prefix_budget);

    if prefix.is_empty() {
        let kept_suffix = truncate_right_by_width(suffix, content_budget);
        return format!("{ELLIPSIS}{kept_suffix}");
    }

    format!("{prefix}{ELLIPSIS}{suffix}")
}

#[cfg(test)]
mod tests {
    use crate::app::{AppState, Notice, NoticeLevel, PageLayoutMode};

    use super::{
        build_presenter_path_text, build_status_text, display_width, format_filename_segment,
        stylize_notice_line,
    };

    #[test]
    fn build_status_text_includes_page_zoom_and_file() {
        let app = AppState {
            current_page: 2,
            zoom: 1.5,
            ..AppState::default()
        };

        let text = build_status_text(&app, "sample.pdf", 10, &[], 80);
        assert_eq!(text, "p. 3/10 | zoom 1.50x | sample.pdf");
    }

    #[test]
    fn build_presenter_path_text_formats_presenter_with_proto() {
        let text = build_presenter_path_text("ratatui-image", Some("kitty"), 200);

        assert_eq!(text, "presenter=ratatui-image(proto=kitty)");
    }

    #[test]
    fn build_presenter_path_text_uses_placeholder_for_unknown_proto() {
        let text = build_presenter_path_text("ratatui-image", None, 200);

        assert_eq!(text, "presenter=ratatui-image(proto=-)");
    }

    #[test]
    fn stylize_notice_line_prefixes_severity() {
        let line = stylize_notice_line(
            &Notice {
                level: NoticeLevel::Error,
                message: "render failed".to_string(),
            },
            80,
        );

        assert_eq!(line.to_string(), "error: render failed");
    }

    #[test]
    fn build_status_text_uses_last_non_empty_extension_segment() {
        let app = AppState::default();
        let text = build_status_text(
            &app,
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
        let text = build_status_text(&app, "very-long-document-name.pdf", 7, &[], 28);
        assert!(text.starts_with("p.1/7 | zoom 1.00x |"));
        assert!(display_width(&text) <= 28);
    }

    #[test]
    fn build_status_text_drops_filename_before_extension() {
        let app = AppState::default();
        let text = build_status_text(
            &app,
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
        let text = build_status_text(&app, "sample.pdf", 10, &[String::from("SEARCH 1/1")], 8);
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
        let text9 = build_status_text(&app9, "sample.pdf", 120, &[], 120);
        let text10 = build_status_text(&app10, "sample.pdf", 120, &[], 120);
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
            &app,
            "漢字.pdf",
            7,
            &[String::from("SEARCH 10/100")],
            target_width,
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn format_filename_segment_keeps_short_name_when_it_fits_under_threshold() {
        assert_eq!(format_filename_segment("a.pdf", 5), "a.pdf");
    }

    #[test]
    fn format_filename_segment_drops_long_name_below_elision_threshold() {
        assert_eq!(format_filename_segment("very-long-document-name.pdf", 5), "");
        assert_eq!(format_filename_segment("very-long-document-name.pdf", 6), "");
    }

    #[test]
    fn format_filename_segment_elides_long_name_at_threshold() {
        assert_eq!(format_filename_segment("very-long-document-name.pdf", 7), "ve….pdf");
    }

    #[test]
    fn build_status_text_uses_spread_page_segment() {
        let app = AppState {
            current_page: 2,
            page_layout_mode: PageLayoutMode::Spread,
            ..AppState::default()
        };
        let text = build_status_text(&app, "sample.pdf", 10, &[], 120);
        assert_eq!(text, "pp. 3- 4/10 | zoom 1.00x | sample.pdf");
    }
}
