# E7 — Education & classroom track (3VI-788: 820, 821, 822, 823)

Grounding (verified): user compose stack is `compose.yaml` at repo root
(postgres/pgvector + single `memory` service serving the web UI, **keyless by
default** — lexical retrieval + deterministic extractive answers; LLM/embeddings
opt-in via env). `memory demo --project X` seeds the embedded demo corpus into
any project, keyless. Web UI fetches the shared api token from
`GET /v1/web/auth-token` and has no read-only notion. Graph viz: activation
heat/size + pulse-on-select spreading particles, layer toggles, depth stepper.
ADR 0004 decided "compose is enough" for keyless/embedded.

## 1. 3VI-821 — read-only / student mode (code, first)
- `mem-api` `ServiceConfig`: add `#[serde(default)] pub read_only: bool`
  (env: `MEMORY_LAYER__SERVICE__READ_ONLY=true`). Update the two manual
  constructions (mem-watch lib.rs test config, mem-cli runtime/tests.rs).
- `mem-service/src/auth.rs`: `read_only_request_allowed(method, path) -> bool`
  (pure, unit-tested) + axum middleware `read_only_guard` applied to the /v1
  router. Allow: GET/HEAD/OPTIONS, websocket, plus POST allowlist
  `/v1/query`, `/v1/query/global`, `/v1/projects/{slug}/resume`,
  `/v1/projects/{slug}/up-to-speed`, `/v1/projects/{slug}/bundle/export`,
  `/v1/projects/{slug}/bundle/export/preview`. Everything else mutating →
  403 "read-only (student) mode". Note: queries still reinforce activation —
  internal dynamics stay alive by design (that IS the lesson); content writes
  are what's blocked. MCP mount already defaults to read-only.
- Expose `read_only` on the `/v1/web/auth-token` response; web UI shows a
  persistent "Student mode — read-only" banner (web/src/api.ts + App shell).
- Tests: unit tests for the allowlist fn; live check against the dev service
  with the env override.

## 2. 3VI-820 — keyless single-machine classroom pack
New `classroom/` directory at repo root:
- `README.md` — teacher guide: Docker-only prerequisites; two class modes
  (A: shared read-only exploration set via `MEMORY_LAYER__SERVICE__READ_ONLY`,
  B: per-student sandbox projects, writes on); setup, verify, reset
  (`docker compose down -v`).
- `compose.classroom.yaml` — overlay on the root compose: classroom defaults
  and a `CLASSROOM_READ_ONLY` toggle mapped to the new service flag.
- `seed.sh` — seeds the shared `classroom` exercise project via
  `memory demo --project classroom` inside the container (keyless), idempotent.
- `worksheet.md` — printable student worksheet aligned with the 3 lessons.
Docs-site: classroom index links to the pack.

## 3. 3VI-822 — cognitive-science curriculum (3 lessons)
New docs-site section `content/docs/classroom/`:
- `index.mdx` (teacher overview + pack setup + how the viz is the instrument),
- `lesson-1-activation-and-decay.mdx` (ACT-R activation, exponential decay,
  reinforcement-on-use; students watch node heat/size change as they query),
- `lesson-2-spreading-activation.mdx` (Collins & Loftus; graph pulse particles,
  hop-distance boosts, co-access),
- `lesson-3-consolidation.mdx` (CLS McClelland 1995, Tse 2007 schemas,
  Park 2023 reflection; `memory structure` + `memory consolidate --dry-run`
  as the classroom instrument).
Each lesson: duration, objectives, prep, guided activity (exact clicks/commands
+ expected observations), the science with the citations already used in
`how-it-works/{reinforcement,consolidation}.mdx`, discussion questions,
exit-ticket assessment. Add `classroom` to docs meta.json.

## 4. 3VI-823 — SPIKE decision doc (no build)
`docs/developer/adr/0006-shared-classroom-multi-user.md`: current state
(single shared api_token = full write access; per-client writer identity
exists end-to-end but is advisory attribution, not auth); options:
(1) shared project + writer-id attribution over today's token,
(2) per-student projects on one service,
(3) real per-user tokens/roles (teacher-admin, student-read/write-own).
Cost/risk each; recommendation: classroom needs are covered by read-only mode
+ per-student projects (option 2 hybrid) now; defer (3) until a real
multi-tenant demand exists. Acceptance: decision + scoping, ticket comment.

## Order, verification, wrap-up
821 → 820 → 822 → 823. Per ticket: fmt/clippy/tests; docs-site build for 822;
live dev-stack verification of read-only mode + seed script; one commit per
ticket; Linear In Progress → Done with closing comments; epic 3VI-788 Done at
the end; push; `memory remember`.

## Checklist
- [ ] 3VI-821: ServiceConfig.read_only + read_only_guard middleware + allowlist unit tests
- [ ] 3VI-821: read_only on /v1/web/auth-token + web student-mode banner
- [ ] 3VI-821: live dev-stack verification (write blocked 403, query/resume allowed)
- [ ] 3VI-820: classroom/ pack (README, compose overlay, seed.sh, worksheet)
- [ ] 3VI-820: seed script verified against a running stack
- [ ] 3VI-822: docs-site classroom section (index + 3 lessons) + meta.json, build green
- [ ] 3VI-823: ADR 0006 shared-classroom spike decision doc
- [ ] Workspace fmt/clippy/tests green; one commit per ticket; push
- [ ] Linear: 820–823 Done with closing comments; epic 3VI-788 Done
- [ ] memory remember
