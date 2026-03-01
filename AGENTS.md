# Repository Guidelines

`pvf` is a Rust CLI/TUI PDF viewer.

## Docs
- `docs/`: specifications and plans. Read relevant docs before implementing features. Keep in sync with code changes.

## Commands
- `cargo check`: fast compile validation during iteration.
- `cargo build`: full debug build.
- `cargo run`: run locally.
- `cargo test`: run tests.
- `cargo fmt`: format.
- `cargo clippy --all-targets --all-features -D warnings`: lint and fail on warnings.

## Testing
- Default: in-file `#[cfg(test)]` modules
- `<module>/tests`: only for testing public-facing specs/interfaces of that module

## Commit & Pull Request Guidelines
- Preferred commit format: `<type>(<scope>): <summary>` where useful (`feat`, `fix`, `refactor`, `docs`, `test`).
- PRs should include:
  - clear problem/solution summary
  - linked issue (if applicable)
