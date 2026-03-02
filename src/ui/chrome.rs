use ratatui::Frame;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::AppState;
use crate::perf::PerfStats;

use super::layout::UiLayout;

#[allow(clippy::too_many_arguments)]
pub fn draw_chrome(
    frame: &mut Frame<'_>,
    layout: UiLayout,
    app: &AppState,
    file_name: &str,
    page_count: usize,
    perf: &PerfStats,
    presenter_backend: &str,
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

    let status = Paragraph::new(stylize_status_line(&status_text))
        .style(Style::default())
        .wrap(Wrap { trim: true });
    if app.debug_status_visible && layout.status.height >= 2 {
        let top =
            ratatui::layout::Rect::new(layout.status.x, layout.status.y, layout.status.width, 1);
        frame.render_widget(status, top);

        let command_id = app
            .status
            .last_action_id
            .map(|id| id.as_str())
            .unwrap_or("-");
        let message = if app.status.message.is_empty() {
            "-"
        } else {
            app.status.message.as_str()
        };
        let protocol = graphics_protocol.unwrap_or("-");
        let debug_text = format!(
            "cmd={command_id} | msg={message} | perf=r{:.1} c{:.1} b{:.1} | q={} | hit=l1 {:.0}% l2 {:.0}% | presenter={} | proto={}",
            perf.render_ms,
            perf.convert_ms,
            perf.blit_ms,
            perf.queue_depth,
            perf.cache_hit_rate_l1 * 100.0,
            perf.cache_hit_rate_l2 * 100.0,
            presenter_backend,
            protocol
        );
        let bottom = ratatui::layout::Rect::new(
            layout.status.x,
            layout.status.y + 1,
            layout.status.width,
            layout.status.height.saturating_sub(1).max(1),
        );
        let debug = Paragraph::new(debug_text)
            .style(Style::default())
            .wrap(Wrap { trim: true });
        frame.render_widget(debug, bottom);
        return;
    }

    frame.render_widget(status, layout.status);
}

fn build_status_text(
    app: &AppState,
    file_name: &str,
    page_count: usize,
    extension_status_segments: &[String],
    max_width: usize,
) -> String {
    let page_total = page_count.max(1);
    let page_now = app.current_page.saturating_add(1).min(page_total);
    let page_width = page_total.to_string().len();
    let base = format!(
        "p. {:>page_width$}/{:>page_width$} | Zoom {:.2}x",
        page_now, page_total, app.zoom
    );
    let sep = " | ";

    if max_width == 0 {
        return String::new();
    }

    if display_width(&base) >= max_width {
        return truncate_right_by_width(&base, max_width);
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
                if filename_budget > 0 {
                    let filename = elide_middle_by_width(file_name, filename_budget);
                    return format!("{base}{sep}{filename}{sep}{ext_text}");
                }
            }
            return format!("{base}{sep}{ext_text}");
        }
    }

    let fixed_with_filename = display_width(&base) + display_width(sep);
    if fixed_with_filename < max_width {
        let filename_budget = max_width - fixed_with_filename;
        let filename = elide_middle_by_width(file_name, filename_budget);
        return format!("{base}{sep}{filename}");
    }

    truncate_right_by_width(&base, max_width)
}

fn stylize_status_line(text: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (idx, part) in text.split(" | ").enumerate() {
        if idx > 0 {
            spans.push(Span::styled(
                " | ".to_string(),
                Style::default().fg(Color::DarkGray),
            ));
        }
        spans.push(Span::raw(part.to_string()));
    }
    Line::from(spans)
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
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

fn elide_middle_by_width(input: &str, max_width: usize) -> String {
    const ELLIPSIS: &str = "...";
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
    use crate::app::AppState;

    use super::{build_status_text, display_width};

    #[test]
    fn build_status_text_includes_page_zoom_and_file() {
        let app = AppState {
            current_page: 2,
            zoom: 1.5,
            ..AppState::default()
        };

        let text = build_status_text(&app, "sample.pdf", 10, &[], 80);
        assert_eq!(text, "p.  3/10 | Zoom 1.50x | sample.pdf");
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
        assert_eq!(text, "p. 1/5 | Zoom 1.00x | sample.pdf | HISTORY 1/3");
    }

    #[test]
    fn build_status_text_elides_filename_in_middle_on_tight_width() {
        let app = AppState::default();
        let text = build_status_text(&app, "very-long-document-name.pdf", 7, &[], 28);
        assert!(text.starts_with("p. 1/7 | Zoom 1.00x | "));
        assert!(text.contains("..."));
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
        assert_eq!(text, "p. 1/7 | Zoom 1.00x | SEARCH 10/100");
    }

    #[test]
    fn build_status_text_handles_very_narrow_width() {
        let app = AppState::default();
        let text = build_status_text(&app, "sample.pdf", 10, &[String::from("SEARCH 1/1")], 8);
        assert_eq!(text, "p.  1/10");
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
        assert!(text9.starts_with("p.   9/120 | Zoom 1.00x"));
        assert!(text10.starts_with("p.  10/120 | Zoom 1.00x"));
    }
}
