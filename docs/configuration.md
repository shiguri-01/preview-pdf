# Configuration

This document owns the current app option sources, precedence rules, and
supported user configuration fields.

Runtime state is not loaded directly from TOML. Each source contributes a
partial `AppOptions` patch, the resolver applies built-in defaults and
validation, and `App` hands the resolved policies to the subsystems that use
them.

## Precedence

Later sources override earlier sources:

1. Built-in defaults
2. `config.toml` if enabled
3. CLI options for the current process
4. Explicit runtime command arguments

Runtime command arguments only override the values they specify. For example,
`page-layout-spread ltr` uses `ltr` for the command direction and the resolved
`view.spread_cover` value for the omitted cover policy.

## Config Lookup

Default config lookup checks these paths in order:

1. `PVF_CONFIG_PATH`
2. `XDG_CONFIG_HOME/pvf/config.toml`
3. `HOME/.config/pvf/config.toml`
4. `APPDATA/pvf/config.toml`

If no default config path resolves, built-in defaults are used. An explicit
`--config <path>` must name an existing regular file.

## Supported TOML

```toml
[view]
initial_page = 1
initial_zoom = 1.0
initial_layout = "single"
spread_direction = "ltr"
spread_cover = "paired"

[input]
sequence_timeout_ms = 1000

[watch]
enabled = false
poll_interval_ms = 250
settle_delay_ms = 500

[render]
worker_threads = 3
input_poll_timeout_idle_ms = 16
input_poll_timeout_busy_ms = 8
prefetch_pause_ms = 120
prefetch_tick_ms = 8
pending_redraw_interval_ms = 33
prefetch_dispatch_budget_per_tick = 6
max_render_scale = 2.5

[cache]
l1_memory_budget_mb = 512
l2_memory_budget_mb = 64
l1_max_entries = 128
l2_max_entries = 96
```

`view.initial_page` is one-based. `view.initial_zoom` is a ratio relative to
the fitted view, not a PDF physical-size percentage.

Allowed enum values:

- `view.initial_layout`: `single`, `spread`
- `view.spread_direction`: `ltr`, `rtl`
- `view.spread_cover`: `paired`, `cover`

Numeric duration fields are milliseconds. Zero duration values are sanitized to
the minimum safe value. Invalid `view.initial_zoom` falls back to the default;
out-of-range finite zoom values are clamped to the runtime zoom bounds.
