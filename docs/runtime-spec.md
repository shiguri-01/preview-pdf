# Runtime Specification

This document is the source of truth for the current CLI contract and
user-visible runtime behavior.

## Scope

This document owns CLI entry points, user-visible viewer behavior, default key
bindings, and runtime configuration lookup and supported config fields.

Implementation structure is described in `architecture.md`. Rendering,
palette, extension, and command internals are described in their owning
documents.

## CLI contract

Supported invocations:

```bash
pvf <file.pdf>
pvf perf <file.pdf> --scenario <scenario-id> [--out <path|->]
```

Rules:

- Exactly one PDF path argument is required.
- The document is opened through the default backend factory.
- `perf` runs a built-in scenario, emits JSON, and exits without opening the
  interactive viewer.
- Supported perf scenarios are:
  - `page-flip-forward`
  - `page-flip-backward`
  - `idle-pending-redraw`

## Viewer behavior

- Navigation
  - next/previous page
  - first/last page
  - direct page jump
  - in spread layout, incremental navigation advances by two pages

- Page layout
  - `single` is the default layout
  - `spread` shows two logical pages side by side
  - spread direction supports `ltr` and `rtl`
  - runtime commands are `page-layout-single` and `page-layout-spread [ltr|rtl]`

- Zoom and viewport
  - zoom uses a bounded discrete ladder
  - `zoom <value>` clamps out-of-range input to the configured bounds and shows
    a warning notice
  - `zoom-reset` returns to the fit baseline
  - panning is available when content exceeds the viewport bounds
  - `pan <direction>` uses a default step of one terminal cell
  - `pan <direction> <amount>` uses an exact cell count

- Search
  - full-text substring search supports case-insensitive and case-sensitive
    matchers
  - search runs asynchronously and reports progress while active
  - visible search hits are highlighted through the generic highlight overlay
    layer
  - `search` opens the search palette
  - `next-search-hit` and `prev-search-hit` are available only while search is
    active
  - `cancel-search` cancels the active search state when search is active
  - pressing `<esc>` in normal mode cancels active search state after any
    pending multi-key sequence resolves or clears

- Command palette
  - `:` opens the command palette
  - recent command inputs are stored and can be recalled while the palette is
    open

- Help
  - `help` and `?` open the help overlay
  - the overlay renders the current runtime keymap
  - `<esc>` closes the overlay

- History
  - back/forward history is available through commands and palette flow
  - `history` opens the history palette
  - history records navigation reasons rather than every page transition

- Outline
  - `outline` opens the outline palette
  - outline entries are extracted from the PDF outline/bookmark tree
  - selecting an outline entry jumps to the linked page
  - unsupported or non-page destinations are ignored during extraction
  - an empty outline state is valid and remains interactive

- Rendering and terminal output
  - the viewer renders PDF pages as terminal images
  - supported output protocols depend on terminal capability negotiation
  - cold start may temporarily show a lower-resolution preview before the
    current full-resolution view is ready

## Default key bindings

Printable bindings are defined by the resulting character, not by a physical
key plus modifiers. For example, `?` and `:` are literal bindings.

| Key | Action |
|---|---|
| `j` | Next page |
| `k` | Previous page |
| `gg` | First page |
| `G` | Last page |
| `[count]G` | Go to page `count` |
| `+` | Zoom in |
| `-` | Zoom out |
| `=` | Reset zoom |
| `H` / `J` / `K` / `L` | Pan |
| `:` | Open command palette |
| `/` | Open search palette |
| `n` | Next search hit |
| `N` | Previous search hit |
| `<c-o>` | History back |
| `<c-i>` | History forward |
| `?` | Open shortcut help |
| `<esc>` | Cancel or close the current interactive surface |
| `q` | Quit |

## Runtime configuration

`config.toml` controls render and cache tuning.

Lookup order:

1. `PVF_CONFIG_PATH`
2. `XDG_CONFIG_HOME/pvf/config.toml`
3. `HOME/.config/pvf/config.toml`
4. `APPDATA/pvf/config.toml`

If no config path resolves, built-in defaults are used.

Supported configuration sections:

- `[render]`
  - `worker_threads`
  - `input_poll_timeout_idle_ms`
  - `input_poll_timeout_busy_ms`
  - `prefetch_pause_ms`
  - `prefetch_tick_ms`
  - `pending_redraw_interval_ms`
  - `prefetch_dispatch_budget_per_tick`
  - `max_render_scale`

- `[cache]`
  - `l1_memory_budget_mb`
  - `l2_memory_budget_mb`
  - `l1_max_entries`
  - `l2_max_entries`

Missing config files fall back to defaults. Invalid numeric render values are
sanitized to a minimum safe value, and invalid `max_render_scale` falls back to
the default.

## Code references

- `src/config.rs`
- `src/command/spec.rs`
- `src/input/keymap.rs`
- `src/ui/help.rs`
