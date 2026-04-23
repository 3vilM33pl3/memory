# Embedding Operations

This page explains the commands used to build, refresh, swap, and clean up semantic search embeddings in Memory Layer, and how to configure multiple embedding backends in parallel.

## Table of Contents

- [When You Need This](#when-you-need-this)
- [How Embeddings Work](#how-embeddings-work)
- [Configuring Multiple Backends](#configuring-multiple-backends)
- [Commands](#commands)
- [Typical Workflows](#typical-workflows)
- [Troubleshooting](#troubleshooting)

## When You Need This

Use these commands when:

- you enabled embeddings for the first time
- you want vector search for existing memories
- you changed the embedding model, or want to keep two models populated at once
- you want to switch which backend search uses without recomputing
- you want to clean up old embedding spaces after a model switch

## How Embeddings Work

Memory Layer stores chunk embeddings in PostgreSQL with `pgvector`.

Every embedding row is keyed by a **space key** of the form `provider|base_url|model`, so vectors from different providers and models coexist without collision. A single chunk can have vectors in several spaces at once.

At any time, exactly one configured backend is **active**: that's the one `memory query` uses for semantic retrieval. Swapping activation is a constant-time metadata flip — no recomputation — as long as the target space is already populated.

**Every new memory is embedded into every configured backend automatically.** The curate step that runs after `memory remember` (and after the watcher's idle captures) writes chunks into every space declared under `[[embeddings.backends]]`, not just the active one. So configuring two backends from the start means you never have to run `reembed` later just to switch. `reembed` and `reindex` are only for *backfilling* existing memories when you add a backend after the fact.

Heads up on cost: with two backends configured, each new memory hits both providers' embedding APIs. That's usually negligible for text-embedding-3-small and voyage-code-3 (both in the low cents per thousand writes), but worth keeping in mind if you add a premium model.

## Configuring Multiple Backends

Declare every backend you want available under `[[embeddings.backends]]` and pick one with `[embeddings].active`:

```toml
[embeddings]
active = "voyage-code"

[[embeddings.backends]]
name = "openai-3-small"
provider = "openai_compatible"
base_url = ""
api_key_env = "OPENAI_API_KEY"
model = "text-embedding-3-small"
batch_size = 16

[[embeddings.backends]]
name = "voyage-code"
provider = "voyage"
base_url = ""
api_key_env = "VOYAGE_API_KEY"
model = "voyage-code-3"
batch_size = 16
```

The `name` field is your activation handle and must be unique. If you leave `name` empty, Memory Layer derives one from `{provider}-{model}` at load time.

The **legacy singleton shape** still works:

```toml
[embeddings]
provider = "voyage"
model = "voyage-code-3"
api_key_env = "VOYAGE_API_KEY"
```

Internally this is normalized to a one-element `backends` list with an auto-derived name, so `memory embeddings list` will show the same information.

`base_url = ""` falls back to the provider's well-known endpoint (`https://api.openai.com/v1`, `https://api.voyageai.com`, etc.).

## Commands

List configured backends and show which is active:

```bash
memory embeddings list
```

Output marks the active backend with `*` and any backend that didn't resolve at startup (missing API key, empty model) with `!`.

The same information, plus per-project chunk and memory counts, is available interactively in the [Embeddings Tab](../tui/embeddings.md) of the TUI.

Switch which backend search uses:

```bash
memory embeddings activate voyage-code
```

The service rewrites `[embeddings].active` in the config file and updates its in-memory state without restarting. Existing embeddings for the new space are used immediately; nothing is recomputed.

Build chunks and embeddings for a project:

```bash
memory embeddings reindex --project my-project
```

By default this populates **every** configured backend so all spaces stay in sync. Restrict to one backend with `--backend`:

```bash
memory embeddings reindex --project my-project --backend voyage-code
```

Preview without writing:

```bash
memory embeddings reindex --project my-project --dry-run
```

Refresh only the embeddings of configured backends for a project (does not rebuild chunks):

```bash
memory embeddings reembed --project my-project
memory embeddings reembed --project my-project --backend voyage-code
memory embeddings reembed --project my-project --dry-run
```

Use `reembed` when:

- you added a new backend to config and want to populate its space
- an existing backend's space is only partially covered
- you prefer not to do the full `reindex` chunk rebuild

Delete embedding rows whose space isn't in any configured backend:

```bash
memory embeddings prune --project my-project
memory embeddings prune --project my-project --dry-run
```

`prune` operates relative to the **set** of currently configured backends (not just the active one), so removing a backend from config before pruning is the right order when you want to retire a model completely.

## Typical Workflows

Your config files live in the locations listed under [Getting Started → File Locations](../getting-started.md#file-locations) — on Debian that's `/etc/memory-layer/memory-layer.toml` and `/etc/memory-layer/memory-layer.env`; on Linux user-level installs it's `~/.config/memory-layer/memory-layer.toml` and `~/.config/memory-layer/memory-layer.env`.

### Enable embeddings for the first time (single backend)

1. Add your API key to `memory-layer.env`:
   ```dotenv
   OPENAI_API_KEY=sk-proj-...
   ```
2. Add an `[embeddings]` block to `memory-layer.toml`:
   ```toml
   [embeddings]
   provider = "openai_compatible"
   base_url = ""
   api_key_env = "OPENAI_API_KEY"
   model = "text-embedding-3-small"
   batch_size = 16
   ```
3. Restart the service. Confirm the setup:
   ```bash
   memory doctor
   memory embeddings list          # should show one backend, no "!"
   ```
4. Backfill embeddings for existing memories:
   ```bash
   memory embeddings reindex --project my-project
   ```

### Enable two backends from day one

Do this if you know up-front you want the option to switch models freely — it avoids having to reembed the whole corpus later.

1. Put both API keys in `memory-layer.env`:
   ```dotenv
   OPENAI_API_KEY=sk-proj-...
   VOYAGE_API_KEY=pa-...
   ```
2. Declare both backends in `memory-layer.toml` and pick one with `active`:
   ```toml
   [embeddings]
   active = "voyage-code"

   [[embeddings.backends]]
   name = "openai-3-small"
   provider = "openai_compatible"
   base_url = ""
   api_key_env = "OPENAI_API_KEY"
   model = "text-embedding-3-small"
   batch_size = 16

   [[embeddings.backends]]
   name = "voyage-code"
   provider = "voyage"
   base_url = ""
   api_key_env = "VOYAGE_API_KEY"
   model = "voyage-code-3"
   batch_size = 16
   ```
3. Restart the service. Confirm both resolved:
   ```bash
   memory embeddings list          # both listed; active marked with *; neither marked with !
   ```
4. Backfill every existing memory into both spaces (default behavior — no `--backend` flag):
   ```bash
   memory embeddings reindex --project my-project
   ```
5. From here on, every new memory is embedded into both spaces automatically. Swap which one search uses any time:
   ```bash
   memory embeddings activate openai-3-small
   memory embeddings activate voyage-code
   ```

### Add a second backend to an existing install

Same end state as the two-backend workflow, just applied incrementally:

1. Add the new `[[embeddings.backends]]` block and its API key line in `memory-layer.env`.
2. Restart the service. `memory embeddings list` should show both.
3. Backfill existing memories into the new space:
   ```bash
   memory embeddings reembed --project my-project
   ```
4. Switch search to the new backend whenever you're ready:
   ```bash
   memory embeddings activate <new-backend-name>
   ```

### Retire a backend

1. Remove its `[[embeddings.backends]]` block from config. Restart the service.
2. `memory embeddings prune --project my-project` drops the orphaned space.

## Troubleshooting

If semantic search is not working:

- run `memory doctor`
- confirm `pgvector` is installed and the `vector` extension exists in the target database
- confirm at least one `[[embeddings.backends]]` entry has a non-empty `model`
- confirm the API key env var referenced by `api_key_env` is present in `memory-layer.env`
- `memory embeddings list` — the active backend should not be marked with `!`

If the active space's vectors are missing for some memories (semantic search returns fewer results than lexical), run:

```bash
memory embeddings reembed --project my-project --backend <active-name>
```

If a newly-added backend is marked `!` even after a restart, check that the referenced API key env var is populated in `memory-layer.env` and that `model` is non-empty.

## Related Docs

- [Getting Started](../getting-started.md)
- [Scan Command](scan.md)
- [Embeddings and Search](../../developer/architecture/embeddings-and-search.md)
- [How Memory Layer Works](../../developer/architecture/how-it-works.md)
