---
name: memory-project-init
version: 0.1.0
description: Initialise or refresh Memory Layer in a project directory after Memory Layer is already installed; use for repo-local setup with .mem, .agents/memory-layer.toml, and repo-local memory skills, previewing writes before applying them
---

# Memory Project Init Skill

Use this skill when:
- the user asks to initialise, configure, bootstrap, or set up Memory Layer for a project
- Memory Layer is already installed on the machine
- the target work is repo-local `.mem/` and `.agents/` setup

Do not use this skill for:
- installing Memory Layer packages, PostgreSQL, pgvector, or system services
- creating or reinitialising git history
- broad Memory Layer behavior questions
- completed-work remembering after setup is done

## Workflow

1. Identify the target project directory; if the user did not specify one, use the current working directory.
2. Run `memory health` and `memory doctor` to check the shared backend and current repo state.
3. Inspect existing `.mem/`, `.agents/memory-layer.toml`, and `.agents/skills/` if present.
4. Preserve existing local customisations; do not delete or overwrite them without an explicit user request.
5. Run `memory wizard --dry-run` in the target project and review the planned repo-local writes.
6. If the preview is safe, run `memory wizard` in the target project.
7. Run `memory doctor` again.
8. Report the project slug, files created or preserved, backend health, and next commands.

## Optional Evidence Setup

Only run these if the user explicitly wants initial evidence imported:

```bash
memory commits sync --project <project-slug> --dry-run
memory scan --project <project-slug> --dry-run
```

Ask before running the non-dry-run versions. Use `commits sync` for commit history evidence and `scan` for LLM-assisted initial memory candidates.

## Safety Rules

- Preview write-capable setup commands first with `--dry-run`.
- Do not run package managers or service installers from this skill.
- Do not create, delete, or reinitialise git repositories.
- If `.mem/config.toml`, `.mem/project.toml`, `.agents/memory-layer.toml`, or any skill directory exists, treat it as user/project state.

## Runtime Requirement

The repo-local Memory Layer skill bundle uses the shared Go helper under `.agents/skills/memory-layer/scripts/`.
`go` must be available on `PATH` for those helper commands to run after setup.
