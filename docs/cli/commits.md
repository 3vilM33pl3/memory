# `mem-cli commits`

`mem-cli commits` manages stored git history for a project.

It does **not** turn every commit into canonical memory.
Instead, it stores commits as project-scoped evidence so Memory Layer can keep:

- full commit hashes
- commit messages
- authors
- timestamps
- changed paths

while still keeping `memory_entries` selective and curated.

## Commands

Sync commit history into Memory Layer:

```bash
mem-cli commits sync --project my-project
```

Limit the import:

```bash
mem-cli commits sync --project my-project --limit 100
mem-cli commits sync --project my-project --since "2 weeks ago"
```

List stored commits:

```bash
mem-cli commits list --project my-project
```

Show one stored commit:

```bash
mem-cli commits show --project my-project <commit-hash>
```

## What Gets Stored

For each imported commit, Memory Layer stores:

- full hash
- short hash
- subject
- body
- author name and email when available
- commit timestamp
- parent hashes
- changed paths
- import timestamp

## Why This Exists

This feature is meant to preserve git history as evidence without degrading normal memory search.

Default query behavior remains memory-first:

- normal `mem-cli query` returns curated memories
- commit history is separate evidence you can inspect directly
- curated memories can still cite commits as provenance
