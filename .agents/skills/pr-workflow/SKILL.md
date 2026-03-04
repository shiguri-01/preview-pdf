---
name: pr-workflow
description: Create and review GitHub pull requests with a consistent workflow. Use this whenever the user asks to create a PR, open/view PR details, draft PR text, or prepare changes for merge.
---

## Preconditions

1. Before creating a PR, run quality checks in this order:
   - `cargo fmt --check`
   - `cargo test`
   - `cargo clippy --all-targets --all-features -D warnings`
2. If any check fails, do not create the PR until fixed.

## Create PR

1. Create a short branch-appropriate PR title.
2. Write a concise PR body tailored to the change's scope:
   - Small/obvious changes: verification results + one-line rationale
   - Larger changes: problem, solution approach, risk/impact, verification.
   - Omit sections that add no information for this specific change.
   - Linked issue (if already known; do not search for it)
3. Create PR with `gh pr create` using title/body prepared above.
   - Note: In `--body`, literal `\n` may not become newlines; pass actual line breaks (or use `--body-file`).

## View PR

Use `gh pr view <number>` to fetch PR details.
- `--json fields` and `--jq expression` are available for structured/filtered output.
