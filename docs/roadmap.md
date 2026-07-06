# Memory Layer — Adoption Roadmap

**Status:** active · **Last updated:** 2026-07-06 · **Tracking:** Linear project "Memory Layer" (epics `3VI-*`, see below)

Memory Layer is a mature, local-first memory system for coding agents. This roadmap turns it into something a much wider audience can **understand, use, and install** — from a first-time beginner to a research scientist, from a classroom to a YouTube demo to an unconventional integration.

The guiding principles: prefer approaches with **scientific backing or a tried-and-tested precedent**, keep a few **deliberately experimental bets** to stay fresh, and sequence **foundation-first** — an effortless install and a first-run "wow" unlock every other audience.

## Where we are

- The engine is strong: hybrid retrieval, an ACT-R-grounded reinforcement/activation model, evidence-backed validation, memory consolidation into insights, a code graph, an automation-loop control plane, and a real evaluation harness with cited science.
- The **adoption surface is thin**: install requires an external PostgreSQL + pgvector + Go on PATH and a two-phase wizard; a fresh install starts empty with no demo or tutorial; the docs carry 15–20 concepts with no 5-minute on-ramp; there is no public API reference, no client libraries, and no turnkey way for a scientist or teacher to get started.

This roadmap closes that gap.

## The nine tracks

Each track is a Linear epic. Ticket estimates are S / M / L. "Reuse" points at existing shipped code to build on rather than greenfield.

### E0 — Backlog & roadmap foundation
*Audience: maintainers. Land first.* A clean, legible backlog and a public roadmap make the project credible.
- Reconcile the meta-memory tickets against the consolidation work that already shipped.
- Close scan tickets that predate the current eval harness and graph channel.
- Publish this roadmap in-repo and on the website.

### E1 — Effortless install *(the funnel gate — everyone)*
Nothing downstream matters if install is hard.
- **User-facing Docker Compose full stack** — `docker compose up` brings up pgvector + service + migrations + web. Highest-leverage item. *Reuse: `evals/docker/app-build-sequence/`.*
- One-line curl installer for Linux/macOS. *Reuse: `Formula/memory-layer.rb`, `packaging/`.*
- `memory doctor` preflight that detects and fixes the common failures (DB unreachable, pgvector missing, PATH, uninitialized project).
- Collapse the two-phase wizard into a single `memory setup`.
- Real Windows support (winget/scoop + WSL2).
- **Spike:** a zero-dependency embedded mode for demos and classrooms (decision doc — bundling Postgres keeps 100% of the code; a DuckDB read path would mean reimplementing the repository layer, since today's offline mode is a write-only outbox).

### E2 — First-run wow *(beginners, demo viewers)*
Turn an empty database into a five-minute "aha".
- **`memory demo`** — one command seeds a rich, privacy-safe showcase project so query, graph, and resume immediately show something. Works keyless (lexical). *Reuse: the eval fixtures and the static demo corpus.* Grounded in the **worked-example effect** (Sweller): people learn a system faster from a filled one.
- Guided first-run tour (remember → query → resume) with the honest message: *you only need three commands*.
- A single 5-minute quickstart page.
- Progressive-disclosure help: the core commands up front, the rest behind `--all`.
- Empty-state calls-to-action across the TUI and web tabs.

### E3 — Concept coherence & docs *(beginners → intermediate)*
- **Fix the memory-type divergence** — the docs describe 8 types; the code has 17. Generate the docs table from the enum (or add a drift-failing test) so they can never diverge again.
- A "three things to know" mental-model page (memories, retrieval, reinforcement) with one diagram.
- A glossary and concept map defining every term once.
- Restructure the docs into **Beginner / Daily use / Advanced / The science** — the cited-science pages become a showcased section, not background noise.

### E4 — Demo & video assets *(YouTube/TikTok, evaluators)*
The 3D memory graph is the most shareable thing in the product.
- Expose **activation** on the memory-graph API and render it as node colour/size.
- **Experimental:** animate spreading activation (particles along links) and decay (fading colour) — this literally renders the ACT-R model the system runs, so it is both eye-catching and honest.
- A 2D graph fallback (low-end machines, screenshots, classrooms).
- Produce and publish launch clips (a 30-second graph hero + short walkthroughs) from the reproducible `demo/` pipeline.
- Upgrade the static web sandbox with client-side search and the graph view — a real "Try it now" from the landing page.

### E5 — Integration surface & API *(experts, scientists, integrators)*
Turn a tool into a platform.
- **Document and freeze the HTTP API v1 and publish an OpenAPI spec** (none exists today). *Generate from `crates/mem-service/src/routes.rs`.*
- A **Python client** (+ notebook quickstart) — scientists live in Python — and a TypeScript client generated from the spec.
- First-class non-git ingestion (`memory ingest`) for arbitrary documents and directories.
- Five integration recipes: Obsidian, a research-paper corpus, game NPC memory, home automation, and **"memory for your AI chats"** via the existing MCP server + Claude Desktop config (the cheapest consumer-angle experiment — pure docs).
- *Stretch:* outbound webhooks, once the recipes prove demand.

### E6 — Research & reproducibility *(scientists)*
- A turnkey `memory eval reproduce <suite>` (one command, end-to-end), with a keyless lexical subset for those without API keys.
- Citeable **reproducible experiment bundles** (config + fixtures + seeds + results). *Reuse: memory bundles export/import.*
- An anonymized eval dataset with `CITATION.cff` and a DOI.
- A methods write-up / preprint building on the citations already in the science docs (ACT-R; McClelland 1995; Tse 2007; RAPTOR; GraphRAG).

### E7 — Education & classroom *(teachers, students)*
Depends on E1 + E2 + E4 existing first.
- A **classroom pack**: single-machine, keyless, with a pre-seeded exercise project and a printable worksheet.
- A read-only / student mode.
- A **cognitive-science curriculum** (activation & decay, spreading activation, consolidation) taught *with the live graph visualization as the instrument* — the tool teaches the very concepts it implements.
- *Spike:* a multi-user / shared-classroom story (the system is single-writer-identity today).

### E8 — Trust & polish *(cross-cutting — everyone)*
Red eval gates and known bugs poison demos and word-of-mouth.
- **Green the adversarial answer-synthesis gate** (`3VI-773`) — refuse on weak evidence, stop echoing superseded facts. First priority: it undercuts every demo and every scientific claim.
- Fix the two known live bugs (`/v1/curate` timeout, plan `--thread-key` friction).
- An error-message pass so every top failure names its own fix.
- Accessibility and naming consistency across CLI / TUI / web / docs.
- **Opt-in** anonymous install/first-run telemetry — the only way to know whether the funnel improved.

## Sequencing

```
E0                     (first — clean slate + public roadmap)
E1.1 compose ─► E2.1 demo ─► E2.3 quickstart ─► E4.4 clips, E7.1 classroom
E1.3 doctor  ─► E8.3 error-message pass
E4.1 activation-in-graph ─► E4.2 animation ─► E7.3 curriculum
E5.1 OpenAPI ─► E5.2 Python, E5.3 TypeScript
E8.1 (3VI-773) — start immediately, independent of everything
Spikes (embedded mode, multi-user) — before any build ticket that depends on them
```

**First five tickets:** backlog reconcile (E0), user Docker Compose (E1), `memory demo` (E2), the memory-type fix (E3), and the adversarial gate `3VI-773` (E8) — plus publishing this roadmap.

## Experimental bets (and why they are safe)

- **Zero-dependency embedded mode** — kept as a *spike* rather than a build, because the read path is entirely Postgres+pgvector; the honest options are "bundle Postgres" or "compose is enough".
- **Animated activation visualization** — feasible on the existing 3d-force-graph stack, and grounded in the ACT-R activation model the engine already computes.
- **The tool as a teaching instrument** — the science docs are already strong; turning them into classroom lessons driven by the live visualization is low-risk and distinctive.

## What we are deliberately *not* doing yet

Hosted/SaaS (revisit once the API is stable), internationalization (revisit if telemetry shows demand), and a hosted multi-tenant sandbox (the static demo is the tried-and-tested path).
