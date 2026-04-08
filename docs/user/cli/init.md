# `memory init`

`memory init` bootstraps or refreshes the repo-local Memory Layer files inside a repository.

Use it when you want the low-level bootstrap step without the interactive wizard.

## Common Usage

```bash
memory init
memory init --project memory
memory init --force
memory init --dry-run
```

## What It Does

- creates or repairs `.mem/` bootstrap files
- installs or refreshes the bundled repo-local Memory Layer skills
- can preview the planned file writes with `--dry-run`

## Notes

- prefer [`memory wizard`](wizard.md) for the normal guided setup flow
- use `--force` only when you intentionally want to replace managed bootstrap files
