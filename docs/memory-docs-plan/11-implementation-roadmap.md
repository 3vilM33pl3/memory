# Implementation Roadmap

## Objective

Build and publish a polished documentation website for Memory Layer in manageable phases.

## Phase 0 — Decisions

### Decide site generator

Recommended: Mintlify.

| Criterion | Mintlify | Docusaurus |
|---|---|---|
| Fast polish | Excellent | Good |
| Developer docs components | Excellent | Good |
| Self-hosting control | Medium | Excellent |
| Maintenance effort | Low | Medium |
| Customisation | Medium | High |

Start with Mintlify unless there is a strong reason to self-host everything.

### Decide docs location

If the repo already has a `docs/` directory for project docs, use:

```text
docs-site/
```

Otherwise use:

```text
docs/
```

## Phase 1 — Skeleton

Tasks:

1. Add documentation site directory.
2. Add site config.
3. Add homepage.
4. Add quickstart.
5. Add top-level section hubs.
6. Add placeholder pages.
7. Add images directory.
8. Add README for docs contributors.

Acceptance criteria:

- Site runs locally.
- Navigation matches the planned IA.
- Homepage looks credible.
- All top-level nav entries work.
- No dead-end placeholder links in visible nav.

## Phase 2 — Onboarding Content

Tasks:

1. Write quickstart.
2. Write install overview.
3. Write Linux install.
4. Write macOS install.
5. Write PostgreSQL/pgvector guide.
6. Write global wizard guide.
7. Write project wizard guide.
8. Write service setup guide.
9. Add verification and troubleshooting to each page.

Acceptance criteria:

- A new user can install Memory Layer from docs alone.
- Every install path includes health checks.
- Database setup is clear.
- Failure modes link to troubleshooting.

## Phase 3 — Agent Integration Content

Tasks:

1. Write agents overview.
2. Write Codex CLI integration.
3. Write Claude Code integration.
4. Write generic agent integration.
5. Write watcher overview.
6. Write generic watcher page.
7. Write MCP overview.
8. Write MCP setup pages.
9. Add copyable agent prompts.

Acceptance criteria:

- Users understand CLI, watcher, and MCP integration.
- Codex and Claude setup pages have verification steps.
- MCP security warnings are visible.
- Agent prompts are easy to copy.

## Phase 4 — Evaluation Content

Tasks:

1. Write evaluation overview.
2. Write ablation tests page.
3. Write run evaluations page.
4. Write benchmark reports page.
5. Write metrics glossary.
6. Write reproducibility checklist.
7. Write limitations page.
8. Link evaluation from homepage.

Acceptance criteria:

- Evaluation story is visible within one click of homepage.
- Results are framed carefully.
- Readers can distinguish retrieval success from autonomous coding success.
- Reproducibility expectations are explicit.

## Phase 5 — Operations and Reference

Tasks:

1. Write service operations.
2. Write database operations.
3. Write backups and restore.
4. Write security and privacy.
5. Write CLI reference.
6. Write config reference.
7. Write environment variables reference.
8. Write troubleshooting and FAQ.

Acceptance criteria:

- Users can operate the service after install.
- Users know what data is stored and where.
- CLI and config are searchable.
- Common errors have direct fixes.

## Phase 6 — Polish

Tasks:

1. Add screenshots.
2. Add diagrams.
3. Improve homepage hero.
4. Add Open Graph/social preview.
5. Add search metadata.
6. Add redirects if replacing old docs.
7. Add docs CI.
8. Add link checking.
9. Add spell checking.
10. Add contribution guidelines.

Acceptance criteria:

- Site looks polished.
- No broken links.
- Screenshots are current.
- Docs can be previewed in CI.
- New docs follow templates.

## Definition of Done

The docs website is ready when a user can install and configure Memory Layer without reading the README, connect at least one agent, understand watchers and MCP, run or understand evaluations, understand security/privacy implications, search CLI/config reference, and understand the project in under one minute from the homepage.
