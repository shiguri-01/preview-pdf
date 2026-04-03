use crossterm::event::{KeyCode, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutKey {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl ShortcutKey {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub const fn key(code: KeyCode) -> Self {
        Self::new(code, KeyModifiers::NONE)
    }

    pub const fn ctrl(ch: char) -> Self {
        Self::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    pub const fn alt(ch: char) -> Self {
        Self::new(KeyCode::Char(ch), KeyModifiers::ALT)
    }

    pub const fn char(ch: char) -> Self {
        Self::key(KeyCode::Char(ch))
    }
}

pub fn format_shortcut_key(key: ShortcutKey) -> String {
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift");
    }

    let key_text = match key.code {
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Char(ch) => {
            if (key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT)
                || key.modifiers.contains(KeyModifiers::SHIFT))
                && ch.is_ascii_alphabetic()
            {
                ch.to_ascii_uppercase().to_string()
            } else {
                ch.to_string()
            }
        }
        _ => format!("{:?}", key.code),
    };

    if parts.is_empty() {
        key_text
    } else {
        let mut text = parts.join("+");
        text.push('+');
        text.push_str(&key_text);
        text
    }
}

pub fn format_shortcut_sequence(keys: &[ShortcutKey]) -> String {
    keys.iter()
        .map(|key| format_shortcut_key(*key))
        .collect::<Vec<_>>()
        .join(" / ")
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::{ShortcutKey, format_shortcut_key, format_shortcut_sequence};

    #[test]
    fn formats_regular_and_modified_keys() {
        assert_eq!(format_shortcut_key(ShortcutKey::ctrl('o')), "Ctrl+O");
        assert_eq!(format_shortcut_key(ShortcutKey::char('?')), "?");
        assert_eq!(format_shortcut_key(ShortcutKey::char('A')), "A");
        assert_eq!(
            format_shortcut_key(ShortcutKey::key(KeyCode::PageDown)),
            "PgDn"
        );
        assert_eq!(
            format_shortcut_key(ShortcutKey::new(KeyCode::Char('O'), KeyModifiers::CONTROL)),
            "Ctrl+O"
        );
    }

    #[test]
    fn formats_shortcut_sequences() {
        let text = format_shortcut_sequence(&[ShortcutKey::char('j'), ShortcutKey::char('k')]);
        assert_eq!(text, "j / k");
    }
}
