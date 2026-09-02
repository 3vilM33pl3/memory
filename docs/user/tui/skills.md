# Skills Tab

**Skills** shows the instructions an agent can actually load, including the managed Memory Layer bundle and visible local, home, Codex, plugin, and unmanaged skills.

![Skill inventory and rendered SKILL.md content in the TUI](https://www.memory-layer.dev/images/tui/skills-tab.png)

The default **Memory** filter focuses on `.agents/skills/`. Each row reports source, version, freshness, repairability, installed path, template path, and rendered `SKILL.md` content.

## Controls

- `j` / `k` choose a skill.
- `f` / `F` cycle filters forward or backward.
- `PgUp` / `PgDn` and `Home` scroll the selected instructions.
- `u` repairs Memory-owned repo-local skills.
- `r` refreshes and `h` opens help.

Repair uses the same safe path as `memory doctor --fix`: it backs up replaced Memory-owned directories and does not mutate custom, home, Codex, or plugin skills.

See also: [Install and setup](https://www.memory-layer.dev/docs/install) and [diagnostics](https://www.memory-layer.dev/docs/reference/cli/service-health).
