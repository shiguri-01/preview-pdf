---
name: rust-crate-path-locator
description: Locate where to read Rust crate source code and docs without assuming ~/.cargo. Use when user asks to inspect a crate's implementation/docs, asks for crate file paths, or when environment-specific cargo cache locations (Scoop, rustup, custom CARGO_HOME) may differ.
---

# Rust Crate Path Locator

Never hardcode `~/.cargo`.

## Workflow

1. Run the cross-platform Python script with `uv`:
   - `uv run scripts/find_crate_path.py --crate <crate-name> --project-path <repo-root>`
2. Prefer `manifest_path` and `root_dir` from `cargo metadata` results.
3. If not found in dependency graph, rerun with cargo-home scan (`--include-cargo-home-scan`).
4. Report both source path and docs URL (`https://docs.rs/<crate>/<version>`).

## Commands

Use from this skill directory or by absolute path.

```bash
# Recommended (cross-platform)
uv run scripts/find_crate_path.py --crate ratatui --project-path /path/to/repo
uv run scripts/find_crate_path.py --crate serde --project-path /path/to/repo --include-cargo-home-scan
```

## Output handling

- If metadata match exists: use that path as authoritative.
- If only cargo-home scan matches exist: treat as heuristic and say so.
- If no match: state crate source is not cached locally yet and suggest `cargo fetch` in a project that depends on it.

### If crate is not installed yet

When evaluating a new crate, local source may not exist yet. In that case, first read docs on `https://docs.rs/<crate>` and metadata on crates.io, then create a temporary Cargo project (or add the crate to an existing one) and run `cargo fetch`; rerun the script after fetch to get an exact local path.

## Notes

- `cargo metadata` is environment-safe because Cargo resolves actual cache paths.
- Scoop/rustup/custom installs are handled because paths come from Cargo output, not assumptions.
