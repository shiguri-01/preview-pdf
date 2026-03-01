# pvf Specification Overview

`pvf` is a Rust CLI/TUI PDF viewer that renders pages as terminal images.

## CLI contract

Command:

```bash
pvf <file.pdf>
```

Rules:
- Exactly one PDF path argument is required.
- The path is opened through the default backend factory (`open_default_backend`).

## Functional specification

- Navigation:
  - next/previous page
  - first/last page
  - direct page jump

- Zoom and viewport:
  - zoom in/out with bounded scale
  - fit-to-viewport behavior
  - horizontal pan when rendered content exceeds viewport width

- Search:
  - full-text substring search
  - case-sensitive and case-insensitive matcher modes
  - asynchronous scanning with progress updates

- History:
  - back/forward navigation history
  - jump-to-entry via history palette

- Rendering and performance:
  - parallel render workers
  - L1 rendered-page cache
  - L2 terminal-frame cache
  - scheduler-driven prefetch

- Terminal protocol handling:
  - protocol negotiation through presenter/picker flow
  - supported protocols include halfblocks, Sixel, Kitty, iTerm2 (environment dependent)

## Default key bindings

| Key      | Action |
|----------|--------|
| `j`      | Next page |
| `k`      | Previous page |
| `g`      | First page |
| `G`      | Last page |
| `+`      | Zoom in |
| `-`      | Zoom out |
| `h`      | Scroll left |
| `l`      | Scroll right |
| `/`      | Open search palette |
| `n`      | Next search hit |
| `N`      | Previous search hit |
| `Ctrl+O` | History back |
| `Ctrl+I` | History forward |
| `q`      | Quit |
| `Esc`    | Cancel / close current interactive surface |

Key behavior is configurable through keymap presets (`default`, `emacs`) in `config.toml`.

## Core runtime composition

- `App` orchestrates interaction, rendering, and UI draw flow.
- `DomainEvent` is the typed loop message boundary.
- Command execution emits typed `AppEvent` values.
- Extensions and palette providers are statically wired.

## Runtime configuration

`config.toml` controls render/cache/keymap/runtime tuning knobs.

Config lookup precedence:
1. `PVF_CONFIG_PATH`
2. `XDG_CONFIG_HOME/pvf/config.toml`
3. `HOME/.config/pvf/config.toml`
4. `APPDATA/pvf/config.toml`

If not found, built-in defaults are used.
