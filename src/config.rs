use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::input::keymap::build_builtin_sequence_registry;
use crate::input::sequence::{DEFAULT_SEQUENCE_TIMEOUT, SequenceRegistry};

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Config {
    pub render: RenderConfig,
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct RenderConfig {
    pub worker_threads: usize,
    pub input_poll_timeout_idle_ms: u64,
    pub input_poll_timeout_busy_ms: u64,
    pub prefetch_pause_ms: u64,
    pub prefetch_tick_ms: u64,
    pub pending_redraw_interval_ms: u64,
    pub prefetch_dispatch_budget_per_tick: usize,
    pub max_render_scale: f32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            worker_threads: 3,
            input_poll_timeout_idle_ms: 16,
            input_poll_timeout_busy_ms: 8,
            prefetch_pause_ms: 120,
            prefetch_tick_ms: 8,
            pending_redraw_interval_ms: 33,
            prefetch_dispatch_budget_per_tick: 6,
            max_render_scale: 2.5,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CacheConfig {
    pub l1_memory_budget_mb: usize,
    pub l2_memory_budget_mb: usize,
    pub l1_max_entries: usize,
    pub l2_max_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            l1_memory_budget_mb: 512,
            l2_memory_budget_mb: 64,
            l1_max_entries: 128,
            l2_max_entries: 96,
        }
    }
}

impl CacheConfig {
    const MEBIBYTE: usize = 1024 * 1024;

    pub fn l1_memory_budget_bytes(&self) -> usize {
        self.l1_memory_budget_mb
            .saturating_mul(Self::MEBIBYTE)
            .max(1)
    }

    pub fn l2_memory_budget_bytes(&self) -> usize {
        self.l2_memory_budget_mb
            .saturating_mul(Self::MEBIBYTE)
            .max(1)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppOptions {
    pub render: RenderOptions,
    pub cache: CacheOptions,
    pub input: InputOptions,
}

impl AppOptions {
    pub fn merge(mut self, next: Self) -> Self {
        self.render = self.render.merge(next.render);
        self.cache = self.cache.merge(next.cache);
        self.input = self.input.merge(next.input);
        self
    }
}

impl From<Config> for AppOptions {
    fn from(config: Config) -> Self {
        Self {
            render: RenderOptions {
                worker_threads: Some(config.render.worker_threads),
                input_poll_timeout_idle_ms: Some(config.render.input_poll_timeout_idle_ms),
                input_poll_timeout_busy_ms: Some(config.render.input_poll_timeout_busy_ms),
                prefetch_pause_ms: Some(config.render.prefetch_pause_ms),
                prefetch_tick_ms: Some(config.render.prefetch_tick_ms),
                pending_redraw_interval_ms: Some(config.render.pending_redraw_interval_ms),
                prefetch_dispatch_budget_per_tick: Some(
                    config.render.prefetch_dispatch_budget_per_tick,
                ),
                max_render_scale: Some(config.render.max_render_scale),
            },
            cache: CacheOptions {
                l1_memory_budget_mb: Some(config.cache.l1_memory_budget_mb),
                l2_memory_budget_mb: Some(config.cache.l2_memory_budget_mb),
                l1_max_entries: Some(config.cache.l1_max_entries),
                l2_max_entries: Some(config.cache.l2_max_entries),
            },
            input: InputOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderOptions {
    pub worker_threads: Option<usize>,
    pub input_poll_timeout_idle_ms: Option<u64>,
    pub input_poll_timeout_busy_ms: Option<u64>,
    pub prefetch_pause_ms: Option<u64>,
    pub prefetch_tick_ms: Option<u64>,
    pub pending_redraw_interval_ms: Option<u64>,
    pub prefetch_dispatch_budget_per_tick: Option<usize>,
    pub max_render_scale: Option<f32>,
}

impl RenderOptions {
    fn merge(self, next: Self) -> Self {
        Self {
            worker_threads: next.worker_threads.or(self.worker_threads),
            input_poll_timeout_idle_ms: next
                .input_poll_timeout_idle_ms
                .or(self.input_poll_timeout_idle_ms),
            input_poll_timeout_busy_ms: next
                .input_poll_timeout_busy_ms
                .or(self.input_poll_timeout_busy_ms),
            prefetch_pause_ms: next.prefetch_pause_ms.or(self.prefetch_pause_ms),
            prefetch_tick_ms: next.prefetch_tick_ms.or(self.prefetch_tick_ms),
            pending_redraw_interval_ms: next
                .pending_redraw_interval_ms
                .or(self.pending_redraw_interval_ms),
            prefetch_dispatch_budget_per_tick: next
                .prefetch_dispatch_budget_per_tick
                .or(self.prefetch_dispatch_budget_per_tick),
            max_render_scale: next.max_render_scale.or(self.max_render_scale),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheOptions {
    pub l1_memory_budget_mb: Option<usize>,
    pub l2_memory_budget_mb: Option<usize>,
    pub l1_max_entries: Option<usize>,
    pub l2_max_entries: Option<usize>,
}

impl CacheOptions {
    fn merge(self, next: Self) -> Self {
        Self {
            l1_memory_budget_mb: next.l1_memory_budget_mb.or(self.l1_memory_budget_mb),
            l2_memory_budget_mb: next.l2_memory_budget_mb.or(self.l2_memory_budget_mb),
            l1_max_entries: next.l1_max_entries.or(self.l1_max_entries),
            l2_max_entries: next.l2_max_entries.or(self.l2_max_entries),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputOptions {
    pub sequence_timeout_ms: Option<u64>,
}

impl InputOptions {
    fn merge(self, next: Self) -> Self {
        Self {
            sequence_timeout_ms: next.sequence_timeout_ms.or(self.sequence_timeout_ms),
        }
    }
}

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
            Self::Path(path) => load_options_from_path(path),
            Self::Disabled => Ok(AppOptions::default()),
        }
    }
}

pub fn load_default_app_options() -> AppResult<AppOptions> {
    let Some(path) = default_config_path() else {
        return Ok(AppOptions::default());
    };
    load_options_from_path(path)
}

pub fn load_options_from_path(path: impl AsRef<Path>) -> AppResult<AppOptions> {
    Config::load_from_path(path).map(AppOptions::from)
}

#[derive(Debug, Clone)]
pub struct ResolvedAppOptions {
    pub render: RenderPolicy,
    pub event_loop: EventLoopPolicy,
    pub cache: CachePolicy,
    pub input: InputPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderPolicy {
    pub worker_threads: usize,
    pub max_render_scale: f32,
}

impl Default for RenderPolicy {
    fn default() -> Self {
        let render = RenderConfig::default();
        Self {
            worker_threads: render.worker_threads,
            max_render_scale: render.max_render_scale,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventLoopPolicy {
    pub input_poll_timeout_idle: Duration,
    pub input_poll_timeout_busy: Duration,
    pub prefetch_pause_after_input: Duration,
    pub prefetch_tick_interval: Duration,
    pub pending_redraw_interval: Duration,
    pub prefetch_dispatch_budget_per_tick: usize,
}

impl Default for EventLoopPolicy {
    fn default() -> Self {
        let render = RenderConfig::default();
        Self {
            input_poll_timeout_idle: Duration::from_millis(render.input_poll_timeout_idle_ms),
            input_poll_timeout_busy: Duration::from_millis(render.input_poll_timeout_busy_ms),
            prefetch_pause_after_input: Duration::from_millis(render.prefetch_pause_ms),
            prefetch_tick_interval: Duration::from_millis(render.prefetch_tick_ms),
            pending_redraw_interval: Duration::from_millis(render.pending_redraw_interval_ms),
            prefetch_dispatch_budget_per_tick: render.prefetch_dispatch_budget_per_tick,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePolicy {
    pub l1_memory_budget_mb: usize,
    pub l2_memory_budget_mb: usize,
    pub l1_max_entries: usize,
    pub l2_max_entries: usize,
}

impl Default for CachePolicy {
    fn default() -> Self {
        let cache = CacheConfig::default();
        Self {
            l1_memory_budget_mb: cache.l1_memory_budget_mb,
            l2_memory_budget_mb: cache.l2_memory_budget_mb,
            l1_max_entries: cache.l1_max_entries,
            l2_max_entries: cache.l2_max_entries,
        }
    }
}

impl CachePolicy {
    const MEBIBYTE: usize = 1024 * 1024;

    pub fn l1_memory_budget_bytes(&self) -> usize {
        self.l1_memory_budget_mb
            .saturating_mul(Self::MEBIBYTE)
            .max(1)
    }

    pub fn l2_memory_budget_bytes(&self) -> usize {
        self.l2_memory_budget_mb
            .saturating_mul(Self::MEBIBYTE)
            .max(1)
    }
}

#[derive(Debug, Clone)]
pub struct InputPolicy {
    pub sequence_timeout: Duration,
    pub sequence_registry: SequenceRegistry,
}

impl Default for InputPolicy {
    fn default() -> Self {
        Self {
            sequence_timeout: DEFAULT_SEQUENCE_TIMEOUT,
            sequence_registry: build_builtin_sequence_registry(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppOptionsResolver {
    options: AppOptions,
}

impl AppOptionsResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_options(mut self, options: AppOptions) -> Self {
        self.options = self.options.merge(options);
        self
    }

    pub fn resolve(self) -> ResolvedAppOptions {
        resolve_options(self.options)
    }
}

impl Default for ResolvedAppOptions {
    fn default() -> Self {
        AppOptionsResolver::new().resolve()
    }
}

fn resolve_options(options: AppOptions) -> ResolvedAppOptions {
    let render_defaults = RenderConfig::default();
    let cache_defaults = CacheConfig::default();

    let worker_threads = options
        .render
        .worker_threads
        .unwrap_or(render_defaults.worker_threads)
        .max(1);
    let input_poll_timeout_idle_ms = options
        .render
        .input_poll_timeout_idle_ms
        .unwrap_or(render_defaults.input_poll_timeout_idle_ms)
        .max(1);
    let input_poll_timeout_busy_ms = options
        .render
        .input_poll_timeout_busy_ms
        .unwrap_or(render_defaults.input_poll_timeout_busy_ms)
        .max(1);
    let prefetch_pause_ms = options
        .render
        .prefetch_pause_ms
        .unwrap_or(render_defaults.prefetch_pause_ms)
        .max(1);
    let prefetch_tick_ms = options
        .render
        .prefetch_tick_ms
        .unwrap_or(render_defaults.prefetch_tick_ms)
        .max(1);
    let pending_redraw_interval_ms = options
        .render
        .pending_redraw_interval_ms
        .unwrap_or(render_defaults.pending_redraw_interval_ms)
        .max(1);
    let prefetch_dispatch_budget_per_tick = options
        .render
        .prefetch_dispatch_budget_per_tick
        .unwrap_or(render_defaults.prefetch_dispatch_budget_per_tick)
        .max(1);
    let mut max_render_scale = options
        .render
        .max_render_scale
        .unwrap_or(render_defaults.max_render_scale);
    if !max_render_scale.is_finite() || max_render_scale < 1.0 {
        max_render_scale = render_defaults.max_render_scale;
    }
    let sequence_timeout_ms = options
        .input
        .sequence_timeout_ms
        .unwrap_or(DEFAULT_SEQUENCE_TIMEOUT.as_millis() as u64)
        .max(1);

    ResolvedAppOptions {
        render: RenderPolicy {
            worker_threads,
            max_render_scale,
        },
        event_loop: EventLoopPolicy {
            input_poll_timeout_idle: Duration::from_millis(input_poll_timeout_idle_ms),
            input_poll_timeout_busy: Duration::from_millis(input_poll_timeout_busy_ms),
            prefetch_pause_after_input: Duration::from_millis(prefetch_pause_ms),
            prefetch_tick_interval: Duration::from_millis(prefetch_tick_ms),
            pending_redraw_interval: Duration::from_millis(pending_redraw_interval_ms),
            prefetch_dispatch_budget_per_tick,
        },
        cache: CachePolicy {
            l1_memory_budget_mb: options
                .cache
                .l1_memory_budget_mb
                .unwrap_or(cache_defaults.l1_memory_budget_mb),
            l2_memory_budget_mb: options
                .cache
                .l2_memory_budget_mb
                .unwrap_or(cache_defaults.l2_memory_budget_mb),
            l1_max_entries: options
                .cache
                .l1_max_entries
                .unwrap_or(cache_defaults.l1_max_entries),
            l2_max_entries: options
                .cache
                .l2_max_entries
                .unwrap_or(cache_defaults.l2_max_entries),
        },
        input: InputPolicy {
            sequence_timeout: Duration::from_millis(sequence_timeout_ms),
            sequence_registry: build_builtin_sequence_registry(),
        },
    }
}

impl Config {
    pub fn load() -> AppResult<Self> {
        let Some(path) = default_config_path() else {
            return Ok(Self::default());
        };
        Self::load_from_path(path)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
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
        let parsed = toml::from_str::<Self>(&raw).map_err(|source| {
            AppError::invalid_argument(format!(
                "failed to parse config {}: {source}",
                path.display()
            ))
        })?;
        Ok(parsed.sanitized())
    }

    fn sanitized(mut self) -> Self {
        let resolved = AppOptionsResolver::new()
            .apply_options(AppOptions::from(self.clone()))
            .resolve();
        self.render.worker_threads = resolved.render.worker_threads;
        self.render.input_poll_timeout_idle_ms =
            resolved.event_loop.input_poll_timeout_idle.as_millis() as u64;
        self.render.input_poll_timeout_busy_ms =
            resolved.event_loop.input_poll_timeout_busy.as_millis() as u64;
        self.render.prefetch_pause_ms =
            resolved.event_loop.prefetch_pause_after_input.as_millis() as u64;
        self.render.prefetch_tick_ms =
            resolved.event_loop.prefetch_tick_interval.as_millis() as u64;
        self.render.pending_redraw_interval_ms =
            resolved.event_loop.pending_redraw_interval.as_millis() as u64;
        self.render.prefetch_dispatch_budget_per_tick =
            resolved.event_loop.prefetch_dispatch_budget_per_tick;
        self.render.max_render_scale = resolved.render.max_render_scale;
        self
    }
}

pub fn default_config_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("PVF_CONFIG_PATH")
        && !explicit.is_empty()
    {
        return Some(PathBuf::from(explicit));
    }

    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Some(PathBuf::from(xdg).join("pvf").join("config.toml"));
    }
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return Some(
            PathBuf::from(home)
                .join(".config")
                .join("pvf")
                .join("config.toml"),
        );
    }
    if let Some(appdata) = std::env::var_os("APPDATA")
        && !appdata.is_empty()
    {
        return Some(PathBuf::from(appdata).join("pvf").join("config.toml"));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use std::time::Duration;

    use super::{AppOptions, AppOptionsResolver, Config, RenderOptions};

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
    fn load_from_path_returns_defaults_for_missing_file() {
        let missing = unique_temp_path("missing.toml");
        let config = Config::load_from_path(&missing).expect("missing config should fallback");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn load_from_path_applies_partial_overrides_and_sanitizes() {
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

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn load_from_path_ignores_legacy_keymap_section() {
        let path = unique_temp_path("legacy-keymap.toml");
        fs::write(
            &path,
            r#"
            [keymap]
            preset = "emacs"

            [cache]
            l1_max_entries = 42
            "#,
        )
        .expect("config file should be written");

        let config = Config::load_from_path(&path).expect("config should parse");
        assert_eq!(config.cache.l1_max_entries, 42);

        fs::remove_file(&path).expect("config file should be removed");
    }

    #[test]
    fn resolver_applies_defaults_and_sanitizes_without_file_source() {
        let options = AppOptions {
            render: RenderOptions {
                worker_threads: Some(0),
                input_poll_timeout_idle_ms: Some(0),
                input_poll_timeout_busy_ms: Some(0),
                prefetch_pause_ms: Some(0),
                prefetch_tick_ms: Some(0),
                pending_redraw_interval_ms: Some(0),
                prefetch_dispatch_budget_per_tick: Some(0),
                max_render_scale: Some(0.5),
            },
            ..AppOptions::default()
        };

        let resolved = AppOptionsResolver::new().apply_options(options).resolve();

        assert_eq!(resolved.render.worker_threads, 1);
        assert_eq!(
            resolved.event_loop.input_poll_timeout_idle,
            Duration::from_millis(1)
        );
        assert_eq!(
            resolved.event_loop.input_poll_timeout_busy,
            Duration::from_millis(1)
        );
        assert_eq!(
            resolved.event_loop.prefetch_pause_after_input,
            Duration::from_millis(1)
        );
        assert_eq!(
            resolved.event_loop.prefetch_tick_interval,
            Duration::from_millis(1)
        );
        assert_eq!(
            resolved.event_loop.pending_redraw_interval,
            Duration::from_millis(1)
        );
        assert_eq!(resolved.event_loop.prefetch_dispatch_budget_per_tick, 1);
        assert_eq!(resolved.render.max_render_scale, 2.5);
    }

    #[test]
    fn resolver_merges_later_options_over_earlier_options() {
        let base = AppOptions {
            render: RenderOptions {
                worker_threads: Some(2),
                ..RenderOptions::default()
            },
            ..AppOptions::default()
        };
        let override_options = AppOptions {
            render: RenderOptions {
                worker_threads: Some(4),
                ..RenderOptions::default()
            },
            ..AppOptions::default()
        };

        let resolved = AppOptionsResolver::new()
            .apply_options(base)
            .apply_options(override_options)
            .resolve();

        assert_eq!(resolved.render.worker_threads, 4);
    }
}
