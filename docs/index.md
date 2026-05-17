# Memory Layer Docs

Memory Layer is a local-first project memory system for coding agents and developers. It captures durable project knowledge, stores it in PostgreSQL with pgvector, and makes it available through a TUI, browser UI, and agent-friendly CLI.

It is designed for the real workflow: several agents, several projects, multiple embedding models, and a human who needs to understand exactly why an answer was produced.

![Memory Layer cited query answer](img/tui/query-tab.png)

![How Memory Layer turns project work into cited context](img/memory-layer-flow.png)

## Highlights

- **Cited answers from project memory:** ask a question, get an answer, and inspect the exact ranked memories behind it.
- **Code graph-aware retrieval:** extract parser-backed symbols, references, graph edges, and evidence, then use that structure to explain why code-related memories were found.
- **Multi-embedding retrieval:** configure several embedding backends, keep all spaces populated, and activate a different backend without reindexing.
- **Distributed agent operations:** watch Codex and Claude sessions, token pressure, rate limits, watcher heartbeats, and process state from one dashboard.
- **Get-up-to-speed briefings:** persisted activities and recent changes become concise context packs for new or returning agents.
- **Repeatable evaluation:** run paired no-memory vs full-memory ablations with artifacted results, gates, token accounting, and retrieval-quality metrics.
- **Curated knowledge base:** browse canonical memories, inspect provenance, and review proposed replacements before old knowledge is superseded.

![Memory Layer agents dashboard](img/tui/agents-tab.png)

## Start Here

- [Install Memory Layer and configure your first project](user/getting-started.md)
- [Open the terminal UI](user/tui/README.md)
- [Use the browser UI](user/web-ui.md)
- [Ask cited questions from project memory](user/tui/query.md)
- [Troubleshoot setup and runtime errors](user/cli/doctor.md)

## Feature Walkthroughs

- **Query and evidence:** [Query Tab](user/tui/query.md), [Query Command](user/cli/query.md), [Code Graph Extraction](user/cli/graph.md)
- **Memory management:** [Memories Tab](user/tui/memories.md), [Review Tab](user/tui/review.md), [Memory Types Reference](developer/architecture/memory-types.md)
- **Operations:** [Browser UI](user/web-ui.md), [Agents Tab](user/tui/agents.md), [Watcher Health](user/cli/watchers.md), [Errors Tab](user/tui/errors.md), [Doctor Diagnostics](user/cli/doctor.md)
- **Model switching:** [Embeddings Tab](user/tui/embeddings.md), [Embedding Operations](user/cli/embeddings.md)
- **Re-entry:** [Activity Tab](user/tui/activity.md), [Get Up To Speed](user/cli/up-to-speed.md), [Resume Briefings](user/cli/resume.md)
- **Evaluation:** [Beginner Guide To Evaluations](user/evaluation-guide.md), [Automated Evaluation](user/cli/eval.md), [Memory Improvement Evaluation](developer/evaluation-memory-improvement.md)

## CLI Reference

- [Wizard And Bootstrap](user/cli/wizard.md)
- [Service Commands](user/cli/service.md)
- [Status Command](user/cli/status.md)
- [Doctor Diagnostics](user/cli/doctor.md)
- [Query Command](user/cli/query.md)
- [Code Graph Extraction](user/cli/graph.md)
- [Remember Command](user/cli/remember.md)
- [Checkpoint Workflow](user/cli/checkpoint.md)
- [Activities Command](user/cli/activities.md)
- [Get Up To Speed](user/cli/up-to-speed.md)
- [Embedding Operations](user/cli/embeddings.md)
- [Watcher Health](user/cli/watchers.md)
- [TUI Command](user/cli/tui.md)
- [Shell Completion](user/cli/completion.md)
- [History](user/cli/history.md), [Prune History](user/cli/prune-history.md), [Stats](user/cli/stats.md), [Dev Command](user/cli/dev.md)

## Developer Docs

- [Developer Documentation Index](developer/README.md)
- [Future Development](future-development/README.md)
- [Dev Stack vs Installed Stack](developer/dev-stack.md)
- [How Skills Work](developer/skills/how-skills-work.md)
- [Architecture Overview](developer/architecture/overview.md)
- [Memory Types Reference](developer/architecture/memory-types.md)
- [How Memory Layer Works](developer/architecture/how-it-works.md)
- [Embeddings and Search](developer/architecture/embeddings-and-search.md)
