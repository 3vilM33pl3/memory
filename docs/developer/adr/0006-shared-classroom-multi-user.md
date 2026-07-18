# ADR 0006: Shared-classroom / multi-user story — spike decision

Date: 2026-07-18
Status: accepted (decision doc — spike, no build)
Relates to: 3VI-823 (spike), 3VI-821 (read-only student mode), 3VI-820 (classroom pack)

## Question

What would it take for a class of students to work against **one shared
memory set** — multiple writers, attribution per student, nobody able to
wreck the set — and is that worth building now?

## Current state (verified in code)

- **Auth is one shared secret with full access.** Every client presents the
  single service `api_token` (`crates/mem-service/src/auth.rs`); the web UI
  is simply handed that token by `GET /v1/web/auth-token`. There are no
  users, roles, scopes, or per-token permissions anywhere in the schema.
- **Writer identity exists end-to-end, but it is attribution, not auth.**
  Every write flows through a resolved writer identity
  (`crates/mem-cli/src/writer_identity.rs`: CLI flag → `MEMORY_LAYER__WRITER_ID`
  env → config → derived default) and is persisted on `sessions.writer_id`
  / `writer_name` (migration 0007, NOT NULL) and on agent workspaces
  (migration 0022). A memory's provenance chain (memory → raw capture →
  task → session) therefore already answers "who wrote this" — trusting
  the client's self-declared identity.
- **Projects are hard namespaces.** All retrieval and mutation is
  project-scoped; per-student projects on one service are isolated by
  construction (though not access-controlled from each other).
- **Read-only student mode now exists** (`service.read_only`, 3VI-821):
  one flag makes the whole service explore-only with a clear 403 story.
- **Concurrency**: agent workspaces (`/v1/agents/workspaces/*`) already
  coordinate multiple concurrent writers on one project; writes are
  append-only immutable versions, so concurrent student writes cannot
  corrupt each other — worst case is duplicate/noisy memories, which
  curation and dedup were built to absorb.

## Options considered

**Option 1 — shared project, self-declared writer ids (zero build).**
Each student sets `MEMORY_LAYER__WRITER_ID=alice`. Attribution lands in the
session chain; the TUI/web can already surface it. Cost: nothing. Risk:
identity is honor-system; any student can write (or delete!) anything —
`/v1/memory` DELETE and archive are one token away. Fine for a supervised
lab, unacceptable for graded work.

**Option 2 — per-student projects + shared read-only reference set
(zero build, composition of shipped pieces).** The shared curated corpus
lives on a read-only service (Mode A of the classroom pack); each student
gets their own project (or their own local stack) for writes. Attribution
is trivially the project name. This is what the classroom pack ships today.
Limitation: no *collaborative* shared write surface.

**Option 3 — real multi-user auth (the build).** Minimum honest scope:
a `tokens` table (token hash → writer_id, role: `teacher|student`,
optional project allowlist), `require_token` resolving to an authenticated
identity, server-side stamping of `writer_id` (stop trusting the client),
role checks on destructive endpoints (delete/archive/proposal decisions =
teacher only), token issuance/revocation CLI (`memory tokens ...`), and web
login by token. Estimate: ~1 migration + auth rework touching every
`require_token` call site + CLI/web surface ≈ **1–2 weeks**, plus it drags
in real security review obligations (rate limits, token storage, audit).

## Decision

**Do not build multi-user auth now.** Classroom needs are covered by the
composition that already shipped: Option 2 as the default (shared
read-only reference set + per-student sandbox projects), upgraded with
Option 1's writer ids when a supervised class wants one collaborative
project. Revisit Option 3 only when there is a concrete institutional user
(a course actually grading against a shared set, or any deployment where
the service crosses a trust boundary) — and treat it then as the same
epic as remote/multi-tenant deployment, because a per-user token model is
the first requirement of both.

## Consequences

- The classroom pack documents the trust model honestly: Mode B sandboxes
  are isolation-by-convention, not security.
- `sessions.writer_id` remains the attribution seam; any future auth build
  should *stamp* it server-side rather than invent a parallel concept.
- The single-token assumption stays load-bearing in `auth.rs`; nothing new
  hardens around it, keeping Option 3's surface small if it ever happens.
