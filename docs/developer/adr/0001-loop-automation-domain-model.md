# ADR 0001: Loop Automation Domain Model

Status: Accepted

Date: 2026-06-15

## Context

Memory Layer is becoming the control plane for background agent loops. The
existing Memory Core stores and retrieves durable project memory. Loop
Automation adds a separate operational layer that decides which loops may run,
what context they receive, what capabilities they have, how runs are evaluated,
and what outcomes may become durable memory.

The source product plan is the Notion page "Memory Layer - Loop Engineering
Automation Plan" and Linear epic `3VI-595`.

## Decision

Loop Automation is a first-class domain owned by the Memory Layer service. It is
not a second memory persistence path. Loops use Memory Core through typed service
interfaces and may only write durable memory through proposal and approval
workflows.

### Terms

- Loop definition: an immutable versioned automation contract containing trigger,
  context, policy, evaluation, output, and writeback specs.
- Loop setting: a per-scope override that controls enabled state, mode, budgets,
  approvals, pause, and snooze state.
- Trigger event: a normalized event from manual invocation, schedule, webhook,
  CI, repository changes, memory changes, or another trusted source.
- Loop run: one attempt to evaluate or execute a loop definition with effective
  settings and policy decisions.
- Approval request: a human decision record for a risky action or durable memory
  mutation.
- Memory proposal: a loop-authored add, update, deprecate, merge, or link
  proposal for Memory Core.
- Learned skill: a reusable development recipe mined from successful runs and
  accepted by a human.
- Run trace: human-readable, redacted evidence for what a run saw, decided, and
  produced.

### Modes

Loop modes are:

- `off`: no trigger handling.
- `observe`: record candidate events and diagnostics only.
- `suggest_only`: produce reports, comments, or task suggestions.
- `draft_output`: create drafts such as proposed memory changes, issue comments,
  or draft PRs when policy allows.
- `autonomous_safe`: execute only low-risk actions within explicit capability and
  budget limits.
- `paused`: temporarily disabled by setting state.
- `snoozed`: temporarily suppressed until a configured time.

Pause and snooze are represented as setting state so the previous intended mode
can be restored.

### Scope Precedence

Effective settings are resolved from broadest to narrowest scope:

1. User
2. Workspace
3. Project
4. Repo

The global kill switch is evaluated before scope settings and blocks all
non-manual loop execution. Manual runs still pass through policy and budget
checks.

### Core Contracts

- Trigger contract: source, event type, payload hash, dedupe key, timestamp,
  trust level, and normalized payload.
- Context contract: allowed memory retrieval, repo facts, docs, project
  instructions, freshness rules, exclusions, and token budget.
- Capability contract: allowed reads, writes, commands, network use, branch or
  worktree creation, memory proposal writes, and approval requirements.
- Evaluation contract: success criteria, checks, reviewers, useful-run signals,
  and cost limits.
- Writeback contract: what may become memory proposals, what requires approval,
  and what must never be written automatically.

### Trust Zones

- User-approved memory: high trust.
- Maintainer-owned repo config: high or medium trust.
- External issue and PR content: low trust; task input only.
- CI logs: data only; parse, never obey.
- Generated summaries: medium trust; verify before storing.
- Accepted memory proposals: high trust.

Default posture is read-only. Forbidden actions include direct pushes to main,
deploys, secret access, destructive migrations, automatic global memory rewrites,
hidden background spend, and loop self-enablement.

## Boundaries

Memory Core owns canonical memories, retrieval, curation, source provenance, and
existing replacement proposals.

Loop Control Plane owns definitions, settings, trigger routing, policy checks,
run ledger, traces, approvals, loop memory proposals, learned skills, and loop
feedback.

Runner adapters, worktree management, safe read-only loop implementations, UI
cards, and `memory loops` CLI commands are later milestones. The foundation
implements control-plane storage and APIs first.

## Consequences

- All external protocol surfaces call the service HTTP API; MCP remains an
  adapter and does not bypass policy or persistence.
- Every loop run has a durable ledger row and trace records, including blocked
  runs.
- Risky actions and durable memory mutations are explicit approval records.
- Built-in definitions are versioned so future behavior changes are auditable.
