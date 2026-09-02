# Agents Tab

**Agents** is a read-only monitor for local Codex and Claude sessions across projects.

![Active coding-agent sessions in the TUI](https://www.memory-layer.dev/images/tui/agents-tab.png)

Rows show project, agent type, state, token and context pressure, and current task. The selected detail can include process ID, session ID, working directory, model, Git state, child processes, open ports, and account-level rate-limit information when available.

## Controls

- `j` / `k` choose a visible session.
- `PgUp` / `PgDn` and `Home` scroll details.
- `r` refreshes the project snapshot; live session collection also updates in the background.
- `h` opens help.

Use Agents to spot worktree collisions, context pressure, stranded processes, or a session that needs a handoff. It does not control the selected agent.

See also: [Agent workspace coordination](https://www.memory-layer.dev/docs/reference/cli/integrations-evals).
