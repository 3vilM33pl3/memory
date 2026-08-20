# Skills Help

## Purpose
Inspect Memory Layer, repo-local, home-directory, Codex, and plugin skills used by coding agents.

## Layout
- Filter bar: active skill source filter and matching row count.
- Skill table: skill name, source, status, installed version, and pending repair action.
- Detail pane: selected skill source, path, template path, version detail, and SKILL.md content.
- Status line: current inventory summary, filter change, or repair result.

## Controls
- `j/k` or `Up/Down`: select a skill.
- `PgUp/PgDn`: scroll selected SKILL.md detail. `Home`: jump to top.
- `f` / `F`: cycle the visible skill filter forward or backward.
- `u`: repair repo-local Memory skills using the current template/GitHub fallback path.
- `r`: refresh project state outside help.

## Workflows
- Open this tab when the footer reports stale, missing, or unversioned skills.
- Keep the default Memory filter for the managed repo-local Memory Layer bundle.
- Switch to Repo local, Home, Codex, Plugins, Unmanaged, or All when tracing where an agent skill comes from.
- Review the selected skill's SKILL.md before asking an agent to use it.
- Use `u`, `memory doctor --fix`, or `memory upgrade` to repair Memory-owned skills.

## Troubleshooting
- If content is missing, verify the project has `.agents/skills/<name>/SKILL.md`.
- If repair fails, check the status message and run `memory doctor --fix` for more detail.
