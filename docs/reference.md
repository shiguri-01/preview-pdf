# Reference

This file indexes stable developer-facing contracts. It is not a full
specification and does not own complete inventories. Each section names what
must remain true, what needs compatibility care, where the full detail lives in
code, and what test areas should protect it.

Use this section shape for new stable-contract sections:

```text
## <Area>

Contract:
- Stable behavior that must remain true, or that must change only intentionally.

Compatibility:
- What kinds of changes require migration care, deprecation, or explicit review.

Owned by:
- Code entry points that own the complete inventory or implementation detail.

Test coverage:
- Test areas expected to protect the contract.
```

Use `Observable behavior:` only when user-visible or cross-subsystem effects
need to be separated from private implementation detail.

## CLI

Contract:
- The viewer requires exactly one PDF path.
- Watch, config, initial page, initial zoom, and initial layout can be provided
  through CLI options.
- Mutually exclusive CLI flags are rejected before the viewer starts.
- Initial page values are user-facing one-based page numbers.
- Initial zoom is a fit-relative ratio.
- Performance diagnostics are developer tooling run through Cargo, not the
  public viewer CLI.

Compatibility:
- Changing or removing a public CLI option requires explicit review, tests, and
  docs updates.
- Error and exit behavior visible to shell users should change only
  intentionally.

Owned by:
- `src/main.rs`
- `src/config/`

Test coverage:
- CLI parser tests in `src/main.rs`.
- Process-level integration tests if exit codes or stderr/stdout behavior need
  protection.

## Configuration

Contract:
- Resolved options are built from patches over built-in defaults.
- Source precedence is: built-in defaults, default config file when enabled,
  CLI options for the current process, then explicit runtime command arguments.
- Default config lookup checks `PVF_CONFIG_PATH`,
  `XDG_CONFIG_HOME/pvf/config.toml`, `HOME/.config/pvf/config.toml`, then
  `APPDATA/pvf/config.toml`.
- If no default config file resolves, built-in defaults are used.
- `--config <path>` reads a specific TOML file and requires an existing regular
  file.
- `--no-config` skips config-file loading.
- `--config` and `--no-config` are mutually exclusive.
- Partial config files leave unspecified options absent so later sources and
  defaults can still apply.
- Validation and sanitization that users can observe, such as enum rejection,
  safe duration minimums, and zoom bounds, are part of the config contract.

Compatibility:
- Supported config fields and enum values are compatibility-sensitive.
- Unknown legacy sections may be tolerated intentionally, but new compatibility
  behavior should be covered by tests.
- Do not document the complete TOML inventory here; keep it in config types,
  parsing code, and tests.

Owned by:
- `src/config/types.rs`
- `src/config/file.rs`
- `src/config/options.rs`
- `src/config/policy.rs`
- `src/main.rs`

Test coverage:
- Config file parser and resolver tests in `src/config/`.
- CLI config selection tests in `src/main.rs`.

## Commands

Contract:
- Command ids are canonical kebab-case strings and are compatibility-sensitive
  when public.
- The command catalog owns command ids, metadata, parser routing, and dispatch
  routing.
- Typed commands must have matching registry metadata.
- Public commands may appear in user-facing command surfaces when their
  invocation and availability policies allow it.
- Internal commands are runtime plumbing and must not appear in the command
  palette.
- Keymap-only commands can be invoked from key bindings but not from direct
  command palette input.
- Internal-only commands can be invoked only by provider-driven internal flows.
- Availability checks are separate from invocation policy.
- Command dispatch emits command execution events after validation and dispatch
  complete, including rejected commands.

Compatibility:
- Public command ids, argument compatibility, and user-facing parser behavior
  require migration care.
- Internal command ids can change more freely, but cross-module callers and
  palette providers must be updated together.
- Do not copy the command inventory into docs; update the catalog and
  consistency tests.

Owned by:
- `src/command/catalog.rs`
- `src/command/parse.rs`
- `src/command/spec.rs`
- `src/command/dispatch.rs`
- `src/command/handlers/`

Test coverage:
- Command registry, parser, validation, and dispatch tests in `src/command/`.
- Command-palette provider tests where command metadata affects palette UI.

## Key Bindings

Contract:
- Printable bindings are defined by resulting characters, not by physical keys.
- Multi-key sequences can remain pending until resolved or timed out.
- Numeric prefixes are parsed by the input sequence layer and dispatch typed
  commands.
- Built-in key bindings must reference known command ids and satisfy command
  invocation policy.
- Palette-local key handling is owned by palette behavior, not the normal-mode
  keymap.

Compatibility:
- Changing a default key binding affects user muscle memory and help output; do
  it intentionally with tests.
- Complete key inventories belong in `src/input/keymap.rs` and rendered help,
  not in docs.

Owned by:
- `src/input/keymap.rs`
- `src/input/sequence.rs`
- `src/input/shortcut.rs`
- `src/ui/help.rs`

Test coverage:
- Sequence and keymap tests in `src/input/`.
- Command/keymap consistency tests under the command module.

## Palette

Contract:
- A palette session has a kind, session id, input state, candidate list,
  visible candidate indexes, selection, optional open payload, and optional
  assistive text.
- Palette providers own candidate generation, input mode, initial input, tab
  effects, submit effects, assistive text, and provider-specific selection
  defaults.
- `PaletteManager` owns common open, cancel, input, history recall, selection,
  tab, submit, and session-id validation behavior.
- Candidate search text is independent from rendered row text.
- Submit effects may close, reopen, or dispatch a typed command with optional
  history recording and a post action.
- Command-palette visibility derives from command metadata and availability,
  not from a hand-written UI list.

Observable behavior:
- Escape closes the active palette.
- Control-p/control-n move selection.
- Up/down recall input history only for palettes that support it; otherwise
  they move selection.
- Tab applies provider tab behavior.
- Enter applies provider submit behavior.
- Empty candidate lists can still represent valid interactive states when the
  provider supports that behavior.

Compatibility:
- Palette input, tab, submit, cancel, and selection behavior is user-visible
  and should change only with focused tests.
- Complete provider inventories belong in the registry and provider modules.

Owned by:
- `src/palette/manager.rs`
- `src/palette/registry.rs`
- `src/palette/types.rs`
- `src/palette/providers/`
- `src/search/palette.rs`
- `src/history/palette.rs`
- `src/outline/palette.rs`

Test coverage:
- Palette manager and provider tests in `src/palette/`, `src/search/`,
  `src/history/`, and `src/outline/`.

## Extensions

Contract:
- Extensions are internal modules composed statically by `ExtensionHost`.
- Extension state remains concrete and owned by the host.
- Extension hooks operate on extension-owned state plus shared app state.
- Input hooks return ignored when they do not claim an input.
- The first claimed input hook result wins.
- Event hooks observe typed `AppEvent` values emitted by command dispatch and
  runtime flow.
- Background hooks report whether visible or behavioral state changed.
- Extension UI data exposed to palettes crosses through `ExtensionUiSnapshot`.

Compatibility:
- Hook order, event propagation, background draining, and status-bar projection
  can affect user-visible behavior and should change only with tests.
- This is not a dynamic plugin API; do not document it as one.

Owned by:
- `src/extension/traits.rs`
- `src/extension/host.rs`
- `src/search/`
- `src/history/`
- `src/outline/`
- `src/event.rs`

Test coverage:
- Extension host tests when adding or changing hook order or event propagation.
- Feature tests in search, history, and outline modules for extension-owned
  behavior.

## Rendering And Workers

Contract:
- Render work returns typed completion results that are accepted or dropped at
  the runtime boundary.
- Stale, canceled, or superseded render results must not replace newer app
  state.
- Current visible pages have priority over prefetch work.
- Active PDF rendering and active terminal encoding may run to completion even
  when queued metadata is canceled; receivers decide whether results still
  apply.
- Search worker events are applied by generation so stale search results do not
  update active search state.
- Encode completions carry enough identity for presenter cache and generation
  checks.

Observable behavior:
- Cold start may show a lower-resolution preview before the full-resolution
  current view is ready.
- A frame should show image content, loading state, or error state rather than
  regressing to a clear-only viewer.
- Reload success replaces the active document, clamps the page, resets render
  work, clears presenter cache, and refreshes extension-owned derived data.
- Reload failure keeps the previous document visible.

Compatibility:
- Stale-result and cancellation behavior is correctness-sensitive and should be
  protected with tests before changing.
- Detailed scheduling and cache algorithms belong in render and presenter code,
  not in docs.

Owned by:
- `src/render/scheduler.rs`
- `src/render/prefetch.rs`
- `src/render/worker.rs`
- `src/presenter/encode.rs`
- `src/presenter/l2_cache.rs`
- `src/app/render_ops.rs`
- `src/search/engine.rs`
- `src/search/state.rs`

Test coverage:
- Render worker and scheduler tests in `src/render/`.
- Presenter cache and encode tests in `src/presenter/`.
- Runtime worker tests in `src/app/tests/`.
- Search generation tests in `src/search/`.

## Caches

Contract:
- L1 cache identity includes document identity, page identity, render scale, and
  layout identity where applicable.
- L1 stores raw rendered page frames; overlays are applied after raw retrieval.
- L2 cache identity includes terminal-frame inputs that affect encoded output,
  including viewport, effective pan, and overlay stamp.
- Cache memory policies may evict old entries, but current critical entries can
  receive special handling to avoid a blank viewer.

Compatibility:
- Cache details are internal unless callers or users can observe the effect,
  such as stale fallback, blank-frame avoidance, or document identity changes.

Owned by:
- `src/render/cache.rs`
- `src/presenter/l2_cache.rs`
- `src/app/runtime/prepare.rs`
- `src/app/runtime/spread_canvas.rs`

Test coverage:
- Cache unit tests and presenter/runtime tests that assert observable fallback
  or identity behavior.

## Performance Diagnostics

Contract:
- Performance diagnostics are developer observability, not correctness tests.
- The bench entry point runs headless viewer scenarios and can emit JSON
  reports.
- Normal tests may protect JSON shape, parser behavior, scenario metadata, and
  validation rules.
- Normal tests must not depend on exact timing, throughput, or performance
  numbers.

Compatibility:
- JSON report fields and scenario ids are developer-facing and should change
  intentionally.

Owned by:
- `benches/perf.rs`
- `benches/fixtures/`
- `src/perf/`
- `src/app/perf_runner.rs`

Test coverage:
- `src/perf/` tests for scenario parsing, validation, summary shape, and report
  serialization.
- Bench runs and diagnostics for performance observation.
