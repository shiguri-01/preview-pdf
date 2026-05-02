# pvf (preview-pdf)

`pvf` is a PDF viewer for the terminal.

```bash
pvf <file.pdf>
```

## Development

With Nix flakes enabled, enter the project development shell:

```bash
nix develop
```

Or allow direnv to load it automatically:

```bash
direnv allow
```

## Highlights

- Keyboard-first PDF viewer for the terminal
- Single-page and spread layout
- Full-text search across the document
- Outline navigation for structured PDFs
- Command palette for all viewer actions

## Keys

| Key | Action |
|---|---|
| `j` / `k` | Next page / Previous page |
| `gg` / `G` | First page / Last page |
| `[count]G` | Go to page `count` |
| `+` / `-` | Zoom in / Zoom out |
| `=` | Reset zoom |
| `H` / `J` / `K` / `L` | Pan |
| `/` | Open search palette |
| `n` / `N` | Next search hit / Previous search hit |
| `<c-o>` / `<c-i>` | History back / History forward |
| `?` | Open help overlay |
| `:` | Open command palette |
| `<esc>` | Cancel current interactive state |
| `q` | Quit |

## Note

Image quality and compatibility depend on terminal image protocol support such as Kitty, Sixel, or iTerm2.
