# Memory Provenance Ranking Integration

## Review Basis

The 2026-05-18 audit (`AUDIT-2026-05-18.md`) found that plan `02-memory-provenance-verifier.md` landed only partially. The verifier exists end-to-end:

- CLI: `memory verify-provenance` at `crates/mem-cli/src/main.rs:498`, `:2733`
- Service: `/v1/provenance/verify` at `crates/mem-service/src/main.rs:3015`
- Schema: `migrations/0017_memory_source_provenance.sql`
- Diagnostic surfacing: `stale_memory_provenance` warnings in `crates/mem-search/src/lib.rs:871`
- Web types: `SourceProvenanceRecord` / `SourceProvenanceStatus` at `web/src/types.ts:155`

But the **ranker ignores these signals**. Stale memories still come back at full confidence — the exact failure mode the original 2026-05-16 review §7 Scenario E identified ("mem-mcp" cited at 0.74 confidence after the crate was removed).

This plan closes that loop.

## Goal

Make the existing provenance signal actually affect retrieval, so stale memories are de-ranked (and visibly flagged), and re-verification happens automatically without a human running the CLI.

## PR Shape

Three small PRs.

1. **Ranker integration.** Read `memory_source_verifications.status` during retrieval in `crates/mem-search/src/lib.rs`. Apply a configurable confidence decay per status:
   - `MissingFile` → multiply confidence by `staleness.missing_file_decay` (default `0.5`)
   - `MissingSymbol` → multiply by `staleness.missing_symbol_decay` (default `0.7`)
   - `Stale` → multiply by `staleness.stale_decay` (default `0.85`)
   - `Verified` → no change
   - Unknown / never-verified → no change (but tag a `provenance_unverified` warning so curation can see it)
2. **Query opt-in flag.** Add `memory query --include-stale` (and matching web/service plumbing) for the cases where a user explicitly wants to see everything regardless of decay. Default: include but de-rank.
3. **Background re-verifier.** Add a scheduled task that re-runs verification per project on a configurable interval (default daily). Wire it through the existing service runtime — same shape as the watcher manager. The v1 plan explicitly punted on scheduling; this PR lands it. Also add symbol-level checks via mem-graph as the v1 plan anticipated (currently file-path only).

## Implementation Notes

- Decay values live in `crates/mem-api/src/types.rs` under `RankingConfig` (or a new `ProvenanceConfig`). Expose via the same config mechanism that already feeds other ranker knobs.
- Surface the decay in query diagnostics so a user can see *why* a memory dropped (e.g. `provenance decay x0.5 (MissingFile)`). The existing diagnostic format at `crates/mem-search/src/lib.rs:871` already carries `stale_memory_provenance`; extend it with the multiplier and source.
- Symbol-level checks: reuse mem-graph extraction. A cited symbol is "missing" when the file resolves but the symbol no longer appears in `code_symbols`.
- Background re-verifier must respect dev/prod profile isolation; do not re-verify across the dev/prod database split.
- All thresholds and the scheduling interval must be configurable per-project.

## Tests

- `cargo test -p mem-search --all-targets --locked` — add a test that seeds a project with one fresh and one `MissingFile` memory, queries, and asserts ordering.
- `cargo test -p mem-service --all-targets --locked` — add a test for `--include-stale` plumbing.
- Add an eval-suite item under `evals/suites/memory-improvement-v1` that intentionally stales a memory mid-suite and asserts the system either de-ranks it or warns appropriately. This addresses the audit's "self-demonstrating failure" finding by gating it in CI.
- Manual: `memory query --project memory --question "<question with known stale source>"` and confirm the diagnostic shows the decay.

## Acceptance Criteria

- A memory with `status = MissingFile` ranks **lower** than an otherwise-equivalent verified memory.
- `memory query --include-stale` returns the same set as today; without the flag, stale items are de-ranked (not hidden).
- Diagnostics include the decay multiplier and source status.
- A scheduled re-verifier runs without operator intervention; `memory status` reports its last-run time.
- Symbol-level checks light up `MissingSymbol` when a cited symbol disappears even if the file still exists.
- The eval suite gates the regression.
