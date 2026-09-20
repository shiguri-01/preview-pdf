use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{AppError, AppResult};

use super::options::{
    AppOptions, KeymapOptions, KeymapPreset, RenderOptions, ViewOptions, WatchOptions,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFileSelection {
    Default,
    Path(PathBuf),
    Disabled,
}

impl ConfigFileSelection {
    pub fn load_options(&self) -> AppResult<AppOptions> {
        match self {
            Self::Default => load_default_app_options(),
            Self::Path(path) => load_options_from_explicit_path(path),
            Self::Disabled => Ok(AppOptions::default()),
        }
    }
}

pub fn load_default_app_options() -> AppResult<AppOptions> {
    let Some(path) = default_config_path() else {
        return Ok(AppOptions::default());
    };
    load_options_from_path_allow_missing(path)
}

pub fn load_options_from_explicit_path(path: impl AsRef<Path>) -> AppResult<AppOptions> {
    read_options_from_path(path.as_ref(), MissingConfigPolicy::Error)
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default)]
struct RawConfig {
    render: RenderOptions,
    view: ViewOptions,
    keymap_preset: Option<KeymapPreset>,
    keymap: Option<Vec<RawKeymapEntry>>,
    watch: WatchOptions,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct RawKeymapEntry {
    when: String,
    key: String,
    command: RawKeymapCommand,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum RawKeymapCommand {
    Command(String),
    Unbind(bool),
}

impl RawConfig {
    fn into_options(self) -> AppResult<AppOptions> {
        Ok(AppOptions {
            render: self.render,
            view: self.view,
            keymap: parse_keymap_options(self.keymap_preset, self.keymap)?,
            watch: self.watch,
        })
    }
}

fn parse_keymap_options(
    preset: Option<KeymapPreset>,
    entries: Option<Vec<RawKeymapEntry>>,
) -> AppResult<KeymapOptions> {
    Ok(KeymapOptions {
        preset,
        bindings: entries
            .unwrap_or_default()
            .iter()
            .map(parse_keymap_entry)
            .collect::<AppResult<Vec<_>>>()?,
    })
}

fn parse_keymap_entry(entry: &RawKeymapEntry) -> AppResult<super::keymap::KeymapBinding> {
    let command = match &entry.command {
        RawKeymapCommand::Command(command) => Some(command.as_str()),
        RawKeymapCommand::Unbind(false) => None,
        RawKeymapCommand::Unbind(true) => {
            return Err(AppError::invalid_argument(
                "keymap command must be a command string or false",
            ));
        }
    };
    super::keymap::parse_keymap_binding(&entry.when, &entry.key, command)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MissingConfigPolicy {
    Default,
    Error,
}

fn load_options_from_path_allow_missing(path: impl AsRef<Path>) -> AppResult<AppOptions> {
    read_options_from_path(path.as_ref(), MissingConfigPolicy::Default)
}

fn read_options_from_path(path: &Path, missing: MissingConfigPolicy) -> AppResult<AppOptions> {
    if !path.exists() {
        return match missing {
            MissingConfigPolicy::Default => Ok(AppOptions::default()),
            MissingConfigPolicy::Error => Err(AppError::invalid_argument(format!(
                "config path does not exist: {}",
                path.display()
            ))),
        };
    }
    if !path.is_file() {
        return Err(AppError::invalid_argument(format!(
            "config path is not a regular file: {}",
            path.display()
        )));
    }

    let raw = fs::read_to_string(path).map_err(|source| {
        AppError::io_with_context(source, format!("failed to read config: {}", path.display()))
    })?;
    let parsed = toml::from_str::<RawConfig>(&raw).map_err(|source| {
        AppError::invalid_argument(format!(
            "failed to parse config {}: {source}",
            path.display()
        ))
    })?;
    parsed.into_options()
}

pub fn default_config_path() -> Option<PathBuf> {
    default_config_path_from_env(|key| std::env::var_os(key), Path::is_file)
}

fn default_config_path_from_env(
    mut env_var: impl FnMut(&str) -> Option<OsString>,
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(explicit) = env_var("PVF_CONFIG_PATH")
        && !explicit.is_empty()
    {
        return Some(PathBuf::from(explicit));
    }

    if let Some(xdg) = env_var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        let path = PathBuf::from(xdg).join("pvf").join("config.toml");
        if is_file(&path) {
            return Some(path);
        }
    }
    if let Some(home) = env_var("HOME")
        && !home.is_empty()
    {
        let path = PathBuf::from(home)
            .join(".config")
            .join("pvf")
            .join("config.toml");
        if is_file(&path) {
            return Some(path);
        }
    }
    if let Some(appdata) = env_var("APPDATA")
        && !appdata.is_empty()
    {
        let path = PathBuf::from(appdata).join("pvf").join("config.toml");
        if is_file(&path) {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
    use crate::command::Command;
    use crate::config::{AppOptionsResolver, KeymapBinding, KeymapPreset, KeymapWhen};
    use crate::extension::ExtensionUiSnapshot;
    use crate::input::sequence::{
        DEFAULT_SEQUENCE_TIMEOUT, KeyBindingContext, SequenceResolution, SequenceResolver,
    };
    use crate::input::shortcut::ShortcutKey;
    use crate::presenter::GraphicsProtocol;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{
        ConfigFileSelection, default_config_path_from_env, load_options_from_explicit_path,
    };

    fn handle_normal_key(resolver: &mut SequenceResolver, key: KeyEvent) -> SequenceResolution {
        let extensions = ExtensionUiSnapshot::default();
        resolver.handle_key_in_context(KeyBindingContext::normal(&extensions), key)
    }

    fn unique_temp_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("pvf_config_{suffix}_{}_{}", process::id(), nanos));
        path
    }

    #[test]
    fn explicit_config_reads_keymap_entries() {
        let path = unique_temp_path("keymap.toml");
        fs::write(
            &path,
            r#"
            [[keymap]]
            when = "normal"
            key = "<down>"
            command = "next-page"

            [[keymap]]
            when = "normal"
            key = "[count]G"
            command = "goto-page"

            [[keymap]]
            when = "normal"
            key = "j"
            command = false
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("config should parse");
        assert_eq!(
            options.keymap.bindings,
            vec![
                KeymapBinding::Exact {
                    when: KeymapWhen::Normal,
                    keys: vec![ShortcutKey::key(KeyCode::Down)],
                    command: Command::NextPage,
                },
                KeymapBinding::NumericPrefix {
                    when: KeymapWhen::Normal,
                    suffix: ShortcutKey::char('G'),
                    command_id: "goto-page",
                },
                KeymapBinding::UnbindExact {
                    when: KeymapWhen::Normal,
                    keys: vec![ShortcutKey::char('j')],
                },
            ]
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_resolves_to_runtime_sequence_registry() {
        let path = unique_temp_path("keymap-runtime.toml");
        fs::write(
            &path,
            r#"
            [[keymap]]
            when = "normal"
            key = "<down>"
            command = "next-page"

            [[keymap]]
            when = "normal"
            key = "j"
            command = false
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("config should parse");
        let resolved = AppOptionsResolver::new()
            .apply_options(options)
            .resolve()
            .expect("keymap options should resolve");
        let mut resolver =
            SequenceResolver::new(resolved.input.sequence_registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            handle_normal_key(
                &mut resolver,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            SequenceResolution::Noop
        );
        assert_eq!(
            handle_normal_key(
                &mut resolver,
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
            ),
            SequenceResolution::Dispatch(Command::NextPage)
        );
        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_preset_none_starts_from_empty_keymap() {
        let path = unique_temp_path("keymap-preset-none.toml");
        fs::write(
            &path,
            r#"
            keymap_preset = "none"

            [[keymap]]
            when = "normal"
            key = "x"
            command = "next-page"
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("config should parse");
        assert_eq!(options.keymap.preset, Some(KeymapPreset::None));

        let resolved = AppOptionsResolver::new()
            .apply_options(options)
            .resolve()
            .expect("keymap options should resolve");
        let mut resolver =
            SequenceResolver::new(resolved.input.sequence_registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            handle_normal_key(
                &mut resolver,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            SequenceResolution::Noop
        );
        assert_eq!(
            handle_normal_key(
                &mut resolver,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            SequenceResolution::Dispatch(Command::NextPage)
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_allows_literal_less_than_binding() {
        let path = unique_temp_path("keymap-less-than.toml");
        fs::write(
            &path,
            r#"
            [[keymap]]
            when = "normal"
            key = "<"
            command = "prev-page"
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("config should parse");
        let resolved = AppOptionsResolver::new()
            .apply_options(options)
            .resolve()
            .expect("keymap options should resolve");
        let mut resolver =
            SequenceResolver::new(resolved.input.sequence_registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            handle_normal_key(
                &mut resolver,
                KeyEvent::new(KeyCode::Char('<'), KeyModifiers::NONE)
            ),
            SequenceResolution::Dispatch(Command::PrevPage)
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_rejects_unknown_when() {
        let path = unique_temp_path("bad-keymap-when.toml");
        fs::write(
            &path,
            r#"
            [[keymap]]
            when = "emacs"
            key = "x"
            command = "next-page"
            "#,
        )
        .expect("config file should be written");

        let err = load_options_from_explicit_path(&path).expect_err("config should be rejected");
        assert!(
            err.to_string().contains("unknown keymap condition"),
            "unexpected error: {err}"
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn explicit_config_rejects_unknown_enum_values() {
        for (name, source, field, value) in [
            (
                "keymap-preset",
                r#"keymap_preset = "bob""#,
                "keymap_preset",
                "bob",
            ),
            (
                "graphics-protocol",
                r#"
                [render]
                graphics_protocol = "graphics-protocol"
                "#,
                "graphics_protocol",
                "graphics-protocol",
            ),
            (
                "view-layout",
                r#"
                [view]
                initial_layout = "grid"
                "#,
                "initial_layout",
                "grid",
            ),
        ] {
            let path = unique_temp_path(&format!("bad-{name}.toml"));
            fs::write(&path, source).expect("config file should be written");

            let err =
                load_options_from_explicit_path(&path).expect_err("config should be rejected");
            let message = err.to_string();
            assert!(message.contains(field), "{name}: {message}");
            assert!(
                message.contains(&format!("unknown variant `{value}`")),
                "{name}: {message}"
            );

            fs::remove_file(&path).expect("config file should be removed");
        }
    }

    #[test]
    fn keymap_config_rejects_legacy_table_shape() {
        let path = unique_temp_path("legacy-keymap.toml");
        fs::write(
            &path,
            r#"
            [keymap]
            preset = "none"

            [keymap.bindings]
            "j" = "next-page"
            "#,
        )
        .expect("config file should be written");

        let err = load_options_from_explicit_path(&path).expect_err("config should be rejected");
        assert!(
            err.to_string().contains("invalid"),
            "unexpected error: {err}"
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_rejects_true_command_value() {
        let path = unique_temp_path("true-keymap-command.toml");
        fs::write(
            &path,
            r#"
            [[keymap]]
            when = "normal"
            key = "j"
            command = true
            "#,
        )
        .expect("config file should be written");

        let err = load_options_from_explicit_path(&path).expect_err("config should be rejected");
        assert!(
            err.to_string()
                .contains("command must be a command string or false"),
            "unexpected error: {err}"
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_requires_command() {
        let path = unique_temp_path("missing-keymap-action.toml");
        fs::write(
            &path,
            r#"
            [[keymap]]
            when = "normal"
            key = "j"
            "#,
        )
        .expect("config file should be written");

        let err = load_options_from_explicit_path(&path).expect_err("config should be rejected");
        assert!(
            err.to_string().contains("missing field `command`"),
            "unexpected error: {err}"
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_rejects_internal_only_commands() {
        let path = unique_temp_path("bad-keymap-command.toml");
        fs::write(
            &path,
            r#"
            [[keymap]]
            when = "normal"
            key = "x"
            command = "submit-search needle"
            "#,
        )
        .expect("config file should be written");

        let err = load_options_from_explicit_path(&path).expect_err("config should be rejected");
        assert!(
            err.to_string().contains("internal command"),
            "unexpected error: {err}"
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_rejects_surface_commands_with_non_app_targets() {
        for (command, expected) in [
            ("palette.submit", "requires an active palette"),
            ("close-help", "requires active help"),
        ] {
            let path = unique_temp_path(&format!("bad-keymap-surface-{command}.toml"));
            fs::write(
                &path,
                format!(
                    r#"
                    [[keymap]]
                    when = "normal"
                    key = "x"
                    command = "{command}"
                    "#
                ),
            )
            .expect("config file should be written");

            let err =
                load_options_from_explicit_path(&path).expect_err("config should be rejected");
            let message = err.to_string();
            assert!(
                message.contains(command) && message.contains(expected),
                "unexpected error for {command}: {err}"
            );

            fs::remove_file(&path).expect("config file should be removed");
        }
    }

    #[test]
    fn keymap_config_accepts_special_key_bindings() {
        for (name, key, command_name, key_code, command) in [
            ("escape", "<esc>", "quit", KeyCode::Esc, Command::Quit),
            (
                "enter",
                "<enter>",
                "next-page",
                KeyCode::Enter,
                Command::NextPage,
            ),
        ] {
            let path = unique_temp_path(&format!("{name}-keymap-key.toml"));
            fs::write(
                &path,
                format!(
                    r#"
                    [[keymap]]
                    when = "normal"
                    key = "{key}"
                    command = "{command_name}"
                    "#
                ),
            )
            .expect("config file should be written");

            let options = load_options_from_explicit_path(&path).expect("config should load");
            assert_eq!(
                options.keymap.bindings,
                vec![KeymapBinding::Exact {
                    when: KeymapWhen::Normal,
                    keys: vec![ShortcutKey::key(key_code)],
                    command,
                }]
            );

            fs::remove_file(&path).expect("config file should be removed");
        }
    }

    #[test]
    fn explicit_config_preserves_unspecified_fields_as_absent_options() {
        let path = unique_temp_path("partial-options.toml");
        fs::write(
            &path,
            r#"
            [render]
            graphics_protocol = "auto"
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("options should parse");
        assert_eq!(
            options.render.graphics_protocol,
            Some(GraphicsProtocol::Auto)
        );
        assert_eq!(options.view.initial_page, None);
        assert!(options.keymap.bindings.is_empty());
        assert_eq!(options.watch.enabled, None);

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn explicit_config_reads_graphics_protocols() {
        for (value, expected) in [
            ("sixel", GraphicsProtocol::Sixel),
            ("auto", GraphicsProtocol::Auto),
        ] {
            let path = unique_temp_path(&format!("{value}-graphics-protocol.toml"));
            fs::write(
                &path,
                format!(
                    r#"
                    [render]
                    graphics_protocol = "{value}"
                    "#
                ),
            )
            .expect("config file should be written");

            let options = load_options_from_explicit_path(&path).expect("config should load");
            assert_eq!(options.render.graphics_protocol, Some(expected));

            fs::remove_file(&path).expect("config file should be removed");
        }
    }

    #[test]
    fn keymap_config_accepts_palette_input_state_bindings() {
        let path = unique_temp_path("palette-input-state-keymap.toml");
        fs::write(
            &path,
            r#"
            [[keymap]]
            when = "palette.input-empty"
            key = "<backspace>"
            command = "close-palette"

            [[keymap]]
            when = "palette.input-not-empty"
            key = "<backspace>"
            command = "text.delete-backward"
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("config should load");
        assert_eq!(
            options.keymap.bindings,
            vec![
                KeymapBinding::Exact {
                    when: KeymapWhen::PaletteInputEmpty,
                    keys: vec![ShortcutKey::key(KeyCode::Backspace)],
                    command: Command::ClosePalette,
                },
                KeymapBinding::Exact {
                    when: KeymapWhen::PaletteInputNotEmpty,
                    keys: vec![ShortcutKey::key(KeyCode::Backspace)],
                    command: Command::TextDeleteBackward,
                },
            ]
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn explicit_config_reads_view_input_and_watch_sections() {
        let path = unique_temp_path("view-input-watch-options.toml");
        fs::write(
            &path,
            r#"
            [view]
            initial_page = 8
            initial_zoom = 1.5
            initial_layout = "spread"
            spread_direction = "rtl"
            spread_cover = "cover"

            [watch]
            enabled = true
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("options should parse");
        assert_eq!(options.view.initial_page, Some(8));
        assert_eq!(options.view.initial_zoom, Some(1.5));
        assert_eq!(options.view.initial_layout, Some(PageLayoutMode::Spread));
        assert_eq!(options.view.spread_direction, Some(SpreadDirection::Rtl));
        assert_eq!(options.view.spread_cover, Some(SpreadCoverPolicy::Cover));
        assert_eq!(options.watch.enabled, Some(true));

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn explicit_config_rejects_missing_path() {
        let missing = unique_temp_path("missing-explicit.toml");
        let err = load_options_from_explicit_path(&missing)
            .expect_err("explicit missing config should fail");
        assert!(
            err.to_string().contains("config path does not exist"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn file_selection_disabled_returns_empty_options() {
        let options = ConfigFileSelection::Disabled
            .load_options()
            .expect("disabled config should not read files");

        assert_eq!(options, super::AppOptions::default());
    }

    #[test]
    fn file_selection_path_requires_existing_file() {
        let missing = unique_temp_path("missing-selection.toml");
        let err = ConfigFileSelection::Path(missing)
            .load_options()
            .expect_err("explicit selection should reject missing paths");

        assert!(
            err.to_string().contains("config path does not exist"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn default_lookup_keeps_explicit_path_even_when_missing() {
        let explicit = PathBuf::from("/tmp/pvf-explicit-config.toml");
        let found = default_config_path_from_env(
            |key| (key == "PVF_CONFIG_PATH").then(|| OsString::from(&explicit)),
            |_| false,
        );

        assert_eq!(found, Some(explicit));
    }

    #[test]
    fn default_lookup_falls_through_missing_implicit_locations() {
        let xdg = PathBuf::from("/tmp/pvf-xdg-config");
        let home = PathBuf::from("/tmp/pvf-home");
        let appdata = PathBuf::from("/tmp/pvf-appdata");
        let expected = home.join(".config").join("pvf").join("config.toml");

        let found = default_config_path_from_env(
            |key| match key {
                "XDG_CONFIG_HOME" => Some(OsString::from(&xdg)),
                "HOME" => Some(OsString::from(&home)),
                "APPDATA" => Some(OsString::from(&appdata)),
                _ => None,
            },
            |path| path == expected,
        );

        assert_eq!(found, Some(expected));
    }
}
