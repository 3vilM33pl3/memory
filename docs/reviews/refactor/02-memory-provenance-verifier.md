# Memory Provenance Verifier

## Review Basis

Claude observed a self-stale memory failure: Memory Layer answered confidently from a memory whose cited code no longer existed. Current `main` has MCP restored, but the broader risk remains: memories can outlive their file, symbol, or source evidence.

## Goal

Add a read-only provenance verification pass that detects stale evidence and exposes that status to query, resume, TUI, and curation workflows.

## PR Shape

Split into two PRs if needed: first add verification data and API exposure, then wire query/ranking behavior. Avoid changing curation replacement logic in the first PR.

## Implementation Notes

- Define a provenance health model with statuses such as `verified`, `missing_file`, `missing_symbol`, `unverifiable`, and `stale`.
- Start with file-path evidence because it is deterministic and cheap; symbol-level checks can follow using `mem-graph`.
- Store the latest verification result per memory/source without rewriting canonical memory text.
- Surface provenance health in memory detail responses, query citations, and TUI memory detail lines.
- In query synthesis, clearly warn when a cited memory has stale or missing provenance. Do not silently hide stale memories in v1.
- Add a CLI entrypoint such as `memory verify-provenance --project <slug> --dry-run --json`, then later allow scheduled/background execution.

## Tests

- Unit-test path extraction and missing-file classification.
- Integration-test a memory with an existing file and then a deleted file using a temp repo fixture.
- Test query response diagnostics include stale provenance warnings.
- Run `cargo test -p mem-api -p mem-search -p mem-service -p mem-cli --all-targets --locked`.

## Acceptance Criteria

- Users can run a dry-run provenance check and see which memories need attention.
- Query results do not present stale evidence as fully verified.
- No canonical memory is deleted or rewritten by the v1 verifier.
