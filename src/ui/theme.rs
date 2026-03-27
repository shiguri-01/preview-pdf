use ratatui::style::{Color, Modifier, Style};

pub fn primary_text() -> Style {
    Style::default()
}

pub fn secondary_text() -> Style {
    Style::default().fg(Color::Gray)
}

pub fn heading_text() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn border() -> Style {
    Style::default().fg(Color::Gray)
}

pub fn warning_text() -> Style {
    Style::default().fg(Color::LightYellow)
}

pub fn error_text() -> Style {
    Style::default().fg(Color::LightRed)
}
