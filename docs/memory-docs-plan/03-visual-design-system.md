# Visual Design System

## Goal

Create a documentation site that feels similar in quality and structure to OpenClaw’s docs, without copying its brand.

Desired feel:

- Modern developer docs.
- Dark-mode friendly.
- High contrast.
- Compact but readable.
- Product-led rather than academic.
- Technical and serious.
- Polished enough to impress a reviewer quickly.

## Recommended Base

Use Mintlify’s default layout as the base if using Mintlify.

Suggested colour direction:

| Role | Colour direction |
|---|---|
| Primary | Violet / indigo |
| Accent | Cyan or electric blue |
| Success | Green |
| Warning | Amber |
| Danger | Red |
| Background | Near-black / slate |
| Code | Dark slate |

## Brand Motifs

Use visual metaphors related to:

- Layers.
- Graphs.
- Evidence trails.
- Timeline continuity.
- Project memory.
- Agent sessions.
- Search and retrieval.

Avoid generic “brain” imagery. Better motifs are stacked translucent layers, nodes connected to code symbols, timelines becoming retrieval results, evidence cards with citations, and context briefing panels.

## Homepage Visual

Create a hero diagram showing:

```text
Codex / Claude / Human Developer
        ↓
Capture: watcher, CLI, TUI, web UI
        ↓
Curate: review, replace, preserve evidence
        ↓
Store: PostgreSQL, pgvector, code graph
        ↓
Retrieve: CLI, MCP, briefings, evals
```

Style: clean technical diagram, dark background, violet/cyan highlights, clear labels, no excessive detail.

## Card Style

Use cards heavily for routing.

Card types:

- Capability cards.
- Task cards.
- Warning cards.
- Evaluation metric cards.

Example capability card:

```mdx
<Card title="Evidence-backed answers" icon="quote">
  Ask a project question and see the exact memories used to answer it.
</Card>
```

## Screenshot Plan

Prepare screenshots for:

1. Memory TUI memories tab.
2. Web UI project view.
3. Evidence-backed query result.
4. Watchers/session monitoring.
5. Evaluation report output.
6. MCP client setup.
7. `memory doctor` / health output.

## Diagram Plan

Create diagrams for:

1. Memory Layer architecture.
2. Capture → curate → retrieve lifecycle.
3. Agent integration model.
4. Watcher lifecycle.
5. MCP tool flow.
6. Evaluation harness flow.
7. Database/storage model.
8. Curation and replacement proposal flow.

## Visual Rules

1. One primary call-to-action per page.
2. Prefer cards over long lists when routing users.
3. Keep hero copy short.
4. Put commands early.
5. Use diagrams to explain architecture, not as decoration.
6. Include verification output after install commands.
7. Use admonitions sparingly and only where they matter.
