# Memory Layer

Memory Layer is a local knowledge base built first for coding agents such as Codex CLI and Claude Code, while still working well for normal developers.

It captures durable project knowledge, stores it in PostgreSQL with pgvector, and makes it searchable in a TUI or browser so important context does not disappear into chat history, terminal scrollback, or old commits.

It supports multiple developers, multiple projects, and multiple coding agents at the same time through a distributed watcher system and a shared memory backend.

![Memory Layer TUI](docs/img/tui/overview.png)

## Table of Contents

- [Quick Start](#quick-start)
- [What It Does](#what-it-does)
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

For semantic-search maintenance and model switching, use [Embedding Operations](docs/user/cli/embeddings.md).

For shareable backup/restore bundles, use [Memory Bundles](docs/user/cli/bundles.md).

For watcher health states, recovery behavior, and the TUI watcher views, use [Watcher Health](docs/user/cli/watchers.md).

For bootstrap, diagnostics, and the main write path, use [Wizard And Bootstrap](docs/user/cli/wizard.md), [Service Commands](docs/user/cli/service.md), [Doctor Diagnostics](docs/user/cli/doctor.md), and [Remember Command](docs/user/cli/remember.md).

Most mutating `memory` commands support `--dry-run` so you can preview writes, service actions, and plan/checkpoint flows before applying them.

For a visual walkthrough of the interface, use the [TUI Guide](docs/user/tui/README.md).

## What It Does

- stores project memory in PostgreSQL with pgvector-backed chunk embeddings
- supports both primary and relay service modes
- keeps memory scoped per project while supporting multiple developers, writers, and agents
- captures raw evidence and curates durable memory from it
- combines lexical search, vector search, and related-memory links
- supports re-embedding when you switch embedding models without losing older embedding spaces
- uses distributed watchers to track active projects and feed evidence into the shared memory system
- provides a TUI and a browser UI
- can scan a repository for durable project knowledge
- can import git commit history as searchable evidence
- can export and import shareable project memory bundles

Project-local customization now has two layers:

- `.mem/` for runtime overrides and generated state
- `.agents/memory-layer.toml` for project-owned memory behavior such as include/ignore paths and future analyzers/plugins

## Documentation

### User Docs

- [User Documentation Index](docs/user/README.md)
- [Getting Started](docs/user/getting-started.md)
- [TUI Guide](docs/user/tui/README.md)
- [Embedding Operations](docs/user/cli/embeddings.md)
- [Memory Bundles](docs/user/cli/bundles.md)
- [Watcher Health](docs/user/cli/watchers.md)
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
- [How Skills Work](docs/developer/skills/how-skills-work.md)
- [Architecture Overview](docs/developer/architecture/overview.md)
- [How Memory Layer Works](docs/developer/architecture/how-it-works.md)
- [Hidden Memory Daemon](docs/developer/architecture/hidden-memory-daemon.md)

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

For working on this repository itself, start with the developer docs. The short version is:

```bash
cargo run --bin memory -- wizard
cargo run --bin memory -- service run
cargo run --bin memory -- tui --project memory
```

Optional watcher:

```bash
cargo run --bin memory -- watcher run --project memory
```

Packaging and implementation details now live under [Developer Documentation](docs/developer/README.md).
