---
name: rust-crate-path-locator
description: Locate a dependency's resolved Rust source or version-matched docs.
---

# Rust Crate Path Locator

Run `cargo metadata --format-version 1` from the project root. Select the
dependency's resolved package and version; the directory of `manifest_path`
is its source root. Do not assume `~/.cargo`.

For registry releases, use `https://docs.rs/<name>/<version>` for matching docs.
