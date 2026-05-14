# `memory init`

`memory init` bootstraps or refreshes Memory Layer for a repository.

Use it when you want the low-level bootstrap step without the interactive wizard.

## Common Usage

```bash
memory init
memory init --project memory
memory init --force
memory init --dry-run
memory upgrade --dry-run
```

## What It Does

- creates or repairs the user-local project config/state directories
- creates or repairs the tiny repo-local `.mem/project.toml` marker
- creates or repairs `.agents/memory-layer.toml`
- installs or refreshes the bundled repo-local Memory Layer skills
- can preview the planned file writes with `--dry-run`

The installed skill bundle includes:

- `memory-layer`
- `memory-project-init`
- `memory-query-resume`
- `memory-plan-execution`
- `memory-direct-task-start`
- `memory-remember`

These are copied from the installed `skill-template` into `.agents/skills/`. The umbrella skill and shared Go helper live under `.agents/skills/memory-layer/`; focused workflow skills live beside it.

## Notes

- prefer [`memory wizard`](wizard.md) for the normal guided setup flow
- use `--force` only when you intentionally want to replace managed bootstrap files
- existing `.mem/config.toml` and `.mem/memory-layer.env` files are treated as legacy inputs and can be migrated with `memory doctor --fix`
- after package upgrades, use [`memory upgrade`](upgrade.md) to refresh existing repo-local skill copies from the installed template
