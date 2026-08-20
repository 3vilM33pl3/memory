# Embeddings Help

## Purpose
Inspect embedding backends, compare per-project coverage, switch semantic search, and backfill missing vectors.

## Layout
- Header: active backend, create setting, ready/not-ready counts, and operation status.
- Table: backend name, provider, model, create flag, base URL, chunk count, and memory count.
- `*` marks active. `!` marks a backend that did not resolve at startup.

## Controls
- `j/k` or `Up/Down`: select backend.
- `Enter`: activate selected backend, or deactivate when selected backend is active.
- `c`: toggle automatic embedding creation.
- `e`: create missing embeddings for selected backend.
- `I`: rebuild chunks and embeddings for all configured backends.
- `r`: refresh backend list and counts.

## Workflows
- Use `e` for normal missing-embedding backfill.
- Use `I` only when chunks need rebuilding or all backends should be refreshed.
- Switch active backend after both spaces are populated to compare semantic retrieval.

## Troubleshooting
- If a backend has `!`, fix API key/model config and restart service.
- If counts differ, run `e` on the lower-coverage backend.
