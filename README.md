# Memory Layer

Memory Layer is a local-first memory system for coding agents and developers. It turns project work into durable, searchable knowledge so the next Codex, Claude, or human session can start with evidence instead of guesswork.

It captures what happened, curates what matters, stores it in PostgreSQL with pgvector, and exposes it through a fast TUI, browser UI, and agent-friendly CLI.

![Memory Layer query answer and evidence](docs/img/tui/query-tab.png)

## Why It Is Interesting

- **Answers with evidence:** ask a project question and see both the synthesized answer and the exact memories used to produce it.
- **Multi-embedding search:** keep OpenAI, Voyage, Cohere, Gemini, or local OpenAI-compatible embedding spaces side by side, then switch the active retrieval backend without recomputing.
- **Distributed agents:** monitor Codex and Claude sessions across projects, including token pressure, context usage, rate limits, process details, and open ports.
- **Agent-linked watchers:** background watchers attach to agent sessions, identify the project automatically, heartbeat to the service, and stop when the owning agent exits.
- **Get up to speed:** persisted activity events, recent memory changes, commits, warnings, and token summaries become a briefing for new or returning agents.
- **Human review loop:** curation can queue replacement proposals so important memory changes can be approved before older knowledge is superseded.

![Memory Layer agents dashboard](docs/img/tui/agents-tab.png)

## Table of Contents

- [Quick Start](#quick-start)
- [Quick Start (Developers)](#quick-start-developers)
- [Feature Tour](#feature-tour)
- [Documentation](#documentation)
- [Development](#development)
- [License](#license)

## Quick Start

The fastest path is:

1. Install the package.
2. Run `memory wizard --global` once per machine.
3. Run `memory wizard` inside each repository.
4. Let Memory Layer auto-derive a writer identity, or set `writer.id` only if you want a custom shared label.
5. Start `memory service run` or enable the packaged service.
6. Open the TUI or web UI.

Debian:

```bash
sudo dpkg -i memory-layer_<version>_amd64.deb
memory wizard --global
cd /path/to/your-project
memory wizard
sudo systemctl enable --now memory-layer.service
memory tui
```

macOS:

```bash
brew tap 3vilM33pl3/memory https://github.com/3vilM33pl3/memory
brew install --HEAD 3vilM33pl3/memory/memory-layer
memory wizard --global
cd /path/to/your-project
memory wizard
memory service enable
memory tui
```

For the full onboarding flow, prerequisites, upgrade path, and troubleshooting, use [Getting Started](docs/user/getting-started.md).

Key docs after setup:

- [TUI Guide](docs/user/tui/README.md) for the visual workflow.
- [Embedding Operations](docs/user/cli/embeddings.md) for multi-backend semantic search and model switching.
- [Watcher Health](docs/user/cli/watchers.md) for distributed watcher behavior.
- [Query Command](docs/user/cli/query.md) for cited answers from memory.
- [Get Up To Speed](docs/user/cli/up-to-speed.md) for new-agent briefings.
- [Memory Bundles](docs/user/cli/bundles.md) for shareable backup and restore.

Most mutating `memory` commands support `--dry-run` so agents can preview writes, service actions, and plan/checkpoint flows before applying them.

## Quick Start (Developers)

If you are working on Memory Layer itself, you can run a development copy from a `cargo` checkout that is **fully isolated** from any packaged install on the same machine — separate ports, separate Cap'n Proto socket, separate runtime directory. The TUI shows `[dev]` in its header so you cannot mistake one for the other.

The mechanism: any `memory` binary launched from `target/{debug,release}/` activates the `dev` profile, which layers `.mem/config.dev.toml` on top of `.mem/config.toml` and ignores the global config entirely. Override with `MEMORY_LAYER_PROFILE=dev|prod` when needed.

```bash
git clone https://github.com/3vilM33pl3/memory
cd memory
npm --prefix web ci && npm --prefix web run build

# Bootstrap the repo-local base config and the dev overlay.
cargo run --bin memory -- init
cargo run --bin memory -- dev init --copy-from-global

# Each piece in its own shell, all on the dev stack.
cargo run --bin memory -- service run            # backend (4250 HTTP, 4251 capnp)
cargo run --bin memory -- watcher manager run    # optional
cargo run --bin memory -- tui                    # header reads [dev]
```

`--copy-from-global` lifts the database URL and LLM/embedding endpoints from the installed config into the dev overlay so credentials are not duplicated.

| Stack | HTTP | capnp TCP | capnp Unix socket |
| --- | --- | --- | --- |
| Installed (Debian/Homebrew package) | `127.0.0.1:4040` | `127.0.0.1:4041` | `/tmp/memory-layer.capnp.sock` |
| Dev (cargo-run from repo) | `127.0.0.1:4250` | `127.0.0.1:4251` | `<repo>/.mem/runtime/dev/memory-layer.capnp.sock` |

For the full isolation contract, override flags, troubleshooting, and the verification recipe, see [Dev Stack vs Installed Stack](docs/developer/dev-stack.md).

## Feature Tour

### Search That Explains Itself

The Query tab and `memory query` combine lexical search, vector search, relation boosts, and memory filters. Results are labelled as `lexical`, `semantic`, or `hybrid`, and answers cite the ranked memories that supported them.

![Memory Layer query tab](docs/img/tui/query-tab.png)

### Multiple Embedding Backends

Memory Layer can keep several embedding spaces populated at once. That means you can compare OpenAI and Voyage retrieval, migrate models safely, or keep a local OpenAI-compatible backend around without losing existing vectors.

![Memory Layer embeddings tab](docs/img/tui/embeddings-tab.png)

### Distributed Agent Awareness

The Agents and Watchers tabs show what is running now: agent sessions, project ownership, context pressure, rate limits, watcher heartbeats, restart attempts, and stale processes.

![Memory Layer watchers tab](docs/img/tui/watchers-tab.png)

### Activity And Re-Entry

The Activity and Resume views turn persisted interactions into operational history and concise re-entry briefings. This is the "get up to speed" path for a fresh agent joining an active project.

![Memory Layer activity tab](docs/img/tui/activity-tab.png)

### Durable Project Knowledge

Memory is scoped by project, typed by purpose, linked to provenance, and curated into canonical entries. The Memories and Review tabs make it possible to inspect, maintain, and approve changes to that knowledge base.

![Memory Layer memories tab](docs/img/tui/overview.png)

Project-local customization now has two layers:

- `.mem/` for runtime overrides and generated state
- `.agents/memory-layer.toml` for project-owned memory behavior such as include/ignore paths and future analyzers/plugins

## Documentation

### User Docs

- [User Documentation Index](docs/user/README.md)
- [Getting Started](docs/user/getting-started.md)
- [TUI Guide](docs/user/tui/README.md)
- [TUI Query Tab](docs/user/tui/query.md)
- [TUI Agents Tab](docs/user/tui/agents.md)
- [TUI Embeddings Tab](docs/user/tui/embeddings.md)
- [Embedding Operations](docs/user/cli/embeddings.md)
- [Memory Bundles](docs/user/cli/bundles.md)
- [Watcher Health](docs/user/cli/watchers.md)
- [Activities And Get Up To Speed](docs/user/cli/activities.md)
- [Resume Briefings](docs/user/cli/resume.md)
- [Wizard And Bootstrap](docs/user/cli/wizard.md)
- [Init Bootstrap](docs/user/cli/init.md)
- [Service Commands](docs/user/cli/service.md)
- [Doctor Diagnostics](docs/user/cli/doctor.md)
- [Health And Stats](docs/user/cli/health.md)
- [Query Command](docs/user/cli/query.md)
- [Checkpoint Workflow](docs/user/cli/checkpoint.md)
- [Capture Command](docs/user/cli/capture.md)
- [Remember Command](docs/user/cli/remember.md)
- [Curate Command](docs/user/cli/curate.md)
- [Repository Index](docs/user/cli/repo.md)
- [Scan Command](docs/user/cli/scan.md)
- [Commit History](docs/user/cli/commits.md)
- [Archive Command](docs/user/cli/archive.md)
- [Automation Commands](docs/user/cli/automation.md)

### Developer Docs

- [Developer Documentation Index](docs/developer/README.md)
- [Dev Stack vs Installed Stack](docs/developer/dev-stack.md)
- [How Skills Work](docs/developer/skills/how-skills-work.md)
- [Architecture Overview](docs/developer/architecture/overview.md)
- [How Memory Layer Works](docs/developer/architecture/how-it-works.md)
- [Hidden Memory Daemon](docs/developer/architecture/hidden-memory-daemon.md)
- [Refactor Baseline](docs/developer/refactor-baseline.md)

## License

Memory Layer is dual-licensed:

- **Open source:** GNU Affero General Public License v3.0 or later, see [LICENSE](LICENSE)
- **Commercial:** available under a separate commercial license from the copyright holder, see [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md)

### What this means

If you use, modify, or host Memory Layer under the open source license, you must comply with the AGPL, including providing source code for modified networked versions.

If you want to use Memory Layer in a proprietary or closed-source commercial setting, contact the copyright holder for a commercial license.

### Contributions

Unless explicitly agreed otherwise in writing, contributions are accepted under the repository's open source license, while the maintainer retains the right to offer the project under separate commercial terms. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Development

For working on this repository itself, start with [Quick Start (Developers)](#quick-start-developers) above and then [Dev Stack vs Installed Stack](docs/developer/dev-stack.md) for the isolation contract.

Packaging, architecture, and implementation details live under [Developer Documentation](docs/developer/README.md).
