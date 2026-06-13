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
pvf -w|--watch <file.pdf>
pvf --no-watch <file.pdf>
pvf -c|--config <config.toml> <file.pdf>
pvf --no-config <file.pdf>
pvf -p|--page <page-number> <file.pdf>
pvf -z|--zoom <fit-ratio> <file.pdf>
pvf -l|--layout <single|spread> <file.pdf>
```

Rules:

- Exactly one PDF path argument is required.
- The document is opened through the default backend factory.
- `-w`, `--watch` enables automatic reload of the displayed document when the
  input file changes.
- `--no-watch` disables automatic reload for the current process.
- `-c`, `--config <path>` reads app options from a specific TOML file and
  requires that path to exist.
- `--no-config` skips configuration file loading.
- `--config` and `--no-config` are mutually exclusive.
- `--watch` and `--no-watch` are mutually exclusive.
- `-p`, `--page <page-number>` sets the initial one-based page.
- `-z`, `--zoom <fit-ratio>` sets the initial zoom ratio relative to fit.
- `-l`, `--layout <single|spread>` sets the initial layout.
- Performance diagnostics are developer tooling and are not part of the public
  viewer CLI. See `performance-diagnostics.md`.

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
  - spread cover policy supports `paired` and `cover`
    - `paired` is the default and groups pages as `1-2`, `3-4`, ...
    - `cover` shows page 1 by itself, then groups pages as `2-3`, `4-5`, ...
  - runtime commands are `page-layout-single` and
    `page-layout-spread [ltr|rtl] [paired|cover]`
  - command arguments are positional; specifying a spread cover policy requires
    specifying a spread direction first, e.g. `page-layout-spread ltr cover`

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
  - each page is searched first with the query text as entered; when that finds
    no hit on the page, search falls back to a whitespace-insensitive substring
    match to tolerate PDF text extraction that omits or inserts whitespace
  - search runs asynchronously and reports progress while active
  - positioned text extraction may be prewarmed in the background from the
    start of the document so later searches can reuse cached page text
  - search text and highlight geometry are cached separately; cached text can
    complete later searches even when highlight geometry has been evicted
  - missing cached highlight geometry is resolved in the background, with the
    current visible page prioritized over broad prewarm work
  - visible search hits are highlighted through the generic highlight overlay
    layer
  - if highlight extraction fails for matched pages, search results are kept and
    a concise warning notice is shown
  - `search` opens the search palette
  - `search-results` opens the search hit list palette while search is active
  - search-results rows show hit index, context snippet, and page label
  - the search-results palette can open even when there are zero hits
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

- Reload
  - `reload-document` reopens the current PDF path
  - with watch enabled, the runtime polls the current PDF path and reloads
    after a settle delay when file metadata stabilizes
  - reload success replaces the active document, clamps the current page to the
    new page count, resets render work, clears presenter output cache, and
    prewarms search text for the new document
  - active search is submitted again against the new document using the same
    query and matcher; cached outline data is cleared
  - reload failure keeps the previous document visible
  - manual reload failures show an error immediately
  - watch reload failures retry quietly with short backoff before showing a
    warning, so a save that temporarily leaves a partial PDF on disk can recover
    without user action
  - acceptance cases for watch:
    - replacing a valid PDF with another valid PDF updates the displayed
      document without restarting the viewer
    - replacing a valid PDF with a temporarily invalid file keeps the previous
      document visible while retrying
    - replacing that temporarily invalid file with a valid PDF during the retry
      window updates the viewer and clears retry state
    - repeated reload failures eventually show a warning instead of retrying
      forever
    - a newer file-change request takes precedence over an older failed reload
      result

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

Configuration fields, lookup order, and precedence are specified in
`configuration.md`.

Resolved ownership:

- worker count is consumed when render workers are spawned
- initial page, zoom, and layout are consumed when `AppState` is constructed
- spread direction and cover defaults live in the view policy
- input polling and redraw timing live in the event-loop policy
- prefetch dispatch budget lives in the event-loop policy
- render scale bounds live in the render policy
- L1 cache limits are consumed by render runtime construction
- L2 cache limits are consumed by presenter construction
- key bindings are resolved into a `SequenceRegistry` before input handling
- watch enablement and timing live in the watch policy

Programmatic construction uses the same resolver without requiring TOML:

- create `AppOptions`
- apply one or more option patches through the resolver or
  `AppBuilder::merge_options`
- build `App` from the resolved feature policies

## Code references

- `src/config.rs`
- `src/command/spec.rs`
- `src/input/keymap.rs`
- `src/ui/help.rs`
