# Changelog

## Unreleased

### Added

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

### Known issues

- The memory-quality canary's adversarial group currently fails: deterministic
  answer synthesis echoes superseded facts as "Also relevant" context and does
  not refuse when only irrelevant or low-confidence memories match. Tracked as
  the next quality workstream; the gate stays red on that group until fixed.

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
