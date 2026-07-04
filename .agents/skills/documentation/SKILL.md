---
name: documentation
description: Edit or review pvf developer docs under docs/. Use when changing docs/ files, trimming or migrating stale docs, deciding whether information belongs in docs, code comments, or tests, or checking architecture/reference/testing doc ownership and drift risk.
---

# Documentation

Use this skill for developer-doc changes in pvf.

pvf docs carry durable project knowledge that does not belong in a single
implementation file: architecture boundaries, stable contracts, and testing
policy. Code owns implementation and complete inventories; tests own detailed
executable specifications. Edit docs to preserve those relationships.

## Before editing

Read the target section for local wording changes, and read `docs/README.md`
when ownership across docs, code comments, or tests may change.

Choose the owner before adding text. Docs own durable design knowledge:
architecture boundaries, stable contracts, and policy. Code comments own local
implementation judgment, especially why nearby code takes one approach over
another. Tests own executable specification: behavior, compatibility, edge
cases, and regressions. Keep complete inventories out of docs unless they are
generated or explicitly protected against drift.

## Editing

Prefer replacing, trimming, or moving stale material over adding another
section. Before adding text, decide what existing sentence, section, test, or
code comment should own the same idea.

When editing docs:

- State stable rules neutrally.
- Do not write agent reasoning, task instructions, harness behavior, or
  navigation that filenames already provide into project docs.
- Do not let obsolete implementation context shape durable docs. Unless history
  is the subject, describe the current rule, boundary, or contract directly
  rather than explaining it as a contrast with an old design.
- Keep document-internal conventions in the docs when readers need them to use
  or maintain that document. Keep agent-only workflow notes out of project
  docs.
- Keep examples representative rather than exhaustive.
- Link to owning docs or code when details are code-owned.
- Avoid copying implementation tables into prose.

For reference entries, use structure to force the necessary review facts, but
do not mistake structure for quality. The entry should tell a maintainer what
must remain true, what compatibility care is needed, and where the owning code
lives without duplicating implementation detail.

## Checks

Before finishing, check that the changed doc is stable enough to maintain, has
the right owner, and will not stale after an ordinary implementation change.
Read the changed section as a future maintainer, not as the agent who just
worked through the reasoning.

For docs-only or skill-only changes, run `git diff --check` and search for stale
paths or deleted doc names. Run Cargo validation only when behavior, code,
tests, or generated Rust-facing artifacts changed.
