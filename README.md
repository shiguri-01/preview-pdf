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

## Keys

| Key | Action |
|---|---|
| `j` / `k` | Next page / Previous page |
| `g` / `G` | First page / Last page |
| `+` / `-` | Zoom in / Zoom out |
| `0` | Reset zoom |
| `H` / `J` / `K` / `L` | Pan |
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

## Note

Image quality and compatibility depend on terminal image protocol support (Kitty, Sixel, iTerm2, or halfblocks fallback).
