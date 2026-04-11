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
  - zoom in/out with a discrete ladder and bounded scale
  - `zoom <value>` clamps out-of-range values to the configured bounds and shows a warning notice
  - reset zoom to the fit baseline
  - fit-to-viewport behavior
  - pan when rendered content exceeds viewport bounds
  - command forms: `zoom <value>`, `zoom-reset`, `pan <left|right|up|down> [amount]`
  - bare `pan <direction>` uses a viewport-relative default step based on one fifth of the visible short edge
  - explicit `pan <direction> <amount>` keeps `amount` as an exact cell count

- Search:
  - full-text substring search
  - case-sensitive and case-insensitive matcher modes
  - asynchronous scanning with progress updates
  - compact search status segment in chrome/status bar while active
  - `<esc>` in normal mode cancels active search state after any pending key sequence either consumes `<esc>` or clears
  - command form: `cancel-search` cancels active search state when search is active
  - command form: `search` opens the dedicated search palette
  - search palette keeps a recent query history
  - while search palette is open, `<up>` / `<down>` navigate query history and `<c-p>` / `<c-n>` select the matcher
  - `next-search-hit` / `prev-search-hit` are available only while search is active

- Command palette:
  - keeps a recent command input history
  - while command palette is open, `<up>` / `<down>` navigate command history and `<c-p>` / `<c-n>` move the candidate selection

- Help:
  - command form: `help` opens the modal shortcut help overlay
  - `?` opens the modal shortcut help overlay
  - the overlay shows the current global keymap bindings
  - `<esc>` closes the help overlay

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

Printable key bindings are specified by the resulting character, not by a physical
key plus `Shift`. For example, `A`, `?`, and `:` are distinct literal bindings,
while `Shift+A` and `Shift+/` are not separate binding forms. This keeps bindings
stable across keyboard layouts: a binding for `?` matches the `?` character the
terminal reports, regardless of which keys produced it.

| Key      | Action |
|----------|--------|
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
| `/`      | Open search palette |
| `n`      | Next search hit |
| `N`      | Previous search hit |
| `<c-o>` | History back |
| `<c-i>` | History forward |
| `?`      | Open shortcut help |
| `q`      | Quit |
| `<esc>`  | Cancel / close current interactive surface |

## Core runtime composition

- `App` orchestrates interaction, rendering, and UI draw flow.
- `DomainEvent` is the typed loop message boundary.
- Command execution emits typed `AppEvent` values.
- Command definitions carry visibility, invocation, and availability metadata.
- Extensions and palette providers are statically wired.

## Runtime configuration

`config.toml` controls render/cache/runtime tuning knobs.

Config lookup precedence:
1. `PVF_CONFIG_PATH`
2. `XDG_CONFIG_HOME/pvf/config.toml`
3. `HOME/.config/pvf/config.toml`
4. `APPDATA/pvf/config.toml`

If not found, built-in defaults are used.
