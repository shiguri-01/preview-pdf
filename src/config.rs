use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Config {
    pub render: RenderConfig,
    pub cache: CacheConfig,
    pub keymap: KeymapConfig,
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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct KeymapConfig {
    pub preset: String,
}

impl Default for KeymapConfig {
    fn default() -> Self {
        Self {
            preset: "default".to_string(),
        }
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
        self.render.worker_threads = self.render.worker_threads.max(1);
        self.render.input_poll_timeout_idle_ms = self.render.input_poll_timeout_idle_ms.max(1);
        self.render.input_poll_timeout_busy_ms = self.render.input_poll_timeout_busy_ms.max(1);
        self.render.prefetch_pause_ms = self.render.prefetch_pause_ms.max(1);
        self.render.prefetch_tick_ms = self.render.prefetch_tick_ms.max(1);
        self.render.pending_redraw_interval_ms = self.render.pending_redraw_interval_ms.max(1);
        self.render.prefetch_dispatch_budget_per_tick =
            self.render.prefetch_dispatch_budget_per_tick.max(1);
        if !self.render.max_render_scale.is_finite() || self.render.max_render_scale < 1.0 {
            self.render.max_render_scale = RenderConfig::default().max_render_scale;
        }
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

    use super::Config;

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
}
