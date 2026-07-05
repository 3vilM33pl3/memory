# Memory Reinforcement & Validation — Design Plan

Status: implemented (phases 1–6 landed on `main`).

See `docs/developer/architecture/memory-reinforcement.md` for the
as-built description, configuration reference, and tuning guide, and
ADR 0002 for the storage decision.

## Problem

Memory Layer stored immutable-versioned project memories but tracked
nothing about how they were *used*, and never re-checked stored knowledge
against the evolving codebase. Frequently used memories could silently rot
as code changed; there was no signal for which memories deserved attention.

## Goals

- Score each memory by real usage (retrieval, citation, direct reads), with
  graph-distance-decayed propagation to linked memories and time decay.
- Track volatility so memories grounded in fast-changing files are
  re-checked more often.
- When a memory becomes hot enough, validate it against project evidence
  (code, docs, provenance, git history) and either re-confirm it, improve
  its wording, queue a correction, or flag it for review — never silently
  changing content on weak or contradictory evidence.
- Keep the trigger available from both the curator workflow and the
  background service; make every knob tunable with safe defaults; audit
  everything; support dry-run.

## Scientific grounding

The design deliberately mirrors established models:

- **ACT-R base-level activation** (Anderson & Schooler 1991): activation is
  a power/exponential-decaying function of the access history. We use
  Petrov's (ICCM 2006) O(1) incremental approximation — a running value
  plus `last_decay_at` — so the query hot path never replays history.
  FSRS (Ye et al., KDD 2022) motivates the separation between a fast
  "retrievability" signal (our activation) and slow "stability" (the
  existing static importance/confidence columns).
- **Spreading activation** (Collins & Loftus 1975; Anderson 1983): access
  boosts propagate over the memory-relation graph with per-hop decay and
  *fan normalization* — dividing by the linking node's degree, the ACT-R
  fan effect — plus a minimum-increment cutoff (Crestani 1997). HippoRAG
  (Gutiérrez et al., NeurIPS 2024) is a modern precedent for propagating
  relevance over an agent-memory graph.
- **Threshold-triggered reflection** (Park et al., Generative Agents, UIST
  2023): an accumulating importance signal crossing a threshold triggers an
  LLM pipeline. Our detect-then-rewrite split follows just-in-time comment
  maintenance research (Panthaplackel et al. AAAI 2021; CUP2, TSE 2022):
  a cheap detector picks candidates, a generator proposes the update.
  A-MEM (NeurIPS 2025) and Mem0 (2025) are precedents for LLM-driven
  evolution of linked memories.
- **Truth maintenance / belief revision** (Doyle 1979 JTMS; de Kleer 1986
  ATMS; AGM 1985): a memory's validity status is a function of recorded
  justifications — our stance-annotated evidence rows — and weakened
  support triggers re-labeling (`needs_review`) rather than silent change.
  Activation acts as a computable entrenchment ordering: contradictions
  should retire the least-entrenched belief first.
- **Volatility-aware revalidation** (Cho & Garcia-Molina 2003; update-risk
  TTL): per-memory re-check intervals derive from the observed change rate
  of the underlying artefacts, allocating validation budget where facts
  actually expire.

## Possible academic directions

1. **Access-driven revalidation for agent memory: a cost/quality frontier.**
   Compare decay policies (none / uniform exponential / ACT-R power-law /
   volatility-aware) on a longitudinal coding-agent corpus, measuring
   stale-memory retrieval rate and downstream task accuracy against LLM
   validation cost. Published agent-memory systems consolidate on *write*;
   decay-triggered *revalidation* with an explicit budget is unexplored.
2. **Entrenchment-ordered belief revision in LLM agent memory.**
   Operationalize AGM entrenchment as the activation score and evaluate
   contradiction handling against LLM-judge ground truth, bridging classic
   TMS/belief-revision theory and agentic memory.
3. **Commit-triggered memory staleness prediction.** Port just-in-time
   comment-inconsistency detection to agent memories with file provenance:
   predict at commit time which stored memories a diff invalidates, using
   replayed repository histories as a natural benchmark.

## Key design decisions

- **New crate `mem-reinforce`** owning pure scoring math, persistence,
  propagation, selection, and the validation pipeline behind a
  `VerdictProvider` trait. Curation stays deterministic; search stays
  read-only (it only joins a table).
- **Mutable state outside the immutable version chain** — see ADR 0002.
- **Handler-level access hooks + bounded channel + async worker**: zero
  added latency on the query path; overflow drops batches (scoring is
  advisory). Only the three retrieval hooks enqueue — validation, curation,
  and provenance reads never count, preventing feedback loops.
- **Scan-based selection, no queue table**: idempotent and restart-safe.
  Hysteresis via post-run cooldowns plus needs_review exclusion.
- **In-service LLM verdicts for v1**; the trait is the seam for a future
  agent-CLI/worktree runner built on the mem-loops sandbox.
- **Apply policy**: auto-apply only high-confidence rewording of
  still-valid memories; corrections always human-gated; weak evidence
  always flags. Anti-hallucination check rejects verdicts citing evidence
  that was not in the gathered context.

## Phases (all landed)

1. Migration `0023` + `mem-reinforce` pure math + unit tests.
2. Access recording: config, runtime, hooks, worker, atomic upserts,
   threshold-crossing audit.
3. Search ranking integration (activation boost, needs_review penalty).
4. Selection + curator hook + background scheduler (retention, compaction,
   volatility, daily budget).
5. Validation pipeline: shared LLM helper, evidence gathering, verdict
   validation, apply policy, dry-run, review resolution.
6. HTTP/CLI surface, timeline events, documentation.
