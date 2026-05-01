---
name: rust-crate-path-locator
description: Locate Rust crate source and documentation paths by asking Cargo instead of assuming ~/.cargo. Use when the user asks to inspect a crate implementation, find local crate files, or get the matching docs.rs URL for a dependency.
---

# Rust Crate Path Locator

Do not assume crates live under `~/.cargo`. Use Cargo's resolved metadata first.

## Workflow

1. From the Rust project root, run `cargo metadata --format-version 1`.
2. Find the package whose `name` matches the crate.
3. Use that package's `manifest_path`; its directory is the crate source root.
4. Report the source root and docs URL: `https://docs.rs/<crate>/<version>`.

## Commands

```bash
cargo metadata --format-version 1
```

If `jq` is available, this is a convenient filter:

```bash
cargo metadata --format-version 1 |
  jq -r '.packages[] | select(.name == "ratatui") | "\(.manifest_path)\nhttps://docs.rs/\(.name)/\(.version)"'
```

If the crate is not in the current dependency graph, say so. For a crate that should be local but has not been fetched yet, run `cargo fetch` in a project that depends on it, then retry `cargo metadata`.
