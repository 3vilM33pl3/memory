# Memory Layer

Memory Layer is a local knowledge base for coding agents such as Codex CLI and Claude Code.

It captures durable project knowledge, stores it in PostgreSQL, and makes it searchable in a TUI or browser so important context does not disappear into chat history, terminal scrollback, or old commits. 

It uses an army of (distributed) watchers to track projects and assist the LMMs and developers working on it. 

![Memory Layer TUI](docs/img/tui-overview.png)

## Table of Contents

- [Quick Start](#quick-start)
- [What It Does](#what-it-does)
- [Documentation](#documentation)
- [Development](#development)

## Quick Start

The fastest path is:

1. Install the package.
2. Run `mem-cli wizard --global` once per machine.
3. Run `mem-cli wizard` inside each repository.
4. Start `mem-service`.
5. Open the TUI or web UI.

Debian:

```bash
sudo dpkg -i memory-layer_<version>_amd64.deb
mem-cli wizard --global
cd /path/to/your-project
mem-cli wizard
sudo systemctl enable --now memory-layer.service
mem-cli tui
```

macOS:

```bash
brew install --HEAD ./packaging/macos/homebrew/memory-layer.rb
mem-cli wizard --global
cd /path/to/your-project
mem-cli wizard
mem-cli service enable
mem-cli tui
```

For the full onboarding flow, prerequisites, upgrade path, and troubleshooting, use [Getting Started](docs/user/getting-started.md).

## What It Does

- stores project memory in PostgreSQL
- supports both primary and relay service modes
- keeps memory scoped per project
- captures raw evidence and curates durable memory from it
- combines lexical search with optional semantic recall and related-memory links
- provides a TUI and a browser UI
- can scan a repository for durable project knowledge
- can import git commit history as searchable evidence

Project-local customization now has two layers:

- `.mem/` for runtime overrides and generated state
- `.agents/memory-layer.toml` for project-owned memory behavior such as include/ignore paths and future analyzers/plugins

## Documentation

### User Docs

- [User Documentation Index](docs/user/README.md)
- [Getting Started](docs/user/getting-started.md)
- [Scan Command](docs/user/cli/scan.md)
- [Commit History](docs/user/cli/commits.md)

### Developer Docs

- [Developer Documentation Index](docs/developer/README.md)
- [Architecture Overview](docs/developer/architecture/overview.md)
- [How Memory Layer Works](docs/developer/architecture/how-it-works.md)
- [Hidden Memory Daemon](docs/developer/architecture/hidden-memory-daemon.md)

## Development

For working on this repository itself, start with the developer docs. The short version is:

```bash
cargo run --bin mem-cli -- wizard
cargo run --bin mem-service
cargo run --bin mem-cli -- tui --project memory
```

Optional watcher:

```bash
cargo run --bin memory-watch -- run --project memory
```

Packaging and implementation details now live under [Developer Documentation](docs/developer/README.md).
