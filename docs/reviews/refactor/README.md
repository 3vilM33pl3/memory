# Claude Review Refactor Plan Pack

These plans turn `docs/reviews/2026-05-16-extensive-review-claude-opus-4-7.md` into small, reviewable implementation tracks. The goal is to make Memory Layer easier to review, easier to contribute to, and safer to change without bundling unrelated refactors into giant pull requests.

## Current-State Triage

The review was written against an older `main`, so a few findings need current-context handling:

- `mem-mcp` exists again and is wired into the workspace. Do not plan a "restore MCP" PR from the review text.
- `memory remember` now prints JSON by default. Treat the review's `remember --json` concern as a broader CLI output-contract concern.
- The TUI already has `?` help. Treat that finding as stale unless follow-up work improves discoverability or structure.
- The localhost auth bypass, DB test gaps, large-file maintainability risk, scattered SQL, eval shell risk, and contributor-readiness concerns still map to useful work.

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

## Pull Request Guidance

- Each file is one intended PR or short PR sequence.
- Prefer behavior-preserving module splits before feature changes.
- Keep PRs easy to review: move code first, then change behavior in a follow-up.
- For every plan, update or add focused tests before broad cleanup.
- If a plan touches public CLI behavior, update `docs/user/cli/` in the same PR.
