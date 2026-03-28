# pvf

`pvf` is a PDF viewer for the terminal.

```bash
cargo run --release -- <file.pdf>
cargo run --release -- perf <file.pdf> --scenario page-flip-forward --out report.json
```

## Features

- Single or spread layout
- Zoom
- Full-text search with next/previous hit navigation
- Page history (back/forward)
- Command palette for actions like `goto-page` and `page-layout-spread`
- Help overlay via `help`

## Keys (default)

| Key | Action |
|---|---|
| `j` / `k` | Next page / Previous page |
| `g` / `G` | First page / Last page |
| `+` / `-` | Zoom in / Zoom out |
| `0` | Reset zoom |
| `H` / `J` / `K` / `L` | Scroll |
| `/` | Open search palette |
| `n` / `N` | Next search hit / Previous search hit |
| `Ctrl+O` / `Ctrl+I` | History back / History forward |
| `?` | Open help overlay |
| `:` | Open command palette |
| `Esc` | Cancel current interactive state |
| `q` | Quit |

## Common Commands

- `goto-page <number>`
- `page-layout-spread [ltr|rtl]`
- `zoom <value>`
- `search`
- `history`

## Configuration (optional)

`pvf` loads `config.toml` from:
1. `PVF_CONFIG_PATH`
2. `XDG_CONFIG_HOME/pvf/config.toml`
3. `HOME/.config/pvf/config.toml`
4. `APPDATA/pvf/config.toml`

Minimal example:

```toml
[keymap]
preset = "default" # or "emacs"
```

## Note

Image quality and compatibility depend on terminal image protocol support (Kitty, Sixel, iTerm2, or halfblocks fallback).
