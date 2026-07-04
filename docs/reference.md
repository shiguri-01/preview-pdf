# Reference

This file indexes stable developer-facing contracts. It is not a full
specification and does not own complete inventories. Use it to find what must
remain true, what needs compatibility care, and where the owning code lives.

Entries should make review obligations explicit without copying implementation
detail. Use `Contract:`, `Compatibility:`, and `Owned by:` so each entry says
what must remain true, what kind of change needs care, and where the complete
detail is maintained. Add orientation or observable behavior only when review
would be ambiguous without it.

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
- [src/cli.rs](../src/cli.rs)
- [src/config/](../src/config/)

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
- `[[keymap]]` config entries patch the shared conditional key sequence
  registry. Each entry has `when`, `key`, and `command` fields. `command` is
  either a command string or `false` to unbind the key.
- Validation and sanitization that users can observe, such as enum rejection,
  keymap condition and command validation, safe duration minimums, and zoom bounds,
  are part of the config contract.

Compatibility:
- Supported config fields and enum values are compatibility-sensitive.
- Do not document the complete TOML inventory here; keep it in config types,
  parsing code, and tests.

Owned by:
- [src/config/](../src/config/)
- [src/cli.rs](../src/cli.rs)

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
- Command roles distinguish user intent commands, surface controls, and
  internal effects.
- Public exposure, binding invocation, internal follow-ups, target
  requirements, and runtime enablement are separate policy concerns. Do not
  collapse them into one visibility check.
- Command-palette listing, help display, typed command submission, and dispatch
  use command policy functions to decide how exposure, invocation policy,
  target, and `enabled_when` apply to that surface.
- Typed command submission is separate from listing: a known typed command is
  parsed and then validated by dispatch policy, so "not listed" does not mean
  "unknown".
- Help surfaces may describe configured commands and keymap entries without hiding
  them solely because `enabled_when` is currently false; a surface that claims
  to show only currently runnable actions must evaluate `enabled_when`.
- Runtime condition vocabulary is shared by commands and keymap entries. A
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
- Command dispatch emits execution events after validation and dispatch
  complete, including rejected commands. Follow-up command requests are
  explicit command effects.

Compatibility:
- Public command ids, argument compatibility, and user-facing parser behavior
  require migration care.
- Internal command ids can change more freely, but cross-module callers and
  palette providers must be updated together.
- Do not copy the full command inventory into docs. Keep the complete list in
  the catalog; use docs for policy, categories, and review cues.

Owned by:
- [src/command/](../src/command/)
- [src/condition.rs](../src/condition.rs)

## Keymap

Orientation:
- Terminal key events are converted to typed command requests before behavior is
  applied. Normal-mode keys and surface keys are resolved by the same
  conditional sequence registry.

Contract:
- Printable keymap entries are defined by resulting characters, not by physical keys.
- Runtime `Meta` key events are normalized to the `Alt`/`<m-...>` shortcut
  representation. Unbound Alt-only chords are not treated as printable input.
- `keymap_preset` selects the starting keymap. Supported values are `default`
  and `none`.
- Configured keymap entries use the same key labels shown in help and resolve
  to typed command requests.
- Later configured bindings replace earlier bindings with the same condition
  selector and key sequence.
- Key binding conditions are normalized before priority is calculated; matching
  uses the normalized runtime condition state.
- Keymap `enabled_when` uses the same runtime condition vocabulary as
  command `enabled_when`; do not add a separate keymap-only condition enum.
- Every key input and sequence timeout resolves against the current runtime
  condition state. Pending sequences do not preserve the state in which they
  started. State transitions clear a pending sequence when it no longer
  matches any currently enabled binding.
- Multi-key sequences can remain pending until resolved or timed out.
- Numeric prefixes are parsed by the input sequence layer and dispatch typed
  commands.
- All keymap entries dispatch with the binding invocation source, reference known
  command ids, and satisfy command invocation policy.
- Surface-local keys, including palette and help keys, dispatch binding-only
  commands through the same command policy path as normal-mode bindings.
- When a multi-key sequence is already pending, `<esc>` clears the pending
  sequence instead of dispatching another command.

Compatibility:
- Changing a default keymap entry affects user muscle memory and help output; do
  it intentionally with tests.
- Complete key inventories belong in config keymap presets and rendered help.
  Docs may summarize categories of keymap entries, but should not own the table.

Owned by:
- [src/config/keymap/](../src/config/keymap/)
- [src/input/](../src/input/)
- [src/ui/help.rs](../src/ui/help.rs)

## Palette

Orientation:
- Palette behavior splits into common session mechanics and provider-owned
  semantics. The common path owns opening, palette input state, selection,
  completion, submit, and closing. Palette keymap entries turn terminal keys into
  commands when their runtime conditions match; providers own candidate meaning
  and the effects returned for completion and submit.

Contract:
- A palette session has a kind, session id, input state, candidate list,
  visible candidate indexes, selection, and optional assistive text.
- Palette open requests may seed only common input state: initial input text and
  an optional provider-scoped candidate id for initial selection.
  Provider-specific state does not travel through palette open requests.
- Palette providers own candidate generation, input mode, completion effects,
  submit effects, assistive text, and provider-specific selection defaults.
- `PaletteSessionController` owns common open, cancel, palette input
  operations, palette input history recall for palettes that support it,
  selection, completion, submit, and session-id validation behavior.
- Palette candidates expose a provider-scoped id plus semantic `label` and
  `detail` text. Renderers decide how those areas map to physical layout.
- Candidate match text is derived from matchable row cells. Display and matching
  share the same formatting path for structured values such as page labels; the
  exact row contents belong to provider code and tests.
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
- Common palette controls for close, selection, completion, submit, input
  editing, and optional input-history recall are user-visible compatibility
  points.
- Empty candidate lists can still represent valid interactive states when the
  provider supports that behavior.

Compatibility:
- Palette input, tab, submit, cancel, and selection behavior is user-visible
  and should change only with focused tests.
- Complete provider inventories belong in the registry and provider modules.
  Docs should explain provider responsibilities and notable cross-palette rules.

Owned by:
- [src/palette/](../src/palette/)
- [src/search/](../src/search/)
- [src/history/](../src/history/)
- [src/outline/](../src/outline/)

## Extensions

Orientation:
- Built-in extensions are internal runtime features that need their own state,
  event observation, status-bar output, worker results, or palette-facing
  snapshots.

Contract:
- Extensions are internal modules composed statically by `ExtensionHost`.
- Extension state remains concrete and owned by the host.
- `ExtensionHost` owns lifecycle routing, hook ordering, and shared snapshots.
- Feature operations are owned by feature runtime or state types.
- Extension hooks operate on extension-owned state plus shared app state.
- Input hooks return ignored when they do not claim an input.
- The first claimed input hook result wins.
- Event hooks observe typed `AppEvent` values emitted by command dispatch and
  runtime flow.
- Extension-owned worker output is routed through the app loop before it mutates
  extension-owned state.
- Document reload behavior is handled through extension lifecycle hooks. Each
  extension explicitly decides whether to reset, preserve, or rehydrate its
  state for the new document.
- Extension UI data exposed to palettes crosses through `ExtensionUiSnapshot`.

Compatibility:
- Hook order, event propagation, worker result handling, and status-bar
  projection can affect user-visible behavior and should change only with
  tests.
- This is not a dynamic plugin API; do not document it as one.

Owned by:
- [src/extension/](../src/extension/)
- [src/search/](../src/search/)
- [src/history/](../src/history/)
- [src/outline/](../src/outline/)
- [src/event.rs](../src/event.rs)

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
- Stale search results must not update active search state.
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
- [src/render/](../src/render/)
- [src/presenter/](../src/presenter/)
- [src/app/](../src/app/) for runtime acceptance and reload effects.
- [src/search/](../src/search/) for search worker generation.

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
- [src/render/](../src/render/)
- [src/presenter/](../src/presenter/)

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
- [benches/](../benches/)
- [src/perf/](../src/perf/)
- [src/metrics.rs](../src/metrics.rs)
