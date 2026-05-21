# Homepage and Positioning

## Homepage Goal

The homepage should explain Memory Layer quickly and make it feel important.

## Proposed Hero

```mdx
# Memory Layer

Local-first memory for coding agents.

Memory Layer turns project work into durable, searchable knowledge so your next Codex, Claude, or human session starts with evidence instead of guesswork.
```

## Hero CTA Cards

```text
Get Started
Install Memory Layer and configure your first project.

Connect an Agent
Use Memory Layer with Codex CLI, Claude Code, or any MCP-capable client.

Run an Evaluation
Measure the impact of memory with paired no-memory vs full-memory ablations.
```

## Core Positioning

Memory Layer is not just a note-taking tool or a vector database wrapper. It is a memory infrastructure layer for agentic software work.

It helps agents and developers:

- Capture useful project facts.
- Retrieve relevant history with evidence.
- Avoid rediscovering decisions.
- Preserve context across sessions.
- Evaluate whether memory improves outcomes.
- Integrate memory into Codex, Claude Code, MCP clients, and custom tools.

## Homepage Sections

### What is Memory Layer?

Memory Layer captures what happened, curates what matters, and makes it available to agents when future work needs context.

Suggested diagram:

```text
Agent sessions / CLI / TUI / Web UI / Watchers
        ↓
Capture and curation
        ↓
PostgreSQL + pgvector + code graph evidence
        ↓
Search, briefing, MCP tools, eval harness
```

### Why it matters

Coding agents lose context between sessions. READMEs and docs rarely capture the messy “why”. Chat histories are hard to search and hard to trust. Agents can confidently repeat stale assumptions. Memory Layer should be framed as a way to start with evidence, reduce rediscovery, and make project memory measurable.

### Key capabilities

Recommended cards:

```text
Evidence-backed answers
Project memories cite the memories, commits, scans, or activity events used.

Agent-ready retrieval
Expose memory through CLI, TUI, web UI, and MCP tools.

Watcher support
Attach to Codex and Claude sessions and capture useful activity.

Code graph-aware memory
Connect memories to symbols, files, references, and project structure.

Human review loop
Queue and approve replacement proposals before older knowledge is superseded.

Repeatable evaluations
Run paired ablations and inspect success, recall, ranking, token, and latency metrics.
```

### Quick start

```bash
memory wizard --global
cd /path/to/project
memory wizard --dry-run
memory wizard
memory doctor
memory health
memory tui
```

### Evaluation highlight

```mdx
## Built to be measured

Memory Layer includes an evaluation harness for paired ablations such as `no-memory` vs `full-memory`. It records immutable artifacts, compares item-by-item results, reports retrieval quality, and tracks token and latency cost.
```

## Tone

Confident, practical, evidence-led, and slightly opinionated. Avoid claims like “perfect memory” or “solves hallucination”. Prefer “start with evidence”, “reduce rediscovery”, and “make memory measurable”.
