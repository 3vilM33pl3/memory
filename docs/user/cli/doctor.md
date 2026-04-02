# `memory doctor`

Use `doctor` when Memory Layer is installed but something is not working the way you expect.

`doctor` is the main setup diagnostic command.

## Table of Contents

- [What It Checks](#what-it-checks)
- [Typical Usage](#typical-usage)
- [Common Failures](#common-failures)

## What It Checks

`memory doctor` checks the current setup for common problems such as:

- missing or placeholder database URL
- missing or placeholder service API token
- unexpected auto-derived or overridden writer identity
- backend connectivity issues
- missing `pgvector`
- project bootstrap problems such as missing `.mem` files
- repo-local service or watcher configuration issues

The exact output is meant to be actionable, not just descriptive.

## Typical Usage

Run it any time setup looks suspicious:

```bash
memory doctor
```

It is especially useful after:

- first install
- upgrades
- changing database config
- enabling embeddings
- bootstrapping a new repo

## Common Failures

Typical remediation paths are:

- unexpected writer identity
  - set `[writer].id` or `MEMORY_LAYER_WRITER_ID` if you want a custom stable label instead of the auto-derived default
- missing `pgvector`
  - install the PostgreSQL package for your server version and enable the `vector` extension
- repo not initialized
  - run `memory wizard` or `memory init`
- backend unreachable
  - start the shared backend service or the local development backend
- placeholder service API token
  - run `memory service ensure-api-token --rotate-placeholder` or `memory wizard --global`

Use `doctor` first before assuming the memory database or watcher system is broken.
