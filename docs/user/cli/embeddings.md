# Embedding Operations

This page explains the three commands used to build, refresh, and clean up semantic search embeddings in Memory Layer.

## Table of Contents

- [When You Need This](#when-you-need-this)
- [How Embeddings Work](#how-embeddings-work)
- [Commands](#commands)
- [Typical Workflows](#typical-workflows)
- [Troubleshooting](#troubleshooting)

## When You Need This

Use these commands when:

- you enabled `[embeddings]` for the first time
- you want vector search for existing memories
- you changed the embedding model
- you want to clean up old embedding spaces after a model switch

## How Embeddings Work

Memory Layer stores chunk embeddings in PostgreSQL with `pgvector`.

The system now supports multiple embedding spaces side by side. That means:

- switching models does not overwrite older vectors
- semantic search uses the currently active embedding space from `[embeddings]`
- old spaces stay available until you explicitly remove them

In practice:

- `reindex` rebuilds chunks and materializes embeddings for the active space
- `reembed` materializes or refreshes embeddings for the active space only
- `prune-embeddings` deletes non-active spaces for a project

## Commands

Build chunks and embeddings for a project:

```bash
mem-cli reindex --project my-project
```

Use this when:

- embeddings were just enabled
- you want full coverage for existing memories
- chunk structure may have changed

Refresh only the active embedding space:

```bash
mem-cli reembed --project my-project
```

Use this when:

- you changed the embedding model
- you changed provider or base URL for embeddings
- you want the new active space without doing a full chunk rebuild

Delete non-active embedding spaces:

```bash
mem-cli prune-embeddings --project my-project
```

Use this only when:

- you have switched models and no longer want to keep the older vectors
- you want to reclaim storage

## Typical Workflows

Enable embeddings for the first time:

```bash
mem-cli doctor
mem-cli reindex --project my-project
```

Switch to a new embedding model but keep the old one available:

1. Change `[embeddings]` in config.
2. Run:

```bash
mem-cli reembed --project my-project
```

3. Query normally. Semantic retrieval will use the new active space.

Switch models and later remove the old vectors:

```bash
mem-cli reembed --project my-project
mem-cli prune-embeddings --project my-project
```

## Troubleshooting

If semantic search is not working:

- run `mem-cli doctor`
- make sure `pgvector` is installed and the `vector` extension exists in the database
- make sure `[embeddings].model` is configured
- make sure the configured API key env var is present

If you just enabled embeddings and old memories still are not searchable semantically, run:

```bash
mem-cli reindex --project my-project
```

If you changed models and want the new model to be usable immediately, run:

```bash
mem-cli reembed --project my-project
```

## Related Docs

- [Getting Started](../getting-started.md)
- [Scan Command](scan.md)
- [How Memory Layer Works](../../developer/architecture/how-it-works.md)
