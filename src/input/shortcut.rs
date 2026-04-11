use crossterm::event::{KeyCode, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutKey {
    code: KeyCode,
    modifiers: KeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutKeyError {
    UnsupportedModifiers,
}

impl ShortcutKey {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self::try_new(code, modifiers)
            .expect("ShortcutKey does not support SUPER, HYPER, or META modifiers")
    }

    pub fn try_new(code: KeyCode, modifiers: KeyModifiers) -> Result<Self, ShortcutKeyError> {
        validate_modifiers(modifiers)?;
        Ok(Self { code, modifiers })
    }

    pub fn key(code: KeyCode) -> Self {
        Self::new(code, KeyModifiers::NONE)
    }

    pub fn ctrl(ch: char) -> Self {
        Self::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    pub fn alt(ch: char) -> Self {
        Self::new(KeyCode::Char(ch), KeyModifiers::ALT)
    }

    pub fn char(ch: char) -> Self {
        Self::key(KeyCode::Char(ch))
    }

    pub fn code(self) -> KeyCode {
        self.code
    }

    pub fn modifiers(self) -> KeyModifiers {
        self.modifiers
    }
}

pub fn format_shortcut_key(key: ShortcutKey) -> String {
    let is_back_tab = key.code() == KeyCode::BackTab;

    if let KeyCode::Char(ch) = key.code()
        && !key
            .modifiers()
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT)
        && ch != ' '
    {
        return ch.to_string();
    }

    let has_modifier = is_back_tab
        || key
            .modifiers()
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
    let key_text = if is_back_tab {
        "tab".to_string()
    } else {
        base_key_text(key)
    };
    if !has_modifier {
        return format!("<{key_text}>");
    }

    let mut modifiers = Vec::new();
    if key.modifiers().contains(KeyModifiers::CONTROL) {
        modifiers.push("c");
    }
    if key.modifiers().contains(KeyModifiers::ALT) {
        modifiers.push("m");
    }
    if is_back_tab || key.modifiers().contains(KeyModifiers::SHIFT) {
        modifiers.push("s");
    }

    format!("<{}-{key_text}>", modifiers.join("-"))
}

fn validate_modifiers(modifiers: KeyModifiers) -> Result<(), ShortcutKeyError> {
    if modifiers.intersects(KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META) {
        return Err(ShortcutKeyError::UnsupportedModifiers);
    }
    Ok(())
}

pub fn format_shortcut_sequence(keys: &[ShortcutKey]) -> String {
    format_shortcut_keys(keys, "")
}

pub fn format_shortcut_alternatives(keys: &[ShortcutKey]) -> String {
    format_shortcut_keys(keys, " / ")
}

pub fn format_shortcut_alternatives_tight(keys: &[ShortcutKey]) -> String {
    format_shortcut_keys(keys, "/")
}

fn format_shortcut_keys(keys: &[ShortcutKey], separator: &str) -> String {
    keys.iter()
        .map(|key| format_shortcut_key(*key))
        .collect::<Vec<_>>()
        .join(separator)
}

fn base_key_text(key: ShortcutKey) -> String {
    match key.code() {
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pgup".to_string(),
        KeyCode::PageDown => "pgdn".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Delete => "del".to_string(),
        KeyCode::Insert => "ins".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(ch) => ch.to_ascii_lowercase().to_string(),
        _ => format!("{:?}", key.code()).to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::{
        ShortcutKey, ShortcutKeyError, format_shortcut_alternatives,
        format_shortcut_alternatives_tight, format_shortcut_key, format_shortcut_sequence,
    };

    #[test]
    fn formats_regular_and_modified_keys() {
        assert_eq!(format_shortcut_key(ShortcutKey::ctrl('o')), "<c-o>");
        assert_eq!(format_shortcut_key(ShortcutKey::char(' ')), "<space>");
        assert_eq!(format_shortcut_key(ShortcutKey::char('?')), "?");
        assert_eq!(format_shortcut_key(ShortcutKey::char('A')), "A");
        assert_eq!(
            format_shortcut_key(ShortcutKey::key(KeyCode::PageDown)),
            "<pgdn>"
        );
        assert_eq!(
            format_shortcut_key(ShortcutKey::key(KeyCode::CapsLock)),
            "<capslock>"
        );
        assert_eq!(
            format_shortcut_key(ShortcutKey::new(KeyCode::Char('O'), KeyModifiers::CONTROL)),
            "<c-o>"
        );
        assert_eq!(
            format_shortcut_key(ShortcutKey::new(KeyCode::Char('x'), KeyModifiers::ALT)),
            "<m-x>"
        );
        assert_eq!(format_shortcut_key(ShortcutKey::key(KeyCode::Esc)), "<esc>");
        assert_eq!(
            format_shortcut_key(ShortcutKey::key(KeyCode::Enter)),
            "<enter>"
        );
        assert_eq!(
            format_shortcut_key(ShortcutKey::key(KeyCode::BackTab)),
            "<s-tab>"
        );
        assert_eq!(
            format_shortcut_key(ShortcutKey::new(KeyCode::BackTab, KeyModifiers::CONTROL)),
            "<c-s-tab>"
        );
        assert_eq!(
            format_shortcut_key(ShortcutKey::new(KeyCode::BackTab, KeyModifiers::ALT)),
            "<m-s-tab>"
        );
        assert_eq!(
            format_shortcut_key(ShortcutKey::new(
                KeyCode::BackTab,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            "<c-s-tab>"
        );
        assert_eq!(
            format_shortcut_key(ShortcutKey::new(
                KeyCode::BackTab,
                KeyModifiers::ALT | KeyModifiers::SHIFT
            )),
            "<m-s-tab>"
        );
    }

    #[test]
    fn formats_shortcut_sequences() {
        let text = format_shortcut_sequence(&[ShortcutKey::char('j'), ShortcutKey::char('k')]);
        assert_eq!(text, "jk");
    }

    #[test]
    fn formats_shortcut_alternatives() {
        let text = format_shortcut_alternatives(&[ShortcutKey::ctrl('p'), ShortcutKey::ctrl('n')]);
        assert_eq!(text, "<c-p> / <c-n>");
    }

    #[test]
    fn formats_tight_shortcut_alternatives() {
        let text =
            format_shortcut_alternatives_tight(&[ShortcutKey::ctrl('p'), ShortcutKey::ctrl('n')]);
        assert_eq!(text, "<c-p>/<c-n>");
    }

    #[test]
    fn try_new_rejects_unsupported_modifiers() {
        assert_eq!(
            ShortcutKey::try_new(KeyCode::Char('k'), KeyModifiers::SUPER),
            Err(ShortcutKeyError::UnsupportedModifiers)
        );
    }
}
