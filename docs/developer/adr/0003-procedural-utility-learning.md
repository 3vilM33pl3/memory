# ADR 0003: Procedural Utility Learning for Automation Loops

Status: Accepted

Date: 2026-07-06

## Context

Memory Layer implements ACT-R's *declarative memory*: base-level activation
in Petrov's O(1) incremental form plus spreading activation with the fan
effect (`crates/mem-reinforce/src/{scoring,propagation}.rs`, ADR 0002). The
LLM agent consuming the store plays the role of ACT-R's *procedural* system.
The one piece of that split the workspace lacked was **production utility
learning**: ACT-R learns each production's utility from reward via the delta
rule `U_n = U_{n-1} + alpha * (R_n - U_{n-1})` (Fu & Anderson 2006, "From
recurrent choice to skill learning: a reinforcement-learning model",
*Journal of Experimental Psychology: General*, doi:10.1037/0096-3445.135.2.184;
Anderson 2007, *How Can the Human Mind Occur in the Physical Universe?*).

Memory Layer's automation loops have static, human-set risk/mode gates and
no learned value, while the reward signal already exists and was discarded:
loop proposals are approved, edited, or rejected; loop-produced memories are
later cited in answers or not.

## Decision

Learn a per-loop utility with the ACT-R delta rule, subject to three
non-negotiable invariants:

1. **Advisory only.** Utility feeds a listing sidecar and recommendation
   strings. It is *never* an input to `LoopMode`, `loop_settings`, or
   `evaluate_action` permission decisions — enforced by construction (those
   APIs take no utility parameter). The single opt-in runtime effect is the
   `utility_floor` (default off), which may only *suppress an auto-fire*;
   manual runs and gates are untouched.
2. **Deterministic.** ACT-R's utility noise term is deliberately omitted; the
   delta rule is pure, clamped, and clock-free. Utility does not decay in v1
   (a deliberate simplification — decisions, not time, move it).
3. **Auditable, outside immutable content.** Utility lives in
   `procedural_utility` keyed by `(project_id, producer_kind, producer_id)`
   (generic so strategies/skills can join later; only `'loop'` is wired),
   mirroring the ADR-0002 pattern, and every update writes a
   `procedural_utility_audit` row.

Rewards are emitted at the proposal-decision funnel
(`resolve_loop_memory_proposal_decision`), in the same transaction as the
status write: approved `+1.0`, edited-then-approved `+0.4` (the `was_edited`
flag persists across the edit's status reset), rejected `-1.0`; only the
first terminal decision rewards. A durable `loop_produced_memory` link lets
the reinforcement access worker pay a `+0.5` citation bonus when a
loop-produced memory is cited in an answer. All magnitudes are
`[procedural]` config knobs.

## Consequences

- The declarative/procedural ACT-R split is now complete in the store's
  terms: declarative activation ranks *memories*; procedural utility ranks
  *producers* — while decision authority stays with humans and the agent.
- Making the reject branch transactional fixed a pre-existing atomicity gap
  (status and trace were previously separate pool writes).
- Reward sparsity is real: loops that rarely propose learn slowly. The
  `min_samples` threshold gates recommendations, and the citation bonus
  densifies the signal for producing loops. Observe-only loops correctly
  have no utility row.
