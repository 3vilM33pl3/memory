# Curation Rules

## Purpose

This document defines how raw task captures become canonical project memory.

The curation pipeline should be:
- deterministic by default
- provenance-preserving
- conservative
- easy to audit

The system should prefer missing a weak memory over storing a misleading one.

---

## Inputs to Curation

Curation operates on uncured `raw_captures`.

A raw capture may include:
- project
- task title
- user prompt
- agent summary
- files changed
- git diff summary
- test runs and outcomes
- command outputs
- notes or lessons learned

Curation may also inspect existing nearby canonical memory entries for dedupe and merge decisions.

---

## Output of Curation

Curation produces one or more `memory_entries` plus provenance.

Each output memory entry must have:
- canonical text
- short summary
- memory type
- project scope
- confidence
- importance
- provenance links
- timestamps

Optional:
- tags
- relations to other memory entries

---

## Golden Rule

No canonical memory without provenance.

If the system cannot tie a candidate memory to concrete evidence, it must:
- discard it
- or mark it for manual review in a future version

For v1, prefer discard.

---

## Memory Types

At minimum, classify into one of:
- `architecture`
- `convention`
- `decision`
- `incident`
- `debugging`
- `environment`
- `domain_fact`
- `plan`

### Suggested meanings

#### architecture
How the system is structured or how components interact.

#### convention
Established project practice or coding workflow.

#### decision
A deliberate choice with rationale or constraint.

#### incident
A notable failure and its verified resolution.

#### debugging
A durable troubleshooting lesson, not a transient attempt.

#### environment
Facts about tooling, setup, build, or runtime environment.

#### domain_fact
Stable fact about the business or product domain.

#### plan
An approved execution plan that should guide the current implementation thread.

---

## What Should Be Curated

Curate information that is:
- durable
- reusable
- verified
- likely to help future tasks

Examples:
- “Refresh tokens are single-use and rotated on renewal.”
- “Database migrations are generated via the internal migration tool.”
- “This service must start after PostgreSQL is available.”
- “Auth middleware must run before rate limiting.”

---

## What Should Not Be Curated

Do not curate:
- transient failed attempts
- speculative guesses
- vague opinions
- one-off noise with no future value
- chain-of-thought style debugging notes
- duplicate statements with no new evidence

Examples to reject:
- “Maybe the bug is in refresh.rs”
- “Tried moving the middleware but not sure”
- “This felt messy”

---

## Canonicalization Rules

When creating canonical memory text:
- make it concise
- make it declarative
- preserve technical specificity
- avoid conversational phrasing
- avoid unsupported causality

Prefer:
- “Refresh tokens are invalidated after successful rotation.”

Avoid:
- “I changed the auth stuff and it seems better now.”

---

## Summary Rules

Each memory entry should also have a short summary.

Guidelines:
- 3 to 8 words where possible
- noun-phrase style preferred
- easy to scan in search results

Examples:
- `JWT refresh token rotation`
- `Auth middleware ordering`
- `Migration generation workflow`

---

## Dedupe Rules

Curation must check for duplicates before writing a new canonical memory.

Use a layered approach:
1. exact normalized text match
2. trigram or fuzzy similarity
3. same task/file provenance
4. overlapping tags and memory type

If a candidate is effectively the same as an existing entry:
- update provenance
- update timestamps and confidence if warranted
- do not create a duplicate row

If a candidate partially overlaps:
- merge only if the merged statement remains accurate and specific
- otherwise keep entries separate

---

## Provenance Rules

Every memory entry must be backed by at least one source.

Allowed provenance sources:
- task prompt
- file path
- git commit
- command output
- test result
- explicit capture note

Preferred provenance bundle:
- task ID
- file path
- short source excerpt

The system should retain provenance even when entries are merged.

---

## Confidence Rules

Confidence should reflect how strongly the memory is supported.

Suggested heuristic:
- high confidence: supported by changed files plus passing tests or explicit verified summary
- medium confidence: supported by source files or summaries but not directly verified by tests
- low confidence: weakly supported, should usually not be curated in v1

If confidence is too low, do not create a canonical memory entry.

---

## Importance Rules

Importance should reflect likely future value.

Raise importance for:
- cross-cutting architectural rules
- security-relevant behavior
- deployment/runtime requirements
- repeated conventions
- frequent debugging lessons

Lower importance for:
- narrow one-file details
- temporary migration quirks
- stale implementation details with low reuse value

---

## Merge Rules

When merging candidate memory into an existing entry:
- preserve the clearest canonical wording
- preserve all valid provenance
- update tags if the new evidence adds useful indexing
- avoid broadening the entry beyond what the evidence supports

Do not merge if the result becomes ambiguous.

---

## Relation Rules

Create `memory_relations` only when the relation is explicit and useful.

Examples:
- `depends_on`
- `supersedes`
- `related_to`
- `caused_by`
- `implements`

Do not invent relations from weak lexical similarity.

---

## Archiving Rules

Canonical memory should be archived, not hard deleted, when it becomes stale or low-value.

Archive candidates:
- superseded conventions
- obsolete environment setup
- stale implementation details no longer referenced

Do not archive aggressively in v1.
Prefer manual or threshold-based archiving with audit logs.

---

## LLM-Assisted Curation Rules

If optional LLM support is enabled, it may be used only for:
- compressing verified candidate assertions
- suggesting clearer canonical wording
- proposing merge wording between already-supported memories

The LLM must not:
- invent provenance
- infer unsupported architecture facts
- produce canonical memory from speculation alone
- silently overwrite existing memory

All LLM-assisted outputs must still pass deterministic validation.

---

## Validation Checklist

Before writing a memory entry, ensure:
- it is useful beyond the current task
- it is not speculative
- it has provenance
- it is not a duplicate
- it has a valid type
- its wording is concise and canonical
- confidence is above the acceptance threshold

If any of these fail, skip the write.

---

## Example Good Memory

Canonical text:
- `Refresh tokens are single-use and invalidated after successful rotation.`

Why it is good:
- durable
- specific
- actionable
- provenance-friendly
- easy to search

---

## Example Bad Memory

Canonical text:
- `Auth was weird so I changed the refresh flow and it worked.`

Why it is bad:
- vague
- conversational
- unsupported
- poor retrieval value

---

## Default Conservative Policy

When uncertain:
- do not store
- keep raw capture
- allow future recuration when better evidence exists
