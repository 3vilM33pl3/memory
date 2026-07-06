# ADR-0004: Zero-dependency embedded mode — spike decision

- Status: accepted (decision doc; no build work scheduled)
- Date: 2026-07-07
- Ticket: 3VI-795 (adoption roadmap E1, spike)

## Question

Should Memory Layer offer a zero-dependency "embedded" mode — a single binary a
beginner, demo viewer, or classroom can run with no PostgreSQL — and if so, how?

Three options were compared:

1. **Bundled PostgreSQL** — ship/auto-download a private Postgres + pgvector
   managed by the service (precedent: Supabase CLI, `postgresql_embedded`,
   pgrx test harness).
2. **DuckDB read path** — put the repository layer behind a trait and add a
   DuckDB implementation.
3. **"Compose is enough"** — treat the bundled Docker Compose stack as the
   zero-dependency answer and invest nothing further.

## Evidence

- **The read path is Postgres-shaped end to end.** Every retrieval query in
  `mem-search`/`mem-service` uses sqlx against PostgreSQL: pgvector cosine
  similarity, `ts_rank` full-text search, pg_trgm GIN trigram indexes
  (migration 0025), recursive CTEs for the graph channel, and 25 embedded
  migrations applied via `sqlx::migrate!` at boot.
- **Today's DuckDB "offline mode" shares nothing with a read path.**
  `crates/mem-service/src/offline.rs` is a write-only outbox: one
  `offline_outbox` table whose rows are replayed into Postgres by
  `sync_offline_batch`. Zero query/search/graph capability exists on DuckDB;
  option 2 means reimplementing the entire repository layer plus a vector
  index, and then maintaining behavioral parity (the determinism guarantees
  and eval gates would need to hold on both engines).
- **The compose stack already delivers the target UX.** Since 3VI-790,
  `docker compose up` brings up pgvector + service (migrations on boot) + web
  UI, keyless, verified end to end on a fresh database. The quickstart and the
  one-line installer (3VI-791) both route users without a database to it.
- **Keyless is orthogonal to embedded.** Lexical retrieval and deterministic
  answer synthesis already run with no API keys; nothing about "no LLM"
  requires "no Postgres".

## Decision

**Option 3 — compose is enough, for now.** The compose stack already gives a
one-command, zero-config, keyless install on every platform with Docker, and
it runs the *real* engine — same migrations, same indexes, same eval-gated
behavior. Nobody who can run a demo or a classroom machine is materially
better served by an embedded binary than by `docker compose up`, and the two
alternatives carry real costs:

- Option 2 (DuckDB) is rejected outright: a second storage engine under the
  repository layer would double the query surface to keep correct and
  eval-gated, for a mode whose only benefit is avoiding Docker.
- Option 1 (bundled Postgres) is *deferred, not rejected*. It keeps 100% of
  the code and is the right shape if real demand appears — the concrete
  trigger would be classroom/teacher feedback (E7) that Docker is a blocker on
  managed/locked-down machines. If that happens, implement it as
  `memory service run --embedded` using a `postgresql_embedded`-style managed
  instance with pgvector, storing its data under the existing runtime dir.

## Consequences

- No embedded build ticket is opened now; 3VI-795 closes with this document.
- The classroom pack (3VI-820) should ship compose-based and *measure* whether
  Docker is actually a barrier before anyone builds option 1.
- The install funnel documents one honest story: Docker for zero-dependency,
  native packages + your own Postgres for production-ish installs.
