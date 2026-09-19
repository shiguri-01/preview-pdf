# Repository Guidelines

`pvf` is a Rust CLI/TUI PDF viewer.

## Development

Use `nix develop` for the flake-provided environment; direnv can load it via
`.envrc`. Choose validation for the changed surface:

- Rust changes: `cargo check` or a focused test during iteration. Before
  handoff, use `cargo fmt --check`, `cargo test`, and
  `cargo clippy --all-targets --all-features -- -D warnings`.
- Docs and instructions only: check the diff and affected references; Cargo
  validation is unnecessary unless Rust-facing artifacts also change.

For implementation requests, carry the change through relevant validation and
fix failures caused by it. Routine local edits and checks within the requested
scope do not need separate confirmation. Report remaining blockers and anything
that could not be verified.

## Project Knowledge

`docs/README.md` routes developer documentation. Consult architecture guidance
for ownership changes, reference guidance for stable contracts, and testing
guidance for test placement or validation policy. Update affected documentation
when those contracts change; keep local implementation details in code and tests.
Complete command, configuration, and registry inventories belong in code;
developer docs explain their contracts and rationale without duplicating lists.

Repo-local skills provide guidance for specific workflows. Load those relevant
to the task.

## Design and Tests

- Prefer one clear implementation path. Do not add fallbacks, aliases, or
  legacy shims unless the task calls for them.
- When behavior changes, update the affected implementation and tests directly.
- Use in-file `#[cfg(test)]` modules by default. Reserve `<module>/tests` for
  public-facing module contracts and repository `tests/` for process behavior.

## Commits and Pull Requests

Preferred commit format: `<type>(<scope>): <summary>` where useful.
Use `pr-workflow` for GitHub PR creation, inspection, or updates.
