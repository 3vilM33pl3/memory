# Skills Tab

Use the `Skills` tab to inspect and repair repo-local Memory Layer skills from
inside the TUI.

## What It Shows

- the current skill inventory filter
- one row per Memory-owned skill in the selected inventory
- installed version, template version, freshness status, and upgrade action
- installed `SKILL.md` path and source/template path when available
- selected skill description and rendered `SKILL.md` content

The TUI shows the full Memory-owned skill inventory so you can inspect focused
skills such as direct task start, plan execution, remember, query/resume, or
proposal review without leaving the terminal.

## Key Controls

- `j/k` move the selected skill
- `PgUp/PgDn` scroll the selected `SKILL.md` content
- `Home` jump the detail pane back to the top
- `u` repair repo-local Memory skills
- `r` refresh the project snapshot
- `h` open or close detailed help for this tab

## Repair Behavior

The repair action uses the same path as `memory doctor --fix`. It downloads the
current GitHub skill bundle when available, falls back to the installed local
template when offline, backs up replaced files, and only mutates Memory-owned
skill directories under `.agents/skills/`.

## When To Use It

- checking why the bottom status bar reports stale or missing skills
- inspecting the exact instructions an agent will load before using a skill
- confirming whether a skill came from the installed package or GitHub template
- repairing outdated, invalid-version, unversioned, or missing Memory-owned skills

## See Also

- [`memory doctor`](../cli/doctor.md)
- [`memory upgrade`](../cli/upgrade.md)
- [TUI Guide](README.md)
