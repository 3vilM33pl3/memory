# `memory upgrade`

`memory upgrade` refreshes the repo-local Memory Layer skill bundle in the current project.

Use it after installing a newer Memory Layer package or when `memory doctor` reports that project skills are missing, unversioned, older than the installed template, or older than the GitHub skill bundle.

## Common Usage

```bash
memory upgrade --dry-run
memory upgrade --dry-run --json
memory upgrade
memory upgrade --force
```

## What It Does

- compares `.agents/skills/` against the best available Memory skill template
- downloads the current GitHub skill bundle for real upgrade runs when possible
- falls back to the installed `skill-template` when GitHub is unavailable
- reads the canonical shared bundle `version` from each `SKILL.md`
- installs missing Memory-owned skills
- replaces outdated, unversioned, or invalid-version Memory-owned skills
- backs up replaced skill directories under the user-local project runtime directory at `runtime/skill-backups/<timestamp>/`

By default, it does not replace a project-local skill that is newer than the selected template. Use `--force` only when you intentionally want to replace all known Memory-owned skills from the template.

All bundled Memory skills should report the same version as the Memory package. The JSON output includes a top-level bundle version/status and per-skill details for troubleshooting.

The Memory-owned skill set is:

- `memory-layer`
- `memory-project-init`
- `memory-github-init`
- `memory-query-resume`
- `memory-review-proposals`
- `memory-plan-execution`
- `memory-direct-task-start`
- `memory-remember`

Template sources:

- GitHub archive: `https://github.com/3vilM33pl3/memory/archive/refs/heads/main.zip`
- cached GitHub template: user-local state directory under `skill-template-github/main`

- Debian package: `/usr/share/memory-layer/skill-template/`
- local Linux install script: `~/.local/share/memory-layer/skill-template/` unless `XDG_DATA_HOME` overrides it
- macOS `.pkg`: `/usr/local/share/memory-layer/skill-template/`
- Homebrew: `$(brew --prefix)/share/memory-layer/skill-template/`
- source/dev checkout: `.agents/skills/`

## Doctor Integration

`memory doctor` includes a `workflow.project_skills` check. If the check warns, run:

```bash
memory upgrade --dry-run
memory upgrade
```

`memory doctor` also includes a `workflow.project_skills_github` check. If that check warns, `memory doctor --fix` downloads the current GitHub skill bundle and applies the same safe upgrade path. It still avoids force-replacing newer local skills.
