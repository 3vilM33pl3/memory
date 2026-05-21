# Memory Layer Documentation Website — Overall Plan

## Goal

Create a polished documentation website for **Memory Layer** that has the same kind of developer-friendly feel as the OpenClaw documentation: a crisp product landing page, strong top-level navigation, compact overview pages, quick-start-first onboarding, and deep guides for installation, configuration, operations, evaluation, and extension.

The website should make Memory Layer feel like a serious, installable developer platform rather than “a repo with a long README”.

## Design Reference

OpenClaw’s docs use a Mintlify-style pattern: top product tabs, grouped left navigation, a short homepage with quick start and routing blocks, section hubs, and command-led guides. Memory Layer should adopt that information design pattern while keeping a distinct identity: **local-first memory infrastructure for coding agents**.

## Recommended Site Stack

Use **Mintlify** if speed and polish are the priority. Use **Docusaurus** only if you want maximum control and fully open-source self-hosting.

Recommended structure:

```text
docs-site/
  mint.json
  index.mdx
  quickstart.mdx
  install/
  concepts/
  agents/
  watchers/
  mcp/
  evals/
  operations/
  reference/
  help/
  images/
```

## Proposed Top Navigation

```text
Get started
Install
Concepts
Agents
Watchers
MCP
Evaluations
Operations
Reference
Help
```

## Primary Audiences

### Developers using coding agents

They need to install Memory Layer, configure a project, connect Codex/Claude/other agents, query memory, and understand the evidence returned.

### Agent/tool builders

They need to understand the CLI, TUI, browser UI, watchers, MCP server, storage model, extension points, and integration patterns.

### Evaluators, recruiters, and OpenAI-style reviewers

They need to see scientific grounding, repeatable evaluations, ablations, metrics, limits, and evidence of engineering maturity.

## Documentation Principles

1. **Start with outcomes, not internals.** Explain what Memory Layer enables before explaining tables, configs, daemons, or embeddings.
2. **Every major page should answer “What do I run next?”** Include commands, expected output, verification steps, and troubleshooting links.
3. **Evidence is a product theme.** Show how answers cite memories, commits, scans, or activity events.
4. **Make evaluation visible.** The evaluation harness is a differentiator and should be part of the product story.
5. **Separate human and agent workflows.** Include human-oriented docs and copyable agent prompts.
6. **Prefer short hub pages plus deep child pages.** Each top-level section should route readers into practical task pages.

## Files in This Planning Pack

| File | Purpose |
|---|---|
| `01-information-architecture.md` | Navigation, page tree, and section map |
| `02-homepage-and-positioning.md` | Landing page structure and product messaging |
| `03-visual-design-system.md` | Look-and-feel inspired by OpenClaw, adapted for Memory |
| `04-content-model-and-page-types.md` | Templates for overview pages, guides, references, troubleshooting |
| `05-install-and-onboarding-section.md` | Install docs, quick start, setup wizard, platform-specific flows |
| `06-core-concepts-section.md` | Conceptual model: projects, memories, evidence, curation, retrieval |
| `07-agents-watchers-and-mcp-section.md` | Agent integration, watchers, Codex, Claude, MCP |
| `08-evaluations-section.md` | Evaluation docs, ablations, metrics, reports, scientific framing |
| `09-operations-and-security-section.md` | Service operations, storage, backups, privacy, observability |
| `10-reference-and-help-section.md` | CLI reference, config reference, troubleshooting, FAQ |
| `11-implementation-roadmap.md` | Build phases, milestones, acceptance criteria |
| `12-codex-build-prompts.md` | Ready-to-use prompts for Codex CLI to implement the site |

## Build Phases

### Phase 1 — Skeleton

Create the site, navigation, homepage, section hubs, theme, and placeholder pages.

### Phase 2 — High-value onboarding

Write quick start, install overview, Linux/macOS install, PostgreSQL/pgvector setup, global wizard, project wizard, health checks, and first query.

### Phase 3 — Agent integration

Document Codex CLI, Claude Code, generic agents, watchers, MCP, agent prompts, and troubleshooting.

### Phase 4 — Evaluation and credibility

Document ablations, metrics, benchmark reports, reproducibility, interpretation, and limitations.

### Phase 5 — Operations and polish

Add service operations, backups, security/privacy, CLI/config reference, screenshots, diagrams, link checking, and docs contribution guidance.

## Success Criteria

A first-time developer should be able to understand the product in under 60 seconds, install it, configure PostgreSQL and pgvector, initialise a project, connect an agent, query memory with evidence, run or understand an evaluation, and find help when something breaks.
