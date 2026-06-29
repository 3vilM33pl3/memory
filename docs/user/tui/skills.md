# Skills Tab

Use the `Skills` tab to inspect agent skills from inside the TUI. It defaults to
repo-local Memory Layer skills and can also show repo-local custom skills, home
directory skills, Codex skills, plugin skills, unmanaged skills, and skills that
need repair.

## What It Shows

- the current skill inventory filter and matching row count
- one row per skill in the selected inventory
- skill source, installed version, freshness status, and repair action
- installed `SKILL.md` path and source/template path when available
- selected skill description, location, repairability, and rendered `SKILL.md` content

The default `Memory` filter shows the managed Memory Layer bundle under
`.agents/skills/`. Use `Repo local`, `Home`, `Codex`, `Plugins`, `Unmanaged`, and
`All` when you need to trace which skill an agent can load and where it lives.

## Key Controls

- `j/k` move the selected skill
- `f` / `F` cycle the visible skill filter forward or backward
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

The repair action does not mutate home-directory, Codex, plugin, or custom
repo-local skills. Those entries are visible for inspection only and are marked
as unmanaged.

## When To Use It

- checking why the bottom status bar reports stale or missing skills
- inspecting the exact instructions an agent will load before using a skill
- confirming whether a skill came from the repo, home directory, Codex, or a plugin
- repairing outdated, invalid-version, unversioned, or missing Memory-owned skills

## See Also

- [`memory doctor`](../cli/doctor.md)
- [`memory upgrade`](../cli/upgrade.md)
- [TUI Guide](README.md)
