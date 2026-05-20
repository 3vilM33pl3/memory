# Claude Review Refactor Plan Pack

These plans turn `docs/reviews/2026-05-16-extensive-review-claude-opus-4-7.md` into small, reviewable implementation tracks. The goal is to make Memory Layer easier to review, easier to contribute to, and safer to change without bundling unrelated refactors into giant pull requests.

## Current-State Triage

The review was written against an older `main`, so a few findings need current-context handling:

- `mem-mcp` exists again and is wired into the workspace. Do not plan a "restore MCP" PR from the review text.
- `memory remember` now prints JSON by default. Treat the review's `remember --json` concern as a broader CLI output-contract concern.
- The TUI already has `?` help. Treat that finding as stale unless follow-up work improves discoverability or structure.
- The localhost auth bypass, DB test gaps, large-file maintainability risk, scattered SQL, eval shell risk, and contributor-readiness concerns still map to useful work.

## Status (2026-05-18)

All 12 plans below landed as commits in the v0.9.0 release window. An audit against current `main` is in [AUDIT-2026-05-18.md](AUDIT-2026-05-18.md): five plans shipped well, two are partial, and five landed cosmetically (the structural god-file splits — `mem-cli/main.rs`, `mem-service/main.rs`, `tui.rs`, `App.tsx` — saw scaffolding but minimal code movement, and three of them actually grew). The follow-up pack at the bottom of this file (`13`–`19`) picks up the missing pieces with explicit LOC budgets.

## Recommended Order

1. [Service Auth Hardening](01-service-auth-hardening.md)
2. [Memory Provenance Verifier](02-memory-provenance-verifier.md)
3. [DB Integration Test Harness](03-db-integration-test-harness.md)
4. [CLI Command Split](04-cli-command-split.md)
5. [Service Route Split](05-service-route-split.md)
6. [API Module Split](06-api-module-split.md)
7. [Repository Layer](07-repository-layer.md)
8. [TUI Architecture Split](08-tui-architecture-split.md)
9. [Web App Decomposition](09-web-app-decomposition.md)
10. [CLI UX And Agent Contract](10-cli-ux-and-agent-contract.md)
11. [Eval Safety And Research Extensibility](11-eval-safety-and-research-extensibility.md)
12. [Contributor PR Readiness](12-contributor-pr-readiness.md)

## Follow-Up Plan Pack

Derived from [AUDIT-2026-05-18.md](AUDIT-2026-05-18.md). Each finishes work the original pack started cosmetically or deferred. Grouped by impact, not numeric order.

### P1 — High impact, contributor unblock

- [13. CLI Command Split — Finish](13-cli-command-split-finish.md) — drive `crates/mem-cli/src/main.rs` from 16,644 LOC to ≤ 3,000.
- [14. Service Route Split — Finish](14-service-route-split-finish.md) — drive `crates/mem-service/src/main.rs` from 9,007 LOC to ≤ 1,500; extract real `handlers/` modules.
- [18. Memory Provenance Ranking Integration](18-provenance-ranking-integration.md) — wire the existing verifier signal into the ranker so stale memories are actually de-ranked. Closes the original review's §7 Scenario E.

### P2 — Cleanup that unlocks future work

- [15. Repository Layer — Extend Across Crates](15-repository-layer-extend.md) — finish the SQL extraction in mem-curate, mem-graph, mem-search; cover writes.
- [17. Web App Decomposition — Finish](17-web-app-decomposition-finish.md) — extract the 10 remaining tabs from `App.tsx`; target ≤ 600 LOC.
  - [17.5. Web Controller Hook Split](17.5-web-controller-hook-split.md) — slice the 1,067-LOC `useWebAppController` into per-feature hooks + a thin `useAppShell`.

### P3 — Quality of life

- [16. TUI Architecture Split — Finish](16-tui-architecture-split-finish.md) — per-tab modules with their own state sub-structs; target `tui.rs` ≤ 3,000 LOC.
  - [16.5. TUI `App` Slim and MVU Seed](16.5-tui-app-slim-and-mvu-seed.md) — group 24 coordinator fields into sub-structs; introduce per-tab `update(...) -> TabAction` seam.
  - [16.6. TUI Query Tab Input Migration](16.6-tui-query-tab-input-migration.md) — move query-tab normal-mode navigation out of `app.rs` into `tabs/query.rs::update`.
- [19. Eval — External Retriever Command](19-eval-retriever-cmd.md) — land the deferred `--retriever-cmd` interface for the researcher persona.

## Pull Request Guidance

- Each file is one intended PR or short PR sequence.
- Prefer behavior-preserving module splits before feature changes.
- Keep PRs easy to review: move code first, then change behavior in a follow-up.
- For every plan, update or add focused tests before broad cleanup.
- If a plan touches public CLI behavior, update `docs/user/cli/` in the same PR.
- Follow-up plans (`13`–`19`) inherit the same "move first, change behavior later" rule. They also carry explicit LOC budgets in their acceptance criteria — those budgets exist because the original pack's structural plans lacked numeric targets and landed cosmetically (see [AUDIT-2026-05-18.md](AUDIT-2026-05-18.md) §4).
