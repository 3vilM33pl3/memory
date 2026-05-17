# Contributor PR Readiness

## Review Basis

Claude's contributor persona bounced off giant files, missing first-contribution guidance, and unclear DB test behavior. The overarching project goal is to be pull request friendly and easy to understand.

## Goal

Add contributor-facing documentation and conventions that make small PRs the default path.

## PR Shape

This is a docs-first PR that can land before the code refactors. Keep it practical and current.

## Implementation Notes

- Add or update `CONTRIBUTING.md` with setup, validation, DB test requirements, and PR expectations.
- Add an architecture map that points to the main crates and explains where to make common changes.
- Document "small PR" examples: CLI command, service route, TUI tab, web tab, curation behavior, eval suite.
- Explain local vs CI validation, including when pgvector-backed tests run.
- Link these refactor plans from contributor docs as the roadmap for PR-friendly structure.
- Add issue labels or suggested task names only if the repo already uses them consistently.

## Tests

- Run a markdown link check or at least verify all linked local paths exist.
- Confirm documented commands match current CLI/workflow names.

## Acceptance Criteria

- A new contributor can find the right crate/file area before opening code.
- PR size expectations are explicit.
- DB and eval validation expectations are not hidden in CI behavior.
