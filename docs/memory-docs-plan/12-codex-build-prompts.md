# Codex Build Prompts

These prompts are designed to build the documentation site incrementally. Use one prompt at a time.

## Prompt 1 — Create the docs site skeleton

```text
You are working in the Memory Layer repository.

Goal:
Create a new documentation website skeleton inspired by the structure of https://docs.openclaw.ai/, but branded for Memory Layer.

Use Mintlify unless the repo already has a clear docs website framework. If Mintlify is not appropriate, explain why and use the closest maintainable alternative.

Requirements:
- Create a docs website directory without disrupting existing docs.
- Add a site config with top navigation:
  Get started, Install, Concepts, Agents, Watchers, MCP, Evaluations, Operations, Reference, Help.
- Add a homepage.
- Add a quickstart page.
- Add section hub pages for every top-level nav item.
- Add placeholder child pages where needed so navigation works.
- Do not invent unsupported commands. Use TODO comments where exact commands need confirmation.
- Keep the design polished, dark-mode friendly, and developer-docs oriented.
- Run local validation/build commands if available.
- Report changed files and any commands I should run next.
```

## Prompt 2 — Build the homepage

```text
You are working in the Memory Layer documentation website.

Goal:
Create a polished homepage for Memory Layer.

Use the positioning:
"Local-first memory for coding agents."

The homepage should include:
- Hero title and subtitle.
- Three primary CTA cards:
  1. Get Started
  2. Connect an Agent
  3. Run an Evaluation
- A "What is Memory Layer?" section.
- A "Why it matters" section.
- Key capability cards:
  evidence-backed answers,
  agent-ready retrieval,
  watcher support,
  code graph-aware memory,
  human review loop,
  repeatable evaluations.
- A concise quick start.
- A "Built to be measured" section that links to evaluations.
- A "Start here" routing section.

Do not overclaim. Avoid saying Memory Layer solves hallucination or gives perfect memory.
Run docs validation/build if available.
```

## Prompt 3 — Write install and onboarding docs

```text
You are working in the Memory Layer documentation website.

Goal:
Write the install and onboarding section.

Create or update these pages:
- install/index
- install/requirements
- install/linux-debian
- install/macos-homebrew
- install/from-source
- install/postgresql-pgvector
- install/wizard-global
- install/wizard-project
- install/service-setup
- install/update
- install/uninstall

Use commands from the repository README where available.
Include prerequisites, commands, expected result, verification steps, troubleshooting links, and next steps.

Be especially careful with database setup:
- PostgreSQL is required.
- pgvector must be installed and enabled.
- include `psql "$DATABASE_URL" -c "SELECT 1;"`
- include `CREATE EXTENSION IF NOT EXISTS vector;`
- include `memory doctor` and `memory health` verification.

Do not invent package names or service names if not confirmed by the repo. Mark uncertain details as TODO.
Run docs validation/build if available.
```

## Prompt 4 — Write concepts docs

```text
You are working in the Memory Layer documentation website.

Goal:
Write the core concepts section.

Create or update:
- concepts/index
- concepts/mental-model
- concepts/projects
- concepts/memories
- concepts/evidence
- concepts/curation
- concepts/retrieval
- concepts/embeddings
- concepts/code-graph
- concepts/activity-events
- concepts/trust-and-staleness

Use the mental model:
Capture → Curate → Store → Retrieve → Verify.

Explain Memory Layer as local-first memory infrastructure for coding agents.
Keep examples concrete and project-focused.
Make evidence, curation, and staleness central themes.
Do not make unsupported claims.
Run docs validation/build if available.
```

## Prompt 5 — Write agents, watchers, and MCP docs

```text
You are working in the Memory Layer documentation website.

Goal:
Write docs for agent integration, watchers, and MCP.

Create or update:
- agents/index
- agents/codex-cli
- agents/claude-code
- agents/generic-agent
- agents/agent-install-prompt
- agents/agent-project-init-prompt
- agents/agent-briefings
- agents/permissions-and-safety
- watchers/index
- watchers/generic-watcher
- watchers/codex-profile
- watchers/claude-profile
- watchers/session-detection
- watchers/token-pressure
- watchers/lifecycle-and-heartbeats
- watchers/troubleshooting
- mcp/index
- mcp/stdio
- mcp/streamable-http
- mcp/tools
- mcp/client-setup
- mcp/security
- mcp/examples

Use confirmed repo behaviour where possible.
Where exact config is unknown, use TODO blocks instead of inventing details.
Include verification steps on every setup page.
Add security warnings for exposing MCP HTTP.
Run docs validation/build if available.
```

## Prompt 6 — Write evaluation docs

```text
You are working in the Memory Layer documentation website.

Goal:
Write the evaluation section so Memory Layer's scientific story is visible and credible.

Create or update:
- evals/index
- evals/ablation-tests
- evals/run-evals
- evals/benchmark-reports
- evals/metrics
- evals/reproducibility
- evals/interpreting-results
- evals/limitations

Use careful language:
- Memory systems should be evaluated by behaviour they improve.
- Paired ablations compare no-memory vs full-memory.
- Retrieval success is not the same as autonomous coding success.
- Token reductions should be interpreted alongside quality.
- Stale or wrong memories can harm outcomes.

Include metric explanations:
success rate, Recall@K, MRR, nDCG, assertion recall, token cost, latency.

Do not invent benchmark numbers. Use existing reports from the repo only if present.
Run docs validation/build if available.
```

## Prompt 7 — Write operations, reference, and help docs

```text
You are working in the Memory Layer documentation website.

Goal:
Write operations, reference, and help docs.

Create or update:
- operations/index
- operations/service-operations
- operations/database-operations
- operations/backups-and-restore
- operations/observability
- operations/performance
- operations/security-and-privacy
- operations/multi-project-use
- reference/index
- reference/cli
- reference/config-global
- reference/config-project
- reference/database-schema
- reference/environment-variables
- reference/exit-codes
- reference/files-and-directories
- help/index
- help/troubleshooting
- help/faq
- help/common-errors
- help/doctor-and-health
- help/support-bundle
- help/contributing-docs

For CLI and config reference, inspect the repo and document actual commands/options.
Do not invent flags.
Use tables for reference material.
Use troubleshooting pages for common errors.
Run docs validation/build if available.
```

## Prompt 8 — Add diagrams and screenshots

```text
You are working in the Memory Layer documentation website.

Goal:
Add diagrams and screenshot placeholders that make the site feel polished.

Add or update:
- Architecture diagram
- Capture → Curate → Store → Retrieve → Verify diagram
- Agent integration diagram
- Watcher lifecycle diagram
- MCP flow diagram
- Evaluation harness flow diagram

If actual screenshots are not available, add clearly labelled placeholder image files or TODOs, not fake screenshots.

Place diagrams near relevant explanations.
Use consistent dark-mode-friendly styling.
Run docs validation/build if available.
```

## Prompt 9 — Polish and validate

```text
You are working in the Memory Layer documentation website.

Goal:
Polish and validate the docs site.

Tasks:
- Check navigation for dead links.
- Check titles are action-oriented.
- Check every task guide has a Verify section.
- Check every major page has Next steps.
- Check no unsupported claims are made.
- Check no secrets or local-only paths were committed.
- Add docs contribution notes.
- Add build/preview instructions.
- Run formatter, link checker, and docs build if available.

Report:
- What was changed.
- Broken links fixed.
- Remaining TODOs.
- Commands to preview the site.
```
