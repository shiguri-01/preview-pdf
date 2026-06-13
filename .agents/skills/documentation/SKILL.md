---
name: documentation
description: Add, update, trim, or review pvf developer documentation. Use when changing docs under docs/, deciding whether material belongs in docs versus code or tests, migrating docs, fixing stale documentation, or reviewing documentation quality.
---

# Documentation

Use this skill for developer-doc changes in pvf.

## Start

Read `docs/README.md` first to understand where material belongs.
Read `docs/architecture.md` when documenting runtime flow, subsystem
boundaries, ownership, or event routing.
Read `docs/reference.md` when documenting stable developer-facing contracts.
Read `docs/testing.md` when documentation changes affect how behavior should be
protected.

## Placement

Put durable orientation and policy in docs. Keep implementation detail near the
code unless it is a stable contract or boundary rationale.

- Use `docs/architecture.md` for system shape, runtime flow, ownership, and
  boundary rationale.
- Use `docs/reference.md` for compatibility-sensitive contracts and the code
  entry points that own complete details.
- Use `docs/testing.md` for test placement, test-first guidance, and validation
  policy.
- Use local code comments for details that only explain nearby implementation.
- Use tests when the important point is executable behavior or consistency.

Do not make docs the source of truth for complete inventories such as command
lists, keymaps, config fields, provider lists, or cache algorithms unless they
are generated or explicitly protected against drift.

## Editing

Prefer replacing, trimming, or moving stale material over adding another
section. Before adding text, decide what existing sentence, section, test, or
code comment should own the same idea.

When editing docs:

- State stable rules neutrally.
- Keep examples representative rather than exhaustive.
- Link to owning docs or code when details are code-owned.
- Avoid copying implementation tables into prose.
- Keep architecture docs about boundaries, not module inventories.
- Keep reference docs about contracts, compatibility, ownership, and tests.
- Keep testing docs about where behavior should be protected.

## Checks

Before finishing, check that the changed doc is stable enough to maintain, has
the right owner, points to code for code-owned detail, and will not stale after
an ordinary implementation change.

For docs-only or skill-only changes, run `git diff --check` and search for stale
paths or deleted doc names. Run Cargo validation only when behavior, code,
tests, or generated Rust-facing artifacts changed.
