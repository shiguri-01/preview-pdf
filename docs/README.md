# Developer Docs

This directory contains the canonical developer-facing documentation.

## Reading order

1. `runtime-spec.md`
2. `command-system.md`
3. `architecture.md`
4. `rendering-pipeline.md`
5. `performance-diagnostics.md`
6. `palette-provider.md`
7. `extension-system.md`

## Writing rules

- Describe current behavior only.
- Keep one normative home for each topic.
- Put stable contracts before implementation references.
- Use `Code references` only as navigation aids.
- Link to the owning document instead of repeating rules.

## Document map

| Document | Owns |
|---|---|
| `runtime-spec.md` | CLI contract, key bindings, config lookup, and user-visible runtime behavior |
| `command-system.md` | Command ids, parsing, invocation policy, and dispatch |
| `architecture.md` | Runtime/module map, subsystem ownership, and event-loop structure |
| `rendering-pipeline.md` | L1/L2 cache semantics, worker lanes, encode flow, and redraw timing |
| `performance-diagnostics.md` | Developer scenario benchmark command, scenarios, and JSON report shape |
| `palette-provider.md` | `PaletteProvider`, `PaletteKind`, palette keyboard behavior, and built-in palettes |
| `extension-system.md` | `Extension`, `ExtensionHost`, extension flows, and built-in extensions |
