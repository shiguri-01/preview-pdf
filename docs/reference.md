# Reference

This file indexes stable developer-facing contracts. It is not a full
specification and does not own complete inventories. Each section names what
must remain true, what needs compatibility care, where the full detail lives in
code, and what test areas should protect it.

Reference material should preserve useful overview without pretending to be the
implementation. A compact category list, lifecycle sketch, or representative
example is fine when it helps contributors reason about a change. A complete
command list, keymap table, provider list, config field list, or cache algorithm
belongs in the owning code unless generated docs or explicit drift tests keep it
current.

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

Use `Orientation:` sparingly when a short map makes the contract easier to use
in review. Keep it descriptive rather than exhaustive.

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
- [src/main.rs](../src/main.rs)
- [src/config/](../src/config/)

Test coverage:
- CLI parser tests in [src/main.rs](../src/main.rs).
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
- Top-level `[keymap]` config patches the normal-mode key sequence registry.
- Validation and sanitization that users can observe, such as enum rejection,
  keymap preset and command validation, safe duration minimums, and zoom bounds,
  are part of the config contract.

Compatibility:
- Supported config fields and enum values are compatibility-sensitive.
- Do not document the complete TOML inventory here; keep it in config types,
  parsing code, and tests.

Owned by:
- [src/config/types.rs](../src/config/types.rs)
- [src/config/file.rs](../src/config/file.rs)
- [src/config/options.rs](../src/config/options.rs)
- [src/config/policy.rs](../src/config/policy.rs)
- [src/main.rs](../src/main.rs)

Test coverage:
- Config file parser and resolver tests in [src/config/](../src/config/).
- CLI config selection tests in [src/main.rs](../src/main.rs).

## Commands

Orientation:
- Commands have five review-relevant concerns: stable ids and argument parsing,
  role, source-aware invocation policy, target requirement, and dispatch
  effects. The command catalog ties identity and routing concerns together;
  feature behavior stays in handlers, active targets, and app state.

Contract:
- Command ids are canonical kebab-case strings and are compatibility-sensitive
  when public.
- The command catalog owns command ids, metadata, parser routing, and dispatch
  routing.
- Typed commands must have matching registry metadata.
- Command roles distinguish user intent commands, surface interaction controls,
  and internal effects.
- Public commands may appear in user-facing command surfaces when their
  invocation policy and `enabled_when` runtime condition allow it.
- Internal commands are runtime plumbing and must not appear in the command
  palette.
- Keymap-only commands can be invoked from key bindings but not from direct
  command palette input.
- Interaction commands can be invoked by key bindings or interaction flows but
  are hidden from user-facing command surfaces.
- Internal-only commands can be invoked only by interaction flows that complete
  another user action.
- `enabled_when` checks are separate from invocation policy.
- Target resolution is separate from invocation policy and `enabled_when`.
  Palette interaction commands, including palette input editing, require an
  active palette. Help interaction commands require active help.
- `enabled_when` may depend on runtime app state such as active search, help
  mode, palette kind, or palette input history availability. Target
  requirements are not duplicated in `enabled_when`.
- Palette input history availability means an active palette whose kind supports
  input history; the owning predicate lives with `PaletteKind`.
- Command-palette listing, help display, typed command submission, and dispatch
  use command policy functions to decide how exposure, invocation policy,
  target, and `enabled_when` apply to that surface.
- Command-palette listing shows public user-intent commands only when the
  command-palette input source is allowed, target requirements are satisfied,
  and `enabled_when` is currently satisfied.
- Typed command submission is separate from listing: a known typed command is
  parsed and then validated by dispatch policy, so "not listed" does not mean
  "unknown".
- Help surfaces may describe configured commands and bindings without hiding
  them solely because `enabled_when` is currently false; a surface that claims
  to show only currently runnable actions must evaluate `enabled_when`.
- Runtime condition vocabulary is shared by commands and key bindings. A
  palette-kind condition is true only when a palette is open and its active
  kind matches; a closed palette does not match any kind.
- Command handlers return `CommandExecution`: an `Applied` or `Noop` outcome
  plus `CommandEffects` for notice changes, explicit app events, palette
  requests, input-history records, follow-up command requests, and lifecycle
  requests. Handlers may mutate their owned feature state through the execution
  context, but they must not directly push runtime queues or record input
  history.
- Process lifecycle requests are command effects, not command outcomes. For
  example, quit is an applied command with a quit lifecycle effect.
- Dispatch applies command effects in one place, then emits transition events
  and the final command execution event.
- Command dispatch emits command execution events after validation and dispatch
  complete, including rejected commands.
- Command dispatch may return follow-up command requests, for example when a
  palette submit completes a user intent or internal effect command.

Known follow-ups:
- Search command intent: `search` is the public search entry point today and
  starts a valid search flow by opening the search palette. Direct query
  submission still happens through the internal `submit-search` command after
  palette submit. When this area is redesigned, keep `search` as the
  user-intent command and make the palette an input-collection path for that
  command instead of exposing `submit-search` as a user-facing command.

Compatibility:
- Public command ids, argument compatibility, and user-facing parser behavior
  require migration care.
- Internal command ids can change more freely, but cross-module callers and
  palette providers must be updated together.
- Do not copy the full command inventory into docs. Keep the complete list in
  the catalog; use docs for policy, categories, and review cues.

Owned by:
- [src/command/catalog.rs](../src/command/catalog.rs)
- [src/command/parse.rs](../src/command/parse.rs)
- [src/command/spec.rs](../src/command/spec.rs)
- [src/command/dispatch.rs](../src/command/dispatch.rs)
- [src/command/handlers/](../src/command/handlers/)
- [src/condition.rs](../src/condition.rs)

Test coverage:
- Command registry, parser, validation, and dispatch tests in [src/command/](../src/command/).
- Command-palette provider tests where command metadata affects palette UI.

## Key Bindings

Orientation:
- Terminal key events are converted to typed command requests before behavior is
  applied. Normal-mode keys and scoped surface keys are resolved by the same
  scoped sequence registry.

Contract:
- Printable bindings are defined by resulting characters, not by physical keys.
- Configured key bindings use the same key labels shown in help, such as
  `gg`, `<c-o>`, `<down>`, and `[count]G`.
- Key bindings have a scope, currently normal, palette, or help. A binding only
  resolves when the active key binding context matches its scope and
  `enabled_when` runtime condition.
- Key binding `enabled_when` uses the same runtime condition vocabulary as
  command `enabled_when`; do not add a separate keymap-only condition enum.
- Multi-key sequences can remain pending until resolved or timed out.
- Numeric prefixes are parsed by the input sequence layer and dispatch typed
  commands.
- Built-in key bindings must reference known command ids and satisfy command
  invocation policy.
- Configured key bindings currently target the normal scope and must reference
  known commands that can be invoked from the keymap. Like built-in bindings,
  they resolve only when their scope and runtime `enabled_when` match; dispatch
  still validates the resolved command before applying behavior.
- Palette-scoped keys dispatch hidden palette interaction commands such as
  submit, complete, selection movement, input editing, and palette input
  history recall.
- Help-scoped keys dispatch hidden help interaction commands such as close and
  scroll.
- `<esc>` is a scoped built-in binding for cancellation or close behavior. When
  a multi-key sequence is already pending, `<esc>` clears the pending sequence
  instead of dispatching another command.

Compatibility:
- Changing a default key binding affects user muscle memory and help output; do
  it intentionally with tests.
- Complete key inventories belong in `src/input/keymap.rs` and rendered help.
  Docs may summarize categories of bindings, but should not own the table.

Owned by:
- [src/input/keymap.rs](../src/input/keymap.rs)
- [src/input/sequence.rs](../src/input/sequence.rs)
- [src/input/shortcut.rs](../src/input/shortcut.rs)
- [src/ui/help.rs](../src/ui/help.rs)

Test coverage:
- Sequence and keymap tests in [src/input/](../src/input/).
- Command/keymap consistency tests under the command module.

## Palette

Orientation:
- Palette behavior splits into common session mechanics and provider-owned
  semantics. The common path owns opening, palette input state, selection,
  completion, submit, and closing. Palette-scoped key bindings turn terminal
  keys into commands; providers own candidate meaning and the effects returned
  for completion and submit.

Contract:
- A palette session has a kind, session id, input state, candidate list,
  visible candidate indexes, selection, optional open payload, and optional
  assistive text.
- Palette providers own candidate generation, input mode, initial input,
  completion effects, submit effects, assistive text, and provider-specific
  selection defaults.
- `PaletteManager` owns common open, cancel, palette input operations, palette
  input history recall for palettes that support it, selection, completion,
  submit, and session-id validation behavior.
- Candidate search text is independent from rendered row text.
- Provider submit effects describe palette-local meaning: close, reopen, or
  dispatch a typed command with optional history recording and a post action.
  The palette submit command handler converts those provider effects into
  command runtime effects; providers do not write command follow-up queues or
  input history directly.
- Command-palette visibility derives from command metadata, invocation policy,
  target availability, and `enabled_when`, not from a hand-written UI list.
- Input history is an opt-in palette input capability; it is not a
  provider-specific palette action.

Observable behavior:
- Escape dispatches the palette close command when a palette is active.
- Control-p/control-n dispatch palette selection commands.
- Up/down dispatch palette input history commands for palettes that support
  history; otherwise they dispatch palette selection commands.
- Tab dispatches palette completion.
- Enter dispatches palette submit.
- Palette input editing preserves common line-editing behavior through scoped
  `text.*` interaction commands, including cursor movement, word movement,
  word/line deletion, and yank.
- Empty candidate lists can still represent valid interactive states when the
  provider supports that behavior.

Compatibility:
- Palette input, tab, submit, cancel, and selection behavior is user-visible
  and should change only with focused tests.
- Complete provider inventories belong in the registry and provider modules.
  Docs should explain provider responsibilities and notable cross-palette rules.

Owned by:
- [src/palette/manager.rs](../src/palette/manager.rs)
- [src/palette/registry.rs](../src/palette/registry.rs)
- [src/palette/types.rs](../src/palette/types.rs)
- [src/palette/providers/](../src/palette/providers/)
- [src/search/palette.rs](../src/search/palette.rs)
- [src/history/palette.rs](../src/history/palette.rs)
- [src/outline/palette.rs](../src/outline/palette.rs)

Test coverage:
- Palette manager and provider tests in [src/palette/](../src/palette/),
  [src/search/](../src/search/), [src/history/](../src/history/), and
  [src/outline/](../src/outline/).

## Extensions

Orientation:
- Built-in extensions are internal runtime features that need their own state,
  event observation, background progress, status-bar output, or palette-facing
  snapshots.

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
- [src/extension/traits.rs](../src/extension/traits.rs)
- [src/extension/host.rs](../src/extension/host.rs)
- [src/search/](../src/search/)
- [src/history/](../src/history/)
- [src/outline/](../src/outline/)
- [src/event.rs](../src/event.rs)

Test coverage:
- Extension host tests when adding or changing hook order or event propagation.
- Feature tests in search, history, and outline modules for extension-owned
  behavior.

## Rendering And Workers

Orientation:
- Rendering correctness depends on two receiver boundaries: render results must
  still match current app state, and presenter encode results must still match
  current terminal-frame identity.

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
- [src/render/scheduler.rs](../src/render/scheduler.rs)
- [src/render/prefetch.rs](../src/render/prefetch.rs)
- [src/render/worker.rs](../src/render/worker.rs)
- [src/presenter/encode.rs](../src/presenter/encode.rs)
- [src/presenter/l2_cache.rs](../src/presenter/l2_cache.rs)
- [src/app/render_ops.rs](../src/app/render_ops.rs)
- [src/search/engine.rs](../src/search/engine.rs)
- [src/search/state.rs](../src/search/state.rs)

Test coverage:
- Render worker and scheduler tests in [src/render/](../src/render/).
- Presenter cache and encode tests in [src/presenter/](../src/presenter/).
- Runtime worker tests in [src/app/tests/](../src/app/tests/).
- Search generation tests in [src/search/](../src/search/).

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
- [src/render/cache.rs](../src/render/cache.rs)
- [src/presenter/l2_cache.rs](../src/presenter/l2_cache.rs)
- [src/app/runtime/prepare.rs](../src/app/runtime/prepare.rs)
- [src/app/runtime/spread_canvas.rs](../src/app/runtime/spread_canvas.rs)

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
- [benches/perf.rs](../benches/perf.rs)
- [benches/fixtures/](../benches/fixtures/)
- [src/perf/](../src/perf/)
- [src/app/perf_runner.rs](../src/app/perf_runner.rs)

Test coverage:
- [src/perf/](../src/perf/) tests for scenario parsing, validation, summary shape, and report
  serialization.
- Bench runs and diagnostics for performance observation.
