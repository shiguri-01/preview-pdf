use ratatui::Frame;
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::{AppState, Mode};
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
) {
    let mode = match app.mode {
        Mode::Normal => "NORMAL",
        Mode::Palette => "PALETTE",
    };

    let page_total = page_count.max(1);
    let page_now = app.current_page.saturating_add(1).min(page_total);

    let status_text = format!(
        "{} | page {}/{} | zoom {:.2}x | {}",
        file_name, page_now, page_total, app.zoom, mode
    );

    let status = Paragraph::new(status_text.clone())
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
