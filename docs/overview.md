# pvf Specification Overview

`pvf` is a Rust CLI/TUI PDF viewer that renders pages as terminal images.

## CLI contract

Command:

```bash
pvf <file.pdf>
pvf perf <file.pdf> --scenario <scenario-id> [--out <path|->]
```

Rules:
- Exactly one PDF path argument is required.
- The path is opened through the default backend factory (`open_default_backend`).
- Perf mode runs a built-in scenario, emits JSON, and exits.
- Perf mode is accessed through the `perf` subcommand.
- Supported perf scenarios:
  - `page-flip-forward`
  - `page-flip-backward`
  - `idle-pending-redraw`

## Functional specification

- Navigation:
  - next/previous page
  - first/last page
  - direct page jump
  - in spread layout, next/previous move by 2 pages

- Page layout:
  - `single` (default)
  - `spread` (2-page side-by-side: 1-2, 3-4, ...)
  - spread direction: `ltr` / `rtl` (display order only)
  - runtime switch via commands: `page-layout-single`, `page-layout-spread [ltr|rtl]`

- Zoom and viewport:
  - zoom in/out with bounded scale
  - fit-to-viewport behavior
  - scroll when rendered content exceeds viewport bounds
  - command forms: `zoom <value>`, `scroll <left|right|up|down> [amount]`

- Search:
  - full-text substring search
  - case-sensitive and case-insensitive matcher modes
  - asynchronous scanning with progress updates
  - compact search status segment in chrome/status bar while active
  - `Esc` in normal mode cancels active search state
  - command form: `search` opens the dedicated search palette
  - `next-search-hit` / `prev-search-hit` are available only while search is active

- Help:
  - `?` opens a modal shortcut help overlay
  - the overlay shows the effective shortcuts for the active keymap preset
  - `Esc` closes the help overlay

- History:
  - back/forward navigation history
  - jump-to-entry via history palette
  - reason-based history recording (`Goto`/`Search`/`Outline`)

- Outline:
  - PDF outline/bookmark extraction
  - command form: `outline` opens the outline palette
  - outline palette shows a hierarchical list with indentation
  - selecting an outline item jumps to the linked page
  - unsupported/non-page destinations are ignored during extraction
  - documents without usable outline entries open an empty outline palette state

- Rendering and performance:
  - parallel render workers
  - L1 rendered-page cache
  - L2 terminal-frame cache
  - scheduler-driven prefetch
  - cold start may show a lower-resolution preview before the full-resolution visible page or spread is ready
  - once shown, the cold-start preview remains visible until the full-resolution current page or spread is ready

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
| `H` / `J` / `K` / `L` | Scroll |
| `/`      | Open search palette |
| `n`      | Next search hit |
| `N`      | Previous search hit |
| `Ctrl+O` | History back |
| `Ctrl+I` | History forward |
| `?`      | Open shortcut help |
| `q`      | Quit |
| `Esc`    | Cancel / close current interactive surface |

Key behavior is configurable through keymap presets (`default`, `emacs`) in `config.toml`.

## Core runtime composition

- `App` orchestrates interaction, rendering, and UI draw flow.
- `DomainEvent` is the typed loop message boundary.
- Command execution emits typed `AppEvent` values.
- Command definitions carry visibility, invocation, and availability metadata.
- Extensions and palette providers are statically wired.

## Runtime configuration

`config.toml` controls render/cache/keymap/runtime tuning knobs.

Config lookup precedence:
1. `PVF_CONFIG_PATH`
2. `XDG_CONFIG_HOME/pvf/config.toml`
3. `HOME/.config/pvf/config.toml`
4. `APPDATA/pvf/config.toml`

If not found, built-in defaults are used.
