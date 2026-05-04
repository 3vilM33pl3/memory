# Getting Started

This guide is written for someone who just wants Memory Layer working with as little setup friction as possible.

## Table of Contents

- [Prerequisites](#prerequisites)
- [PostgreSQL Requirement](#postgresql-requirement)
- [Agent Install Prompt](#agent-install-prompt)
- [Fast Install: Debian](#fast-install-debian)
- [Fast Install: macOS](#fast-install-macos)
- [What The Wizard Will Ask For](#what-the-wizard-will-ask-for)
- [File Locations](#file-locations)
- [What To Put Where](#what-to-put-where)
- [Writer ID](#writer-id)
- [Primary And Relay Services](#primary-and-relay-services)
- [Daily Use](#daily-use)
- [Optional Background Watcher](#optional-background-watcher)
- [Upgrading An Existing Install](#upgrading-an-existing-install)
- [Using `scan`](#using-scan)
- [Web UI Notes](#web-ui-notes)
- [Importing Commit History](#importing-commit-history)
- [Running From Source](#running-from-source)

## Prerequisites

Before you install or run the wizard, have these ready:

- a PostgreSQL connection string for a database Memory Layer can own
- optional: an OpenAI-compatible API key if you want `memory scan`
- PostgreSQL with `pgvector` installed and `CREATE EXTENSION vector` enabled in the Memory Layer database
- `go` on `PATH` if you plan to use the repo-local Memory Layer skills through `go run`

You do not need to invent a Memory Layer service token yourself for normal installs. Setup generates a machine-local token automatically in `memory-layer.env`, and local write-capable tools use that token to authenticate to `mem-service`.

## PostgreSQL Requirement

Memory Layer stores durable memories in PostgreSQL. The backend cannot become healthy until the database URL points at a reachable PostgreSQL database. Semantic retrieval and current embedding migrations also require pgvector.

There are two pgvector steps, and both matter:

1. Install pgvector on the PostgreSQL server.
2. Enable the `vector` extension inside the specific database Memory Layer uses.

The extension is per database. Enabling it in `postgres` does not enable it in `memory_layer`.

Example database URL:

```text
postgres://memory_layer:<password>@127.0.0.1:5432/memory_layer
```

### Existing Or Hosted PostgreSQL

Use this path when you already have PostgreSQL, including a hosted provider:

1. Confirm the provider or server supports pgvector.
2. Create a dedicated database and user, or ask your database admin for a URL.
3. From the machine that will run Memory Layer, verify connectivity:

```bash
export DATABASE_URL='postgres://memory_layer:<password>@db-host:5432/memory_layer'
psql "$DATABASE_URL" -c "SELECT 1;"
```

4. Enable pgvector in the target database:

```bash
psql "$DATABASE_URL" -c "CREATE EXTENSION IF NOT EXISTS vector;"
psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"
```

If the `CREATE EXTENSION` command fails with a permissions error, ask the database admin to run it or grant extension privileges for that database.

### Local Debian Or Ubuntu PostgreSQL

Use this when you want PostgreSQL on the same machine as Memory Layer:

```bash
sudo apt-get update
sudo apt-get install -y postgresql postgresql-contrib
pg_config --version
```

Install the pgvector package that matches your PostgreSQL server major version. For PostgreSQL 16, the package is commonly:

```bash
sudo apt-get install -y postgresql-16-pgvector
```

If your server is a different major version, replace `16` with that version. If the package is unavailable from your OS repositories, install it from the PostgreSQL Global Development Group packages or follow the upstream pgvector installation instructions.

Create a dedicated user and database:

```bash
sudo -u postgres createuser --pwprompt memory_layer
sudo -u postgres createdb --owner=memory_layer memory_layer
export DATABASE_URL='postgres://memory_layer:<password>@127.0.0.1:5432/memory_layer'
```

Enable and verify pgvector:

```bash
psql "$DATABASE_URL" -c "CREATE EXTENSION IF NOT EXISTS vector;"
psql "$DATABASE_URL" -c "SELECT 1;"
psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"
```

### Local macOS PostgreSQL

Use this when you want Homebrew PostgreSQL on the same Mac as Memory Layer:

```bash
brew install postgresql@16 pgvector
brew services start postgresql@16
```

Create a dedicated user and database:

```bash
createuser --pwprompt memory_layer
createdb --owner=memory_layer memory_layer
export DATABASE_URL='postgres://memory_layer:<password>@127.0.0.1:5432/memory_layer'
```

Enable and verify pgvector:

```bash
psql "$DATABASE_URL" -c "CREATE EXTENSION IF NOT EXISTS vector;"
psql "$DATABASE_URL" -c "SELECT 1;"
psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"
```

After Memory Layer is configured, run:

```bash
memory doctor
memory health
```

`memory doctor` should not report a missing database URL or missing pgvector before you treat the install as complete.

## Agent Install Prompt

Give this prompt to an agent when you want it to install Memory Layer for you:

````
# Install Memory Layer

You are installing Memory Layer for me. Work in the terminal, explain before using sudo, and stop before destructive changes.

## Goal

Install Memory Layer completely on this machine and configure it for the project I choose.

## Rules

- Detect whether this is Linux/Debian-style or macOS.
- Do not invent secrets.
- PostgreSQL is required. Before running `memory wizard --global`, find an existing database URL or ask me whether to use an existing/hosted PostgreSQL database or create a local one.
- If creating a local PostgreSQL database, create a dedicated database and user named `memory_layer` unless I ask for different names.
- Do not invent the database password; ask me for it or generate one only after confirming that is OK.
- Make sure the PostgreSQL server has pgvector installed and that the target database has `CREATE EXTENSION IF NOT EXISTS vector;` applied.
- Verify PostgreSQL with `psql "$DATABASE_URL" -c "SELECT 1;"` and verify pgvector with `psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"` before configuring Memory Layer.
- Ask me for optional LLM or embedding API keys only if I want scan or semantic retrieval.
- Make sure Go is available on PATH so repo-local Memory Layer skills can run.
- Run health checks before saying the install is done.

## Linux / Debian path

1. Download the latest Memory Layer `.deb` from GitHub Releases.
2. Install it with `sudo dpkg -i memory-layer_<version>_amd64.deb`.
3. Prepare PostgreSQL before configuring Memory Layer:
   - If using a hosted/existing database, verify that it accepts connections from this machine and supports pgvector.
   - If creating a local database, install PostgreSQL and the matching pgvector package for the server major version, for example `postgresql-16-pgvector` when the server is PostgreSQL 16.
   - Create or receive a database URL such as `postgres://memory_layer:<password>@127.0.0.1:5432/memory_layer`.
   - Run `psql "$DATABASE_URL" -c "CREATE EXTENSION IF NOT EXISTS vector;"`.
   - Run `psql "$DATABASE_URL" -c "SELECT 1;"` and `psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"`.
4. Run `memory wizard --global` and configure the verified database URL and optional LLM/embedding settings.
5. Go to my target project directory.
6. Run `memory wizard` for repo-local setup.
7. Start the backend with `sudo systemctl enable --now memory-layer.service`.
8. Run `memory doctor`, `memory health`, and then open `memory tui`.

## macOS path

1. Run `brew tap 3vilM33pl3/memory https://github.com/3vilM33pl3/memory`.
2. Run `brew install 3vilM33pl3/memory/memory-layer`.
3. Prepare PostgreSQL before configuring Memory Layer:
   - If using a hosted/existing database, verify that it accepts connections from this machine and supports pgvector.
   - If creating a local database, use Homebrew PostgreSQL and pgvector, then create a dedicated `memory_layer` database and user.
   - Create or receive a database URL such as `postgres://memory_layer:<password>@127.0.0.1:5432/memory_layer`.
   - Run `psql "$DATABASE_URL" -c "CREATE EXTENSION IF NOT EXISTS vector;"`.
   - Run `psql "$DATABASE_URL" -c "SELECT 1;"` and `psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"`.
4. Run `memory wizard --global` and configure the verified database URL and optional LLM/embedding settings.
5. Go to my target project directory.
6. Run `memory wizard` for repo-local setup.
7. Start the backend with `memory service enable`.
8. Run `memory doctor`, `memory health`, and then open `memory tui`.

## Finish

Report what was installed, where the config files are, whether the service is healthy, and what I should run next.
````

## Fast Install: Debian

1. Download the latest `.deb` package from the GitHub Releases page.
2. Install it:

```bash
sudo dpkg -i memory-layer_<version>_amd64.deb
```

3. Prepare PostgreSQL using the [PostgreSQL Requirement](#postgresql-requirement) section. Do not continue until these commands work:

```bash
psql "$DATABASE_URL" -c "SELECT 1;"
psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"
```

4. Configure the shared/global settings once on this machine:

```bash
memory wizard --global
```

This is where you set the shared database URL. The shared service API token is provisioned automatically if it is missing or still using the development placeholder. A writer ID is optional; if you do not set one, Memory Layer derives a stable writer identity automatically.

5. Go to the project you want to use:

```bash
cd /path/to/your-project
```

6. Run the repo-local setup wizard:

```bash
memory wizard
```

The repo-local skill bundle that `memory wizard` installs uses a shared Go helper under `.agents/skills/memory-layer/scripts/`, so agent-driven skill usage in that repository requires `go` to be available on `PATH`.

Most mutating `memory` commands also support `--dry-run`, so you can preview setup, write, indexing, bundle, and checkpoint operations before they touch local files, services, or backend state.

7. Start the backend service:

```bash
sudo systemctl enable --now memory-layer.service
```

8. Verify the setup and open the UI you prefer:

```bash
memory doctor
memory health
memory tui
```

## Fast Install: macOS

1. Tap this repository and install the formula:

```bash
brew tap 3vilM33pl3/memory https://github.com/3vilM33pl3/memory
brew install 3vilM33pl3/memory/memory-layer
```

If you specifically want the latest unreleased `main` branch:

```bash
brew install --HEAD 3vilM33pl3/memory/memory-layer
```

2. Prepare PostgreSQL using the [PostgreSQL Requirement](#postgresql-requirement) section. Do not continue until these commands work:

```bash
psql "$DATABASE_URL" -c "SELECT 1;"
psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"
```

3. Configure the shared/global settings once on this machine:

```bash
memory wizard --global
```

4. Go to the project you want to use:

```bash
cd /path/to/your-project
```

5. Run the repo-local setup wizard:

```bash
memory wizard
```

The repo-local skill bundle that `memory wizard` installs uses a shared Go helper under `.agents/skills/memory-layer/scripts/`, so agent-driven skill usage in that repository requires `go` to be available on `PATH`.

6. Start the backend LaunchAgent:

```bash
memory service enable
```

7. Verify the setup and open the TUI:

```bash
memory doctor
memory health
memory tui
```

or in a browser:

```text
http://127.0.0.1:4040/
```

## What The Wizard Will Ask For

The wizard can set up:

- shared/global settings when that scope is enabled:
  - the PostgreSQL database URL
  - the shared service API token override, if you want to replace the auto-generated one
  - an optional shared `writer.id`
- optional LLM settings for `scan`
- repo-local `.mem/` files
- optional watcher setup
- the repo-local memory skill bundle, which uses a shared Go helper under `.agents/skills/memory-layer/scripts/`

Important detail:

- inside a repository, `memory wizard` is local-first by default
- use `memory wizard --global` when you want to edit the shared/global config
- or enable `shared/global setup` in the first wizard step

## File Locations

### Shared configuration

Debian install:

- `/etc/memory-layer/memory-layer.toml`
- `/etc/memory-layer/memory-layer.env`

macOS install:

- `~/Library/Application Support/memory-layer/memory-layer.toml`
- `~/Library/Application Support/memory-layer/memory-layer.env`

Local install:

- `~/.config/memory-layer/memory-layer.toml`
- `~/.config/memory-layer/memory-layer.env`

### Per-project configuration

Inside each project:

- `.mem/config.toml`
- `.mem/project.toml`
- `.mem/memory-layer.env`
- `.mem/runtime/`
- `.agents/memory-layer.toml`

## What To Put Where

### Shared/global config

Use this for values shared by many repos:

- `database.url`
- `service.api_token`
- `[cluster]` settings for backend relay discovery on a local network
- `[llm]` settings
- `[[embeddings.backends]]` — one block per embedding backend you want available (OpenAI, Voyage, Cohere, Gemini, Ollama). See [Embedding Operations](cli/embeddings.md#configuring-multiple-backends) for the full shape and the "enable two backends from day one" workflow.

The shared service API token normally lives in the adjacent `memory-layer.env` file and is provisioned automatically during setup.

### Repo-local config

Use this for project-specific overrides:

- watcher settings
- local backend ports
- project-specific DB override if needed
- repo-specific `writer.id` override if one project should write under a different custom writer identity

### Project memory behavior

Use `.agents/memory-layer.toml` for project-owned behavior that should be easy to adapt without digging through service config:

- include and ignore path hints for repository scans
- enabled analyzers
- curation replacement policy for memory updates
- future graph and plugin controls

Example:

```toml
[curation]
replacement_policy = "balanced"
```

Available policies are `conservative`, `balanced`, and `aggressive`. `balanced` is the default.

### Env files

Use these for secrets such as:

```bash
MEMORY_LAYER__SERVICE__API_TOKEN=auto-generated-or-manually-overridden
OPENAI_API_KEY=your-api-key-here
VOYAGE_API_KEY=your-voyage-key-here
```

When you declare multiple `[[embeddings.backends]]` blocks, each one's `api_key_env` field names the variable the service will look up here. The name is arbitrary — whatever you put in `api_key_env` in the TOML, put the same key in `memory-layer.env`.

For local Ollama, use the first-class `ollama` provider and leave `api_key_env`
empty unless you are running behind a proxy that requires auth:

```toml
[llm]
provider = "ollama"
base_url = "http://127.0.0.1:11434/v1"
api_key_env = ""
model = "llama3.2"

[[embeddings.backends]]
name = "ollama-nomic"
provider = "ollama"
base_url = "http://127.0.0.1:11434/v1"
api_key_env = ""
model = "nomic-embed-text"
```

Run `ollama serve` and pull the models first, for example `ollama pull
llama3.2` and `ollama pull nomic-embed-text`. `memory doctor` checks the
local `/v1/models` endpoint and warns when the configured LLM model is missing.

## Writer ID

Each coding agent or tool that writes memory gets a writer ID.

If you do nothing, Memory Layer derives one automatically from:

- the writing tool
- the local user
- the local host name

That gives stable defaults such as:

- `memory-olivier-monolith`
- `memory-watcher-olivier-monolith`

For most setups, that automatic writer identity is enough.

You can configure it in TOML:

```toml
[writer]
id = "codex-cli-main"
name = "Codex CLI"
```

or with an environment variable:

```bash
export MEMORY_LAYER_WRITER_ID=codex-cli-main
```

Use an explicit writer ID only when you want a custom stable label shared across tools or machines.

## Primary And Relay Services

If a machine can reach PostgreSQL, `mem-service` runs as a `primary`.

If a machine cannot reach PostgreSQL but can see another Memory Layer service on the local network, `mem-service` can run as a `relay`. In relay mode it discovers a primary over UDP multicast and forwards the normal HTTP API and browser WebSocket traffic to it.

Relay discovery is controlled from shared config:

```toml
[cluster]
enabled = true
```

The wizard exposes this as a shared setup option, and `memory service enable` can offer to turn it on after a database-connect failure.

## Daily Use

Open the TUI:

```bash
memory tui
```

For a visual walkthrough of each tab, use the [TUI Guide](tui/README.md).

Open the web UI:

```text
http://127.0.0.1:4040/
```

Check health:

```bash
memory service status
memory health
memory doctor
```

Save a useful project fact:

```bash
memory remember --project my-project --note "Deployment uses a systemd service."
```

Search project memory:

```bash
memory query --project my-project --question "How is deployment handled here?"
```

Export a shareable memory bundle:

```bash
memory bundle export --project my-project --out my-project.mlbundle.zip
```

For semantic-search maintenance commands such as `memory embeddings reindex`, `memory embeddings reembed`, and `memory embeddings prune`, see [Embedding Operations](cli/embeddings.md).
For project memory backup and restore, see [Memory Bundles](cli/bundles.md).
For watcher health states, restart behavior, and recovery signals in the TUI, see [Watcher Health](cli/watchers.md).
For the direct write command, see [Remember Command](cli/remember.md).
For service management and setup diagnostics, see [Service Commands](cli/service.md) and [Doctor Diagnostics](cli/doctor.md).
For bootstrap behavior, see [Wizard And Bootstrap](cli/wizard.md).

For getting back into flow after an interruption, see [Resume Briefings](cli/resume.md).

## Optional Background Watcher

If you want Memory Layer to capture useful work in the background:

```bash
memory watcher enable --project my-project
```

When the backend service restarts, service-managed watchers will restart too so they reconnect cleanly to the new backend instance.

Check it:

```bash
memory watcher status --project my-project
```

In the TUI:

- the `Watchers` tab shows each watcher's health, restart attempts, and last heartbeat
- the `Project` tab's recent activity section shows watcher-health transitions such as `stale`, `restarting`, `failed`, and recovery back to `healthy`
- recovery events now show what state the watcher recovered from and, when relevant, how many restart attempts happened before recovery

Disable it later:

```bash
memory watcher disable --project my-project
```

## Upgrading An Existing Install

If you already use Memory Layer and are upgrading to a newer release:

1. install the new `.deb`
2. make sure pgvector is installed on the PostgreSQL server for your server version
3. enable the extension in the specific database named by `database.url`:

```bash
psql "$DATABASE_URL" -c "CREATE EXTENSION IF NOT EXISTS vector;"
psql "$DATABASE_URL" -c "SELECT extversion FROM pg_extension WHERE extname = 'vector';"
```

4. restart the backend service:

```bash
sudo systemctl restart memory-layer.service
```

5. verify the setup:

```bash
memory doctor
```

6. rebuild embeddings for existing project memories:

```bash
memory embeddings reindex --project my-project
```

If you later switch the embedding model, Memory Layer keeps the old embedding space instead of overwriting it. Use:

```bash
memory embeddings reembed --project my-project
```

to materialize vectors for the newly active space, and:

```bash
memory embeddings prune --project my-project
```

only when you want to delete non-active embedding spaces explicitly.

For the command-level explanation of when to use each of those operations, see [Embedding Operations](cli/embeddings.md).

If `memory doctor` reports that `pgvector` is missing, install the PostgreSQL server package first, enable `vector` in the Memory Layer database, and rerun the check.

On Debian, upgrades should preserve local edits to:

- `/etc/memory-layer/memory-layer.env`
- `/etc/memory-layer/memory-layer.toml`

Those files are treated as package-managed configuration files rather than being overwritten
with package defaults on every upgrade.

## Using `scan`

`scan` reads a repository, sends a structured summary to the configured LLM, and writes useful durable memories back into Memory Layer.

Full command documentation:

- [Memory Bundles](cli/bundles.md)
- [Scan Command](cli/scan.md)

Try it safely first:

```bash
memory scan --project my-project --dry-run
```

Then write the results:

```bash
memory scan --project my-project
```

If `scan` fails, the two most common causes are:

- missing `[llm].model` in config
- missing `OPENAI_API_KEY`

## Web UI Notes

The browser UI is served by `mem-service` itself. In a normal install it should work automatically once the service is running.

If you build from source, build the frontend first:

```bash
npm --prefix web ci
npm --prefix web run build
```

Then start the backend:

```bash
cargo run --bin memory -- service run
```

## Importing Commit History

Memory Layer can also store git commits as project evidence without turning every commit into canonical memory.

Import recent or full history:

```bash
memory commits sync --project my-project
```

Browse imported commits:

```bash
memory commits list --project my-project
memory commits show --project my-project <commit-hash>
```

If `memory doctor` reports that no commit history has been imported yet, the fix is:

```bash
memory commits sync --project my-project
```

## Running From Source

If you are developing Memory Layer itself, a `cargo` checkout runs as a **dev** stack that is fully isolated from any packaged install on the same machine — separate ports (`4250`/`4251` instead of `4040`/`4041`), separate Cap'n Proto socket, and a separate runtime directory. The TUI shows `[dev]` in its header.

```bash
cargo run --bin memory -- init
cargo run --bin memory -- dev init --copy-from-global
cargo run --bin memory -- service run            # in its own shell
cargo run --bin memory -- tui                    # header reads [dev]
```

Optional watcher manager:

```bash
cargo run --bin memory -- watcher manager run
```

`memory dev init` without `--copy-from-global` leaves the overlay without shared credentials — fine if you want the dev stack on a different database or LLM endpoint, otherwise rerun with the flag or copy `[database]`, `[llm]`, `[embeddings]` into `.mem/config.dev.toml` by hand.

The full isolation contract, default endpoint table, and troubleshooting live in [Dev Stack vs Installed Stack](../developer/dev-stack.md).

## Related Docs

- [User Documentation](README.md)
- [Memory Bundles](cli/bundles.md)
- [Scan Command](cli/scan.md)
- [Commit History](cli/commits.md)
- [Developer Documentation](../developer/README.md)
