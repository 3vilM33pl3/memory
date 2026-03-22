# Getting Started

This guide is written for someone who just wants Memory Layer working with as little setup friction as possible.

## Easiest Path: Debian Package

1. Download the latest `.deb` package from the GitHub Releases page.
2. Install it:

```bash
sudo dpkg -i memory-layer_<version>_amd64.deb
```

3. Go to the project you want to use:

```bash
cd /path/to/your-project
```

4. Run the setup wizard:

```bash
mem-cli wizard
```

5. Start the backend service:

```bash
sudo systemctl enable --now memory-layer.service
```

6. Open the TUI:

```bash
mem-cli tui
```

## What The Wizard Will Ask For

The wizard can set up:

- the PostgreSQL database URL
- the write API token
- optional LLM settings for `scan`
- repo-local `.mem/` files
- optional watcher setup

## The Few Things You Need

Before you run the wizard, it helps to have:

- a PostgreSQL connection string
- an API token string you want the local tools to use
- optional: an OpenAI-compatible API key if you want `mem-cli scan`

## File Locations

### Shared configuration

Debian install:

- `/etc/memory-layer/memory-layer.toml`
- `/etc/memory-layer/memory-layer.env`

Local install:

- `~/.config/memory-layer/memory-layer.toml`
- `~/.config/memory-layer/memory-layer.env`

### Per-project configuration

Inside each project:

- `.mem/config.toml`
- `.mem/project.toml`
- `.mem/memory-layer.env`
- `.mem/runtime/`

## What To Put Where

### Shared/global config

Use this for values shared by many repos:

- `database.url`
- `service.api_token`
- `[llm]` settings

### Repo-local config

Use this for project-specific overrides:

- watcher settings
- local backend ports
- project-specific DB override if needed

### Env files

Use these for secrets such as:

```bash
OPENAI_API_KEY=your-api-key-here
```

## Daily Use

Open the TUI:

```bash
mem-cli tui
```

Check health:

```bash
mem-cli health
mem-cli doctor
```

Save a useful project fact:

```bash
mem-cli remember --project my-project --note "Deployment uses a systemd service."
```

Search project memory:

```bash
mem-cli query --project my-project --question "How is deployment handled here?"
```

## Optional Background Watcher

If you want Memory Layer to capture useful work in the background:

```bash
mem-cli watch enable --project my-project
```

Check it:

```bash
mem-cli watch status --project my-project
```

Disable it later:

```bash
mem-cli watch disable --project my-project
```

## Using `scan`

`scan` reads a repository, sends a structured summary to the configured LLM, and writes useful durable memories back into Memory Layer.

Full command documentation:

- [Scan Command](cli/scan.md)

Try it safely first:

```bash
mem-cli scan --project my-project --dry-run
```

Then write the results:

```bash
mem-cli scan --project my-project
```

If `scan` fails, the two most common causes are:

- missing `[llm].model` in config
- missing `OPENAI_API_KEY`

## Running From Source

If you are developing Memory Layer itself:

```bash
cargo run --bin mem-cli -- wizard
cargo run --bin mem-service
cargo run --bin mem-cli -- tui --project memory
```

Optional watcher:

```bash
cargo run --bin memory-watch -- run --project memory
```
