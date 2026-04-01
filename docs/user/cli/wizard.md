# `mem-cli wizard`

Use `wizard` to bootstrap Memory Layer configuration either globally for the machine or locally for the current repository.

## Table of Contents

- [Global vs Repo-Local](#global-vs-repo-local)
- [What It Creates](#what-it-creates)
- [Typical Usage](#typical-usage)
- [How It Differs From `init`](#how-it-differs-from-init)

## Global vs Repo-Local

`mem-cli wizard --global` configures shared machine-level settings such as:

- `database.url`
- default `writer.id`
- optional LLM settings

The shared service API token is usually generated automatically into the adjacent `memory-layer.env` file. The wizard only needs an explicit token value if you want to override that generated token.

`mem-cli wizard` inside a repository is local-first and bootstraps project files such as:

- `.mem/config.toml`
- `.mem/project.toml`
- `.agents/memory-layer.toml`
- `.agents/skills/memory-layer/`

Inside a repository, the wizard defaults to repo-local scope unless you explicitly choose shared/global setup.

## What It Creates

At repo scope, the wizard creates:

- `.mem/` runtime and repo-local config files
- `.agents/memory-layer.toml`
- a repo-local copy of the Memory Layer skill

The skill copy is created from the packaged `skill-template`, or from the repo-local template during source/dev usage.

## Typical Usage

First machine setup:

```bash
mem-cli wizard --global
```

Then inside each repo:

```bash
cd /path/to/your-project
mem-cli wizard
```

## How It Differs From `init`

`init` is the lower-level repo bootstrap command.

For normal interactive setup, prefer `wizard`.

Use `init` when you want:

- a more scriptable bootstrap path
- a non-interactive setup flow
- direct control over the generated repo-local files

For the full onboarding flow, see [Getting Started](../getting-started.md).
