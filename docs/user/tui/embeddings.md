# Embeddings Tab

**Embeddings** shows every configured semantic-search backend, its readiness, active state, automatic-creation setting, and coverage for the current project.

![Embedding backend coverage in the TUI](https://www.memory-layer.dev/images/tui/embeddings-tab.png)

The table distinguishes the active backend, an unavailable backend, configured provider/model, automatic creation, and per-project chunk and memory coverage. A newly added backend may legitimately show zero coverage until it is backfilled.

## Controls

| Key | Action |
| --- | --- |
| `j` / `k` | Select a backend |
| `Enter` | Activate the selected backend or turn embeddings off when it is already active |
| `c` | Toggle automatic embedding creation for the selected backend |
| `e` | Create missing embeddings for the selected backend |
| `I` | Rebuild chunks and embeddings across configured backends |
| `r` | Refresh coverage |
| `h` | Open help |

Activation changes configuration but does not rewrite existing vectors. Use explicit re-embedding or reindexing for backfill, and start with a dry run from the CLI when operating at scale.

See also: [Embedding reference](https://www.memory-layer.dev/docs/reference/cli/repository-evidence).
