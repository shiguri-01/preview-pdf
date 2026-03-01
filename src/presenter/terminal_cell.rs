use crossterm::terminal;
use ratatui_image::picker::{Capability, Picker, ProtocolType};

pub(crate) fn picker_with_resolved_cell_size(
    picker: Picker,
    protocol_type: ProtocolType,
) -> Picker {
    let current = picker.font_size();
    let resolved = resolve_cell_size_px(&picker).unwrap_or(current);
    if resolved == current {
        return picker;
    }

    #[allow(deprecated)]
    let mut rebuilt = Picker::from_fontsize(resolved);
    rebuilt.set_protocol_type(protocol_type);
    rebuilt
}

fn resolve_cell_size_px(picker: &Picker) -> Option<(u16, u16)> {
    cell_size_from_window_size().or_else(|| cell_size_from_picker_capabilities(picker))
}

fn cell_size_from_picker_capabilities(picker: &Picker) -> Option<(u16, u16)> {
    picker.capabilities().iter().find_map(|cap| match cap {
        Capability::CellSize(Some((width, height))) if *width > 0 && *height > 0 => {
            Some((*width, *height))
        }
        _ => None,
    })
}

fn cell_size_from_window_size() -> Option<(u16, u16)> {
    let window = terminal::window_size().ok()?;
    cell_size_from_window_metrics(window.width, window.height, window.columns, window.rows)
}

pub(crate) fn cell_size_from_window_metrics(
    width_px: u16,
    height_px: u16,
    columns: u16,
    rows: u16,
) -> Option<(u16, u16)> {
    if width_px == 0 || height_px == 0 || columns == 0 || rows == 0 {
        return None;
    }
    let cell_width = width_px / columns;
    let cell_height = height_px / rows;
    if cell_width == 0 || cell_height == 0 {
        return None;
    }
    Some((cell_width, cell_height))
}

pub(crate) fn protocol_type_label(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Halfblocks => "halfblocks",
        ProtocolType::Sixel => "sixel",
        ProtocolType::Kitty => "kitty",
        ProtocolType::Iterm2 => "iterm2",
    }
}
