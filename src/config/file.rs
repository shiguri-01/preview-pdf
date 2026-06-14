use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
use crate::error::{AppError, AppResult};

use super::options::{
    AppOptions, CacheOptions, InputOptions, KeymapOptions, RenderOptions, ViewOptions, WatchOptions,
};
use super::policy::AppOptionsResolver;
use super::types::Config;

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

impl Config {
    pub fn load() -> AppResult<Self> {
        let Some(path) = default_config_path() else {
            return Ok(Self::default());
        };
        Self::load_from_path(path)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> AppResult<Self> {
        let options = read_options_from_path(path.as_ref(), MissingConfigPolicy::Default)?;
        Ok(AppOptionsResolver::new()
            .apply_options(options)
            .resolve()
            .into())
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default)]
struct RawConfig {
    render: Option<RawRenderConfig>,
    cache: Option<RawCacheConfig>,
    view: Option<RawViewConfig>,
    input: Option<RawInputConfig>,
    keymap: Option<RawKeymapConfig>,
    watch: Option<RawWatchConfig>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default)]
struct RawRenderConfig {
    worker_threads: Option<usize>,
    input_poll_timeout_idle_ms: Option<u64>,
    input_poll_timeout_busy_ms: Option<u64>,
    prefetch_pause_ms: Option<u64>,
    prefetch_tick_ms: Option<u64>,
    pending_redraw_interval_ms: Option<u64>,
    prefetch_dispatch_budget_per_tick: Option<usize>,
    max_render_scale: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
struct RawCacheConfig {
    l1_memory_budget_mb: Option<usize>,
    l2_memory_budget_mb: Option<usize>,
    l1_max_entries: Option<usize>,
    l2_max_entries: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default)]
struct RawViewConfig {
    initial_page: Option<usize>,
    initial_zoom: Option<f32>,
    initial_layout: Option<String>,
    spread_direction: Option<String>,
    spread_cover: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
struct RawInputConfig {
    sequence_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
struct RawKeymapConfig {
    preset: Option<String>,
    unbind: Vec<String>,
    bindings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
struct RawWatchConfig {
    enabled: Option<bool>,
    poll_interval_ms: Option<u64>,
    settle_delay_ms: Option<u64>,
}

impl RawConfig {
    fn into_options(self) -> AppResult<AppOptions> {
        Ok(AppOptions {
            render: self.render.map(RenderOptions::from).unwrap_or_default(),
            cache: self.cache.map(CacheOptions::from).unwrap_or_default(),
            view: self
                .view
                .map(ViewOptions::try_from)
                .transpose()?
                .unwrap_or_default(),
            input: self.input.map(InputOptions::from).unwrap_or_default(),
            keymap: self
                .keymap
                .map(KeymapOptions::try_from)
                .transpose()?
                .unwrap_or_default(),
            watch: self.watch.map(WatchOptions::from).unwrap_or_default(),
        })
    }
}

impl From<RawRenderConfig> for RenderOptions {
    fn from(raw: RawRenderConfig) -> Self {
        Self {
            worker_threads: raw.worker_threads,
            input_poll_timeout_idle_ms: raw.input_poll_timeout_idle_ms,
            input_poll_timeout_busy_ms: raw.input_poll_timeout_busy_ms,
            prefetch_pause_ms: raw.prefetch_pause_ms,
            prefetch_tick_ms: raw.prefetch_tick_ms,
            pending_redraw_interval_ms: raw.pending_redraw_interval_ms,
            prefetch_dispatch_budget_per_tick: raw.prefetch_dispatch_budget_per_tick,
            max_render_scale: raw.max_render_scale,
        }
    }
}

impl From<RawCacheConfig> for CacheOptions {
    fn from(raw: RawCacheConfig) -> Self {
        Self {
            l1_memory_budget_mb: raw.l1_memory_budget_mb,
            l2_memory_budget_mb: raw.l2_memory_budget_mb,
            l1_max_entries: raw.l1_max_entries,
            l2_max_entries: raw.l2_max_entries,
        }
    }
}

impl TryFrom<RawViewConfig> for ViewOptions {
    type Error = AppError;

    fn try_from(raw: RawViewConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            initial_page: raw.initial_page,
            initial_zoom: raw.initial_zoom,
            initial_layout: raw
                .initial_layout
                .as_deref()
                .map(parse_page_layout_mode)
                .transpose()?,
            spread_direction: raw
                .spread_direction
                .as_deref()
                .map(parse_spread_direction)
                .transpose()?,
            spread_cover: raw
                .spread_cover
                .as_deref()
                .map(parse_spread_cover)
                .transpose()?,
        })
    }
}

impl From<RawInputConfig> for InputOptions {
    fn from(raw: RawInputConfig) -> Self {
        Self {
            sequence_timeout_ms: raw.sequence_timeout_ms,
        }
    }
}

impl TryFrom<RawKeymapConfig> for KeymapOptions {
    type Error = AppError;

    fn try_from(raw: RawKeymapConfig) -> Result<Self, Self::Error> {
        let preset = raw
            .preset
            .as_deref()
            .map(super::keymap::parse_keymap_preset)
            .transpose()?;
        let unbind = raw
            .unbind
            .iter()
            .map(|key| super::keymap::parse_keymap_target(key))
            .collect::<AppResult<Vec<_>>>()?;
        let bindings = raw
            .bindings
            .iter()
            .map(|(key, command)| super::keymap::parse_keymap_binding(key, command))
            .collect::<AppResult<Vec<_>>>()?;

        Ok(Self {
            preset,
            unbind,
            bindings,
        })
    }
}

impl From<RawWatchConfig> for WatchOptions {
    fn from(raw: RawWatchConfig) -> Self {
        Self {
            enabled: raw.enabled,
            poll_interval_ms: raw.poll_interval_ms,
            settle_delay_ms: raw.settle_delay_ms,
        }
    }
}

fn parse_page_layout_mode(value: &str) -> AppResult<PageLayoutMode> {
    match value {
        "single" => Ok(PageLayoutMode::Single),
        "spread" => Ok(PageLayoutMode::Spread),
        _ => Err(AppError::invalid_argument(format!(
            "unknown view.initial_layout: {value}"
        ))),
    }
}

fn parse_spread_direction(value: &str) -> AppResult<SpreadDirection> {
    match value {
        "ltr" => Ok(SpreadDirection::Ltr),
        "rtl" => Ok(SpreadDirection::Rtl),
        _ => Err(AppError::invalid_argument(format!(
            "unknown view.spread_direction: {value}"
        ))),
    }
}

fn parse_spread_cover(value: &str) -> AppResult<SpreadCoverPolicy> {
    match value {
        "paired" => Ok(SpreadCoverPolicy::Paired),
        "cover" => Ok(SpreadCoverPolicy::Cover),
        _ => Err(AppError::invalid_argument(format!(
            "unknown view.spread_cover: {value}"
        ))),
    }
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
    use crate::config::{AppOptionsResolver, KeymapBinding, KeymapPreset, KeymapTarget};
    use crate::input::sequence::{DEFAULT_SEQUENCE_TIMEOUT, SequenceResolution, SequenceResolver};
    use crate::input::shortcut::ShortcutKey;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{
        Config, ConfigFileSelection, default_config_path_from_env, load_options_from_explicit_path,
    };

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
    fn optional_config_file_missing_uses_defaults() {
        let missing = unique_temp_path("missing.toml");
        let config = Config::load_from_path(&missing).expect("missing config should fallback");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn config_file_partial_sections_merge_with_sanitized_defaults() {
        let path = unique_temp_path("custom.toml");
        fs::write(
            &path,
            r#"
            [render]
            worker_threads = 0
            input_poll_timeout_idle_ms = 0
            input_poll_timeout_busy_ms = 0
            prefetch_pause_ms = 0
            prefetch_tick_ms = 0
            pending_redraw_interval_ms = 0
            prefetch_dispatch_budget_per_tick = 0
            max_render_scale = 0.5

            [cache]
            l1_memory_budget_mb = 256

            [view]
            initial_page = 4
            initial_zoom = 1.25
            initial_layout = "spread"
            spread_direction = "rtl"
            spread_cover = "cover"

            [input]
            sequence_timeout_ms = 333

            [watch]
            enabled = true
            poll_interval_ms = 125
            settle_delay_ms = 250
            "#,
        )
        .expect("config file should be written");

        let config = Config::load_from_path(&path).expect("config should parse");
        assert_eq!(config.render.worker_threads, 1);
        assert_eq!(config.render.input_poll_timeout_idle_ms, 1);
        assert_eq!(config.render.input_poll_timeout_busy_ms, 1);
        assert_eq!(config.render.prefetch_pause_ms, 1);
        assert_eq!(config.render.prefetch_tick_ms, 1);
        assert_eq!(config.render.pending_redraw_interval_ms, 1);
        assert_eq!(config.render.prefetch_dispatch_budget_per_tick, 1);
        assert_eq!(config.render.max_render_scale, 2.5);
        assert_eq!(config.cache.l1_memory_budget_mb, 256);
        assert_eq!(config.cache.l2_memory_budget_mb, 64);
        assert_eq!(config.cache.l1_max_entries, 128);
        assert_eq!(config.cache.l2_max_entries, 96);
        assert_eq!(config.view.initial_page, 4);
        assert_eq!(config.view.initial_zoom, 1.25);
        assert_eq!(config.view.initial_layout, PageLayoutMode::Spread);
        assert_eq!(config.view.spread_direction, SpreadDirection::Rtl);
        assert_eq!(config.view.spread_cover, SpreadCoverPolicy::Cover);
        assert_eq!(config.input.sequence_timeout_ms, 333);
        assert!(config.watch.enabled);
        assert_eq!(config.watch.poll_interval_ms, 125);
        assert_eq!(config.watch.settle_delay_ms, 250);

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn explicit_config_reads_keymap_section() {
        let path = unique_temp_path("keymap.toml");
        fs::write(
            &path,
            r#"
            [keymap]
            preset = "none"
            unbind = ["j", "[count]G"]

            [keymap.bindings]
            "<down>" = "next-page"
            "[count]G" = "goto-page"
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("config should parse");
        assert_eq!(options.keymap.preset, Some(KeymapPreset::None));
        assert_eq!(
            options.keymap.unbind,
            vec![
                KeymapTarget::Exact(vec![ShortcutKey::char('j')]),
                KeymapTarget::NumericPrefix(ShortcutKey::char('G')),
            ]
        );
        assert_eq!(
            options.keymap.bindings,
            vec![
                KeymapBinding::Exact {
                    keys: vec![ShortcutKey::key(KeyCode::Down)],
                    command: Command::NextPage,
                },
                KeymapBinding::NumericPrefix {
                    suffix: ShortcutKey::char('G'),
                    command_id: "goto-page",
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
            [keymap]
            preset = "none"

            [keymap.bindings]
            "<down>" = "next-page"
            "[count]G" = "goto-page"
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("config should parse");
        let resolved = AppOptionsResolver::new().apply_options(options).resolve();
        let mut resolver =
            SequenceResolver::new(resolved.input.sequence_registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            SequenceResolution::Noop
        );
        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            SequenceResolution::Dispatch(Command::NextPage)
        );
        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );
        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );
        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE)),
            SequenceResolution::Dispatch(Command::GotoPage { page: 42 })
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_allows_literal_less_than_binding() {
        let path = unique_temp_path("keymap-less-than.toml");
        fs::write(
            &path,
            r#"
            [keymap]
            preset = "none"

            [keymap.bindings]
            "<" = "prev-page"
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("config should parse");
        let resolved = AppOptionsResolver::new().apply_options(options).resolve();
        let mut resolver =
            SequenceResolver::new(resolved.input.sequence_registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('<'), KeyModifiers::NONE)),
            SequenceResolution::Dispatch(Command::PrevPage)
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_rejects_unknown_preset() {
        let path = unique_temp_path("bad-keymap-preset.toml");
        fs::write(
            &path,
            r#"
            [keymap]
            preset = "emacs"
            "#,
        )
        .expect("config file should be written");

        let err = load_options_from_explicit_path(&path).expect_err("config should be rejected");
        assert!(
            err.to_string().contains("unknown keymap preset"),
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
            [keymap.bindings]
            "x" = "submit-search needle"
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
    fn keymap_config_rejects_scoped_interaction_commands() {
        for (command, expected) in [
            ("palette.submit", "requires an active palette"),
            ("close-help", "requires active help"),
        ] {
            let path = unique_temp_path(&format!("bad-keymap-interaction-{command}.toml"));
            fs::write(
                &path,
                format!(
                    r#"
                    [keymap.bindings]
                    "x" = "{command}"
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
    fn keymap_config_rejects_escape_bindings() {
        let path = unique_temp_path("bad-keymap-key.toml");
        fs::write(
            &path,
            r#"
            [keymap.bindings]
            "<esc>" = "quit"
            "#,
        )
        .expect("config file should be written");

        let err = load_options_from_explicit_path(&path).expect_err("config should be rejected");
        assert!(err.to_string().contains("<esc>"), "unexpected error: {err}");

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn keymap_config_accepts_enter_bindings() {
        let path = unique_temp_path("enter-keymap-key.toml");
        fs::write(
            &path,
            r#"
            [keymap.bindings]
            "<enter>" = "next-page"
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("config should load");
        assert_eq!(
            options.keymap.bindings,
            vec![KeymapBinding::Exact {
                keys: vec![ShortcutKey::key(KeyCode::Enter)],
                command: Command::NextPage,
            }]
        );

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn explicit_config_preserves_unspecified_fields_as_absent_options() {
        let path = unique_temp_path("partial-options.toml");
        fs::write(
            &path,
            r#"
            [cache]
            l1_max_entries = 42
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("options should parse");
        assert_eq!(options.cache.l1_max_entries, Some(42));
        assert_eq!(options.cache.l1_memory_budget_mb, None);
        assert_eq!(options.render.worker_threads, None);
        assert_eq!(options.render.max_render_scale, None);
        assert_eq!(options.view.initial_page, None);
        assert_eq!(options.input.sequence_timeout_ms, None);
        assert_eq!(options.keymap.preset, None);
        assert!(options.keymap.unbind.is_empty());
        assert!(options.keymap.bindings.is_empty());
        assert_eq!(options.watch.enabled, None);

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

            [input]
            sequence_timeout_ms = 750

            [watch]
            enabled = true
            poll_interval_ms = 100
            settle_delay_ms = 200
            "#,
        )
        .expect("config file should be written");

        let options = load_options_from_explicit_path(&path).expect("options should parse");
        assert_eq!(options.view.initial_page, Some(8));
        assert_eq!(options.view.initial_zoom, Some(1.5));
        assert_eq!(options.view.initial_layout, Some(PageLayoutMode::Spread));
        assert_eq!(options.view.spread_direction, Some(SpreadDirection::Rtl));
        assert_eq!(options.view.spread_cover, Some(SpreadCoverPolicy::Cover));
        assert_eq!(options.input.sequence_timeout_ms, Some(750));
        assert_eq!(options.watch.enabled, Some(true));
        assert_eq!(options.watch.poll_interval_ms, Some(100));
        assert_eq!(options.watch.settle_delay_ms, Some(200));

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn explicit_config_rejects_unknown_view_enum_values() {
        let path = unique_temp_path("bad-view-options.toml");
        fs::write(
            &path,
            r#"
            [view]
            initial_layout = "grid"
            "#,
        )
        .expect("config file should be written");

        let err = load_options_from_explicit_path(&path).expect_err("config should be rejected");
        assert!(
            err.to_string().contains("unknown view.initial_layout"),
            "unexpected error: {err}"
        );

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
