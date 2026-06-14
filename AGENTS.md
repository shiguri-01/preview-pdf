# Repository Guidelines

`pvf` is a Rust CLI/TUI PDF viewer.

## Docs
- `docs/README.md`: developer docs entry point. Read only relevant sections.
- Keep docs in sync with code and test changes.
- Repo-local skills provide task-specific workflow guidance; use them when they apply.

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

## Simplicity
- Prefer one clear implementation path over compatibility branches.
- Do not add fallbacks, aliases, or legacy shims unless the task specifically calls for them.
- When behavior changes, update the affected code and tests directly instead of keeping both old and new paths.

## Commit & Pull Request Guidelines
- Preferred commit format: `<type>(<scope>): <summary>` where useful (`feat`, `fix`, `refactor`, `docs`, `test`).
- For GitHub PR creation/view/update tasks, use `pr-workflow` skill.
