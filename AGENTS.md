# Repository Guidelines

`pvf` is a Rust CLI/TUI PDF viewer.

## Docs
- `docs/README.md`: entry point for durable developer docs and where material belongs.
- `docs/architecture.md`: runtime flow, subsystem boundaries, ownership, and event routing.
- `docs/reference.md`: stable developer-facing contracts and owning code entry points.
- `docs/testing.md`: test placement, test-first guidance, quality checks, and validation policy.
- Read only the relevant docs sections for the change. Keep docs in sync with code and test changes.

## Skills
- Use `documentation` for docs placement, migration, stale-doc cleanup, and docs quality review.
- Use `testing` for regression tests, test layer choice, module contract tests, and validation workflow.
- Use `pvf-command` for command ids, parsing, invocation policy, key bindings, help, and dispatch.
- Use `pvf-palette` for palette kinds, providers, payloads, rows, input modes, selection, tab, submit, and rendering contracts.
- Use `pvf-extension` for extension state, hooks, event handling, background drain, status bar data, and UI snapshots.
- Use `bench` for pvf headless performance diagnostics.
- For GitHub PR creation/view/update tasks, use `pr-workflow`.

## Commands
- `nix develop`: enter the flake-provided development shell; direnv can load it via `.envrc`.
- `cargo check`: fast compile validation during iteration.
- `cargo build`: full debug build.
- `cargo run`: run locally.
- `cargo test`: run tests.
- `cargo fmt`: format.
- `cargo clippy --all-targets --all-features -- -D warnings`: lint and fail on warnings.

## Testing
- Default: in-file `#[cfg(test)]` modules
- `<module>/tests`: only for testing public-facing specs/interfaces of that module

## Commit & Pull Request Guidelines
- Preferred commit format: `<type>(<scope>): <summary>` where useful (`feat`, `fix`, `refactor`, `docs`, `test`).
