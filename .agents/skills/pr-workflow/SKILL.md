---
name: pr-workflow
description: Create, inspect, review, or update GitHub pull requests for pvf.
---

# Pull Requests

Use `gh` for PR operations. Follow `AGENTS.md` and `docs/testing.md` for
validation appropriate to the diff; reuse passing checks for unchanged content.

Keep the title and body proportional to the change: problem, resulting behavior,
rationale, and validation. Pass multiline bodies with `--body-file`.

For reviews, inspect the diff, checks, and review feedback. `gh pr view --json
reviews` omits inline comments; fetch those with
`gh api "repos/:owner/:repo/pulls/<number>/comments"`.
