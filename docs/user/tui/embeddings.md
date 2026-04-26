# Embeddings Tab

Shows every embedding backend declared under `[[embeddings.backends]]` in your Memory Layer config, their per-project coverage, and lets you switch which backend search uses without leaving the TUI.

![Embeddings tab](../../img/tui/embeddings-tab.png)

## Table of Contents

- [When To Use](#when-to-use)
- [What You See](#what-you-see)
- [Keybindings](#keybindings)
- [How Activation Works](#how-activation-works)
- [Related Docs](#related-docs)

## When To Use

- You want a quick read on which embedding backends are configured on this install and whether any of them failed to resolve (missing API key, empty model).
- You want to see how many chunks each backend covers for the project you're browsing, to tell whether a newly-added backend has been populated yet.
- You want to flip `memory query` to a different backend momentarily (e.g. compare OpenAI vs Voyage retrieval quality) and flip back without running any CLI.

## What You See

**Header panel**

- `Active:` the name of the backend currently used for semantic retrieval, or `(none)` when nothing is active.
- `Backends: N configured · M ready · K not ready` — `ready` means the backend resolved at service startup (API key env var present, `model` non-empty). `not ready` entries are declared in config but can't embed until fixed; they show `!` in the leading column.
- `Status:` transient activation or refresh status — `activating <name>…`, `Activated <name>`, `Activation failed: …`, or `idle`.

**Backends table**

One row per backend. Columns:

| Column | Meaning |
|---|---|
| leading marker | `*` for the active backend, `!` for a backend that failed to resolve, blank otherwise |
| `NAME` | the backend's `name` field (your handle for `memory embeddings activate <name>`) |
| `PROVIDER` | e.g. `openai_compatible`, `voyage`, `cohere`, `gemini` |
| `MODEL` | the exact model name as sent to the provider |
| `BASE URL` | empty when the backend uses the provider's default endpoint; otherwise the configured URL |
| `CHUNKS` | chunks in the current project that have an embedding in this backend's space |
| `MEMORIES` | distinct memories in the current project covered by this backend |

A healthy dual-backend setup shows equal `CHUNKS` and `MEMORIES` across both rows. A freshly-added backend shows zero until you run `memory embeddings reembed --project <slug>`.

## Keybindings

| Key | Action |
|---|---|
| `j` / `↓` | Move selection down (wraps to the first row from the last) |
| `k` / `↑` | Move selection up (wraps to the last row from the first) |
| `Enter` | Activate the highlighted backend as the new search target |
| `r` | Force an immediate refresh of the backend list and coverage counts |

Tab movement (`Tab`, `Shift+Tab`, `l`, `h`) and the quit shortcut (`Ctrl+C`) work as on every other tab.

## How Activation Works

Pressing `Enter` fires `POST /v1/embeddings/activate {name: "<selected>"}`. The service:

1. Validates the name against its in-memory registry.
2. Flips the active backend atomically (`tokio::sync::RwLock` writer; search reads keep working).
3. Rewrites `embeddings.active` in your `memory-layer.toml` using `toml_edit`, preserving comments and formatting around every other key.
4. Returns the refreshed backend list, which the tab uses as its next snapshot.

No embedding rows are touched, so flipping back is instantaneous. See [Embedding Operations](../cli/embeddings.md) for the underlying model.

If activation fails (e.g. the config file can't be written because of file permissions), the transient status line shows `Activation failed: …` and the in-memory active stays on whatever was active before — config and registry stay in sync.

## Related Docs

- [Embedding Operations](../cli/embeddings.md) — CLI commands and the "configure multiple, activate one" workflow.
- [Embeddings and Search](../../developer/architecture/embeddings-and-search.md) — internals: how spaces are keyed and why multiple can coexist.
- [TUI Guide](README.md) — other tabs and shared navigation.
