use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiLayout {
    pub viewer: Rect,
    pub status: Rect,
}

pub fn split_layout(area: Rect, debug_status_visible: bool) -> UiLayout {
    let status_height = if debug_status_visible { 2 } else { 1 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(status_height)])
        .split(area);

    let viewer = chunks[0];
    UiLayout {
        viewer,
        status: chunks[1],
    }
}

pub(crate) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.max(1).min(area.width);
    let height = height.max(1).min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::split_layout;

    #[test]
    fn split_layout_reserves_configured_status_rows() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };

        for (name, debug_status_visible, status_height, viewer_height) in
            [("normal", false, 1, 39), ("debug", true, 2, 38)]
        {
            let layout = split_layout(area, debug_status_visible);
            assert_eq!(layout.status.height, status_height, "{name}");
            assert_eq!(layout.viewer.height, viewer_height, "{name}");
        }
    }

    #[test]
    fn centered_rect_stays_within_area() {
        let area = Rect::new(10, 5, 20, 8);
        let centered = super::centered_rect(area, 99, 99);
        assert_eq!(centered.x, 10);
        assert_eq!(centered.y, 5);
        assert_eq!(centered.width, 20);
        assert_eq!(centered.height, 8);
    }
}
