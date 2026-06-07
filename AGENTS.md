# Memory Repo Agent Guide

Treat this file as the table of contents for working in this repository. Keep it
short, load only the linked docs you need, and update the durable source of truth
when repeated instructions or project knowledge should survive future sessions.

## Start Here

- Use the dev Memory Layer environment to store, retrieve, and search project
  memory.
- Before coding, read [CONTRIBUTING.md](CONTRIBUTING.md) and the code map at
  [docs/developer/architecture/code-map.md](docs/developer/architecture/code-map.md).
- For architecture context, start with [docs/developer/architecture/overview.md](docs/developer/architecture/overview.md).
- For reviewable refactor work, use
  [docs/reviews/refactor/README.md](docs/reviews/refactor/README.md).

## Memory Workflow

- For direct no-plan implementation work, record the task start before edits
  with `memory-direct-task-start` / `start-task-execution`.
- For approved plans, use the plan execution workflow before editing and verify
  the plan before claiming completion.
- Completed work should be remembered as `implementation`, or as `refactor` when
  the change intentionally reshapes code without functional behavior changes.
- Repo-local skills live under `.agents/skills/`; the overview is in
  [docs/developer/skills/memory-layer-skill.md](docs/developer/skills/memory-layer-skill.md).

## Version Control

- Keep commits small and incremental.
- Commit during a task when the subtask is contained.
- Always commit at the end of a task.
- Use expressive commit prefixes when they fit:
  - `Feat:` for user-visible features or capabilities.
  - `Fix:` for bugs, regressions, and broken behavior.
  - `Docs:` for documentation-only changes.
  - `Build:` for packaging, release, or dependency/build-system changes.
  - `Refactor:` for internal code reshaping without intended behavior change.
  - `Test:` for test-only changes.
  - `Chore:` for maintenance work that does not fit the categories above.
- Prefer the most specific prefix instead of defaulting to `Chore:`.

## When You Need X, Read Y

| Need | Source of truth |
| --- | --- |
| CLI behavior or output contracts | [docs/user/cli/](docs/user/cli/) and `crates/mem-cli` |
| Service routes, API behavior, or persistence | [docs/developer/architecture/](docs/developer/architecture/) and `crates/mem-service` / `crates/mem-api` |
| Search, retrieval, or embeddings | [docs/developer/architecture/embeddings-and-search.md](docs/developer/architecture/embeddings-and-search.md) and `crates/mem-search` |
| Curation, provenance, or memory types | [docs/developer/architecture/memory-types.md](docs/developer/architecture/memory-types.md) |
| TUI behavior | [docs/user/tui/](docs/user/tui/) and `crates/mem-cli/src/tui/` |
| Browser UI behavior | [docs/user/web-ui.md](docs/user/web-ui.md) and `web/src/` |
| Documentation site editing | [docs-site/README.md](docs-site/README.md) and [docs-site/EDITING.md](docs-site/EDITING.md) |
| Evaluations | [docs/developer/evaluation.md](docs/developer/evaluation.md) and [docs/user/cli/eval.md](docs/user/cli/eval.md) |
| Future ideas | [docs/future-development/README.md](docs/future-development/README.md) |

## Validation

- Use the smallest validation loop that proves the change.
- For common Rust, web, database, and eval commands, follow the validation
  sections in [CONTRIBUTING.md](CONTRIBUTING.md).
- If behavior changes touch user workflows, update the matching page under
  `docs/user/` or `docs-site/content/docs/`.

## Agent Legibility

- Prefer repo-local docs, code, tests, and memory evidence over assumptions.
- Keep pull requests reviewable: one behavioral change per PR, with mechanical
  moves separated from behavior changes.
- Do not duplicate long guidance here; link to the source document instead.
- If an instruction becomes recurring, move it into a stable doc, test, or
  project memory so future agents can find it.
