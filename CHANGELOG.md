# Changelog

## 2.0.0 - Unreleased

Pre-ATProto refactor: six waves of simplification, boundary work, and record
model changes preparing the codebase for AT Protocol federation. This is a
breaking release for config, HTTP API, CLI flags, bundles, and the stream
protocol. Databases migrate in place (migrations 0027–0032); bundle v1
imports remain supported.

### Security

- `DELETE /v1/memory` now resolves the owning project from the request body
  and requires project-scoped Admin; the route-policy fallback no longer
  grants access based on membership in any project.
- `GET /v1/runtime/status` is project-scoped instead of unscoped.
- Route authorization is a fail-closed registration-time policy table:
  unknown path/method combinations are denied instead of defaulting to
  read access. Denied and errored authorizations are audited with request ids.

### Added

- Permission sets replace the ordinal role ladder: principals carry an
  explicit permission bitset; role names survive as presets and
  `/v1/auth/me` exposes expanded permissions (migration 0027).
- Single-user installs get a persistent local-owner principal and
  `POST /v1/auth/session/bootstrap` (loopback-only) replaces the
  `GET /v1/web/auth-token` handoff.
- Per-canonical memory state table; version rows are immutable and query
  results/citations carry `canonical_id`/`version_no` (migrations 0028–0032).
- The project timeline is an ordered, durable event log with a monotonic
  `seq`, transactional appends, and 12 new activity kinds covering the loop,
  provenance, consolidation, workspace, and auth planes.
- The stream (`/ws`) pushes deltas (`memory_upserted`/`memory_removed`/
  `overview_changed`) with `Resync` instead of refetching snapshots.
- Append-only provenance (`memory_source_checks`) with git commit, line
  range, and content-hash anchors; content drift is detected by re-hashing.
- Deterministic content-addressed bundles (schema v2): same content, same
  bytes, same `bundle_id`; v1 imports still accepted.
- Graph extraction runs through `POST /v1/projects/{slug}/graph/extract`;
  the CLI no longer links sqlx.
- `/v1/runtime/status` reports server-side database, LLM, and embeddings
  facts; `memory doctor` relays them (`backend.llm_report`,
  `backend.embeddings_report`).

### Changed (breaking)

- The Cap'n Proto socket transport is gone; the TUI and clients use the
  `/ws` websocket. Config keys `capnp_unix_socket`/`capnp_tcp_addr` removed.
- `mem-api` is deleted; wire types live in `mem-record`, configuration in
  `mem-config`.
- Feature gates: `memory` builds with `tui` by default; `memory service run`
  needs `--features embedded-service` (packaged builds use `full`);
  mem-service's bundled DuckDB offline store is behind `offline` (default on).
- Writer identity is derived from the authenticated principal; client
  `writer_id` is advisory metadata.
- Four loop-settings routes collapse into
  `POST /v1/loops/{loop_id}/settings`; four activity routes into
  `POST /v1/activity`.
- `memory automation` folds into `memory watcher flush`;
  `memory capture task` becomes `memory capture`; `memory dev init` becomes
  `memory dev`; `memory setup` (duplicate of `wizard`) is removed.
- `memory_entries.status`/`archived_at` are dropped in favor of the
  canonical state table; relation edges live on canonical endpoints with
  `asserted`/`derived` origin.
- `/v1/stats`, `/v1/offline/pending`, and the web auth-token route are
  removed; watcher restart and shutdown are in-process calls.
- SDK clients (`clients/`) and the Skyrim integration left the main branch
  (history preserves them).

### Deferred

- Shared clap `OutputArgs`/`ScopeArgs` flatten and a global `--json`
  polarity flip (eval keeps JSON-default output): high churn, no behavior
  change, revisit if the CLI surface grows.
- TUI agents tab still collects locally; `mem-agenttop` is dependency-light
  and the watcher manager needs the local collector regardless.

## 1.0.1 - 2026-07-17

### Added

- Multi-platform release packaging: release tags now build Debian `amd64` and
  `arm64` packages, macOS Intel and Apple Silicon `.pkg` installers, Windows
  x86_64 MSI/ZIP artifacts, checksums, and a source archive for Homebrew.
- Procedural utility learning (ACT-R production utility, ADR-0003): each
  automation loop learns a per-project utility from proposal decisions via
  the delta rule (approve +1.0, edited-approve +0.4, reject −1.0, cited
  memory +0.5), updated atomically with the decision and fully audited
  (`procedural_utility` / `procedural_utility_audit`, migration 0024).
  Advisory only — `memory loops --project` shows utility and
  recommendations; modes and permission gates are never affected. Optional
  `utility_floor` (default off) can suppress auto-triggers for
  collapsed-utility loops. New `[procedural]` config section. Also makes
  proposal rejection transactional (previously status and trace were
  separate writes).

- Memory-quality canary suite (`evals/suites/memory-quality-v1`) with a new
  `adversarial_stale` eval item type (refuse-or-prefer-fresh contracts) and a
  release gate (`evals/gates/memory-quality-v1.toml`) enforcing absolute
  success-rate floors, including zero tolerated adversarial-stale failures.
  Gate policies gained `min_candidate_success_rate` and per-group floors.
- Semantic dedup pass: after automatic embedding creation, curation links
  paraphrased near-duplicates via chunk-embedding cosine similarity and
  queues human-gated merge proposals (`loop_id` `semantic_dedup` in
  `memory proposals`); high-similarity pairs with low lexical overlap and
  supersede/negation cues are flagged as likely contradictions instead.
  New `[curation]` config section: `semantic_dedup_enabled`,
  `semantic_duplicate_threshold`.
- Property tests for the search ranker (penalties never raise scores, total
  result ordering, finite scores).

### Fixed

- Plan checkpoint flows no longer stall on `/v1/curate` timeouts (3VI-824):
  start-execution, finish-execution plan sync, and the implementation memory
  each curate only the capture they just created instead of the whole project
  backlog, and start-execution degrades a curation error to a warning since
  the checkpoint and capture are already durable. `finish-execution` also
  resolves the plan thread recorded at start-execution, so `--thread-key` is
  only needed to override when several plans are active.
- Deterministic answer synthesis now refuses on weak evidence and no longer
  echoes superseded facts (3VI-773): a weak-match refusal predicate (low term
  overlap and low semantic similarity with no exact-phrase anchor), a memory
  confidence floor for stating or citing evidence, and a same-topic runner-up
  filter that drops duplicate/contradicting "Also relevant" context. The
  memory-quality adversarial group went 0/7 → 7/7 and the release gate passes
  (overall 18/26 → 25/26, deterministic across runs).

## 1.0.0 - 2026-07-05

First stable release, cut locally on monolith from the v1.0 stabilization
line plus the memory reinforcement & validation system.

### Added

- Memory reinforcement and validation system (`mem-reinforce`): access-driven
  activation scoring with spreading activation over memory relations, time
  decay, and volatility tracking; activation-aware search ranking with
  needs-review penalties; a threshold-triggered, evidence-backed LLM
  validation pipeline (opt-in, dry-run first) with human-gated corrections
  and full audit trails; `memory scores`, `memory validate`, and
  `memory review` CLI commands plus matching HTTP endpoints. See
  `docs/developer/architecture/memory-reinforcement.md`.

### Stabilization focus

- Lock the documented v1 compatibility contract for CLI, config, migrations,
  MCP read tools, and local-first service operation.
- Validate fresh installs and upgrades for Debian packages, Homebrew installs,
  and source/dev runs.
- Run the release validation gate: formatting, workspace tests, clippy, web
  tests/builds, pgvector-backed database tests, and eval gate reports.
- Keep loop automation, graph visualization, and eval research workflows
  documented as advanced surfaces where behavior is still intentionally
  conservative.

### Known issues carried into 1.0.0

- Fix the local `/v1/curate` timeout that can prevent plan-memory closure.
- Close or intentionally document stale active plan memories.
- Verify `memory doctor --fix` repairs missing or outdated Memory-owned skills
  from GitHub and falls back to the installed template when offline.
- Publish the RC from a clean pushed `main`, then promote to final only after
  packaged install, upgrade, TUI, web UI, watcher, MCP, and eval gates pass.
