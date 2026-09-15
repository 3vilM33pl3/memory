# Memory Layer

[![CI](https://github.com/3vilM33pl3/memory/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/3vilM33pl3/memory/actions/workflows/ci.yml?query=branch%3Amain) [![Nightly](https://github.com/3vilM33pl3/memory/actions/workflows/nightly.yml/badge.svg?branch=main&event=schedule)](https://github.com/3vilM33pl3/memory/actions/workflows/nightly.yml?query=branch%3Amain) [![Latest release](https://img.shields.io/github/v/release/3vilM33pl3/memory?display_name=tag&sort=semver&style=flat)](https://github.com/3vilM33pl3/memory/releases/latest) [![Release date](https://img.shields.io/github/release-date/3vilM33pl3/memory?label=release%20date&style=flat)](https://github.com/3vilM33pl3/memory/releases/latest) [![Downloads](https://img.shields.io/github/downloads/3vilM33pl3/memory/total?label=downloads&style=flat)](https://github.com/3vilM33pl3/memory/releases) [![Docs](https://img.shields.io/website?url=https%3A%2F%2Fwww.memory-layer.dev%2Fdocs&label=docs&style=flat)](https://www.memory-layer.dev/docs)

[![License](https://img.shields.io/badge/license-AGPL--3.0--or--later%20%2F%20commercial-2563eb?style=flat)](#license) [![Rust 2024](https://img.shields.io/badge/Rust-2024-000000?logo=rust&logoColor=white&style=flat)](Cargo.toml) [![Linux](https://img.shields.io/badge/platform-Linux-333333?logo=linux&logoColor=white&style=flat)](https://github.com/3vilM33pl3/memory/releases/latest) [![macOS](https://img.shields.io/badge/platform-macOS-333333?logo=apple&logoColor=white&style=flat)](https://github.com/3vilM33pl3/memory/releases/latest) [![Windows](https://img.shields.io/badge/platform-Windows-0078D4?logo=windows&logoColor=white&style=flat)](https://github.com/3vilM33pl3/memory/releases/latest) [![Stars](https://img.shields.io/github/stars/3vilM33pl3/memory?label=stars&style=flat)](https://github.com/3vilM33pl3/memory) [![Forks](https://img.shields.io/github/forks/3vilM33pl3/memory?label=forks&style=flat)](https://github.com/3vilM33pl3/memory/forks) [![Open issues](https://img.shields.io/github/issues/3vilM33pl3/memory?label=open%20issues&style=flat)](https://github.com/3vilM33pl3/memory/issues) [![Open PRs](https://img.shields.io/github/issues-pr/3vilM33pl3/memory?label=open%20PRs&style=flat)](https://github.com/3vilM33pl3/memory/pulls) [![Last commit](https://img.shields.io/github/last-commit/3vilM33pl3/memory/main?label=last%20commit&style=flat)](https://github.com/3vilM33pl3/memory/commits/main) [![Citation](https://img.shields.io/badge/citation-CITATION.cff-2563eb?style=flat)](CITATION.cff)

Memory Layer is a local-first memory system for coding agents and developers.
It turns project work into durable, searchable knowledge, so the next Codex,
Claude, or human session can start with evidence instead of guesswork.

It captures what happened, curates what matters, stores it in PostgreSQL with
pgvector, and exposes it through a TUI, browser UI, and agent-friendly CLI.

[Website](https://www.memory-layer.dev) · [Documentation](https://www.memory-layer.dev/docs) · [v2.0.0](https://github.com/3vilM33pl3/memory/releases/tag/v2.0.0)

![Memory Layer memories tab](docs/img/tui/memories-tab.png)

## Start here

### Try it with Docker

The reproducible demo needs only Docker; it starts PostgreSQL with pgvector,
the service, and the web UI. Clone the repository and start the stack:

```bash
git clone https://github.com/3vilM33pl3/memory
cd memory
docker compose up
```

In a second terminal, load the demo project and ask one question:

```bash
docker compose exec memory memory demo
docker compose exec memory memory query --project demo --question "How does reinforcement work?"
```

Open `http://localhost:4040` for the browser UI, or run
`docker compose exec memory memory tui` for the terminal UI. Follow the
[Quickstart](https://www.memory-layer.dev/docs/quickstart) for the complete
demo path.

### Use it in a project

For a native installation, prerequisites, and verification, start with the
[Install guide](https://www.memory-layer.dev/docs/install) or download the
[v2.0.0 release](https://github.com/3vilM33pl3/memory/releases/tag/v2.0.0).
Version 2 is a breaking upgrade from v1; existing users should follow the
[Update guide](https://www.memory-layer.dev/docs/install/update) before
restarting the service.
The setup wizard configures the machine once and the project you are working
in:

```bash
memory wizard --global
cd /path/to/your-project
memory wizard --dry-run
memory wizard
memory doctor
```

Native installations need a PostgreSQL database with pgvector; the install
guide covers local, hosted, Windows, and package-specific paths. The Windows
x86_64 MSI is per-user, installs under
`%LOCALAPPDATA%\Programs\Memory Layer`, and adds its `bin` directory to the
user `PATH`.

### Work with it every day

Capture finished work, query before making a change, resume after an
interruption, and inspect or recover when something looks wrong. The
[Daily workflow](https://www.memory-layer.dev/docs/daily-workflow) gives the
commands and the [TUI](https://www.memory-layer.dev/docs/tui) and
[Web UI](https://www.memory-layer.dev/docs/web-ui) make the stored evidence
easy to inspect.

### Connect agents

- [Codex Desktop plugin](https://www.memory-layer.dev/docs/codex-plugin) — the
  supported MCP connection and desktop workflow skill.
- [Agents](https://www.memory-layer.dev/docs/agents) — project setup and
  agent-facing workflows.
- [MCP](https://www.memory-layer.dev/docs/mcp) — read-only project-memory
  tools over stdio or local Streamable HTTP.

Use one Memory Layer MCP connection per client. The Codex plugin guide
explains how to avoid duplicate tool registration and how to verify the active
project.

## What it provides

- Cited answers from lexical, semantic, relation, and code-graph retrieval.
- Project memories with provenance, curation, review proposals, and durable
  re-entry briefings.
- Local TUI and browser UI for memories, activity, review, watchers, and
  runtime health.
- Coding-agent integration through the CLI, repo-local skills, and MCP.
- Repeatable evaluation with paired ablations, immutable artifacts, gates, and
  cost/latency reporting.

## Evidence and evaluation

The newest checked-in local reference is the
[2026-07-06 `memory-quality-v1` re-baseline](docs/developer/evaluation-runs/2026-07-06-memory-quality-v1-comparison.md):
`0.692` (18/26), with retrieval at 10/10 and grounded answers at 8/9. Its gate
is still red because the adversarial-stale floor is intentionally unmet; treat
it as a precise engineering reference, not a general release claim.

The separately recorded
[2026-05-03 Docker `memory-improvement-v1` benchmark](docs/developer/evaluation-runs/2026-05-03-memory-improvement-v1-full.md)
ran five paired repeats against a different suite. It reported full-memory
aggregate success from 0.0% to 18.1%, perfect retrieval ranking metrics, and a
41.2% reduction in total tokens. Read it as historical Docker-harness evidence
rather than a directly comparable successor to the July local run.

For methodology and commands, see the
[evaluation guide](https://www.memory-layer.dev/docs/evals) and
[CLI reference](https://www.memory-layer.dev/docs/reference/cli/eval).

## Documentation

The public site is organised with an essentials-first path and optional deep
dives:

- [Quickstart](https://www.memory-layer.dev/docs/quickstart)
- [Install](https://www.memory-layer.dev/docs/install)
- [Daily workflow](https://www.memory-layer.dev/docs/daily-workflow)
- [Help](https://www.memory-layer.dev/docs/help)
- [Operations](https://www.memory-layer.dev/docs/operations)
- [How it works](https://www.memory-layer.dev/docs/how-it-works)
- [CLI reference](https://www.memory-layer.dev/docs/reference/cli)

The [`docs-site/`](docs-site/README.md) directory contains the public site;
the [`docs/`](docs/developer/README.md) tree contains the detailed in-repository
manual and developer reference.

## Development and contributing

Start with [Contributing](CONTRIBUTING.md), the
[developer documentation](docs/developer/README.md), and the
[dev-stack guide](docs/developer/dev-stack.md). The development stack is
isolated from packaged installations; its setup and verification steps belong
in that guide rather than this README.

## License

Memory Layer is dual-licensed:

- **Open source:** GNU Affero General Public License v3.0 or later; see
  [LICENSE](LICENSE).
- **Commercial:** available under a separate commercial license; see
  [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md).

Contributions are accepted under the repository's open-source license unless
explicitly agreed otherwise in writing. See [Contributing](CONTRIBUTING.md).
