# `memory agent`

`memory agent` records which local agents are working on which branches or
worktrees. It is a coordination surface for running multiple Codex, Claude,
OpenCode, or scripted agents against the same project without losing track of
branch ownership, dirty file overlap, or stale work.

The command stores coordination metadata in the Memory Layer service. It does
not replace Git. Agents still need normal branches, commits, pushes, and merges.

## Workflow

Start a workspace when an agent begins work:

```bash
memory agent start --project memory --task "Graph UI follow-up"
```

Check active work before starting or merging another agent:

```bash
memory agent status --project memory
memory agent status --project memory --include-finished --json
```

Finish the workspace after pushing or intentionally abandoning the work:

```bash
memory agent finish --project memory --summary "Pushed graph UI follow-up"
memory agent finish --project memory --abandoned --summary "Superseded by main"
```

If multiple active workspaces match the same repository and branch, pass the
workspace id from `memory agent status`:

```bash
memory agent finish --project memory --workspace-id 00000000-0000-0000-0000-000000000000
```

## Commands

### `start`

`memory agent start` writes an active workspace record from the current Git
checkout.

It records:

- project slug
- repository root and worktree path
- branch
- optional task label
- base commit and head commit when Git can resolve them
- current dirty files from `git status --porcelain`
- agent CLI name and agent session id
- writer id, hostname, profile, and service endpoint

Useful options:

- `--project <slug>`: required project slug.
- `--task <text>`: short reason this agent exists.
- `--agent-cli <name>`: defaults to `codex`; use `opencode`, `claude`, or another integration name when appropriate.
- `--agent-session-id <id>`: stable session id. If omitted, Memory Layer checks common agent environment variables and falls back to the current process id.
- `--branch <name>`: override detected branch.
- `--json`: emit the created workspace record.

### `status`

`memory agent status` lists active workspaces for a project.

It reports warnings when active workspaces:

- share the same branch
- share the same worktree path
- have overlapping dirty files
- have uncommitted dirty files
- have not heartbeated recently

Use `--include-finished` to include completed and abandoned workspaces.

### `finish`

`memory agent finish` marks one active workspace `completed` or `abandoned`.

It refreshes the head commit and dirty file list from the current checkout,
records an optional summary, and stores whether the branch appears pushed. The
pushed check uses the Git upstream when one exists; pass `--pushed true` or
`--pushed false` to override it.

Useful options:

- `--workspace-id <uuid>`: finish a specific workspace when auto-detection is ambiguous.
- `--summary <text>`: final note about the work.
- `--abandoned`: mark as abandoned instead of completed.
- `--pushed <bool>`: override pushed-branch detection.
- `--merged-commit <sha>`: record the merge commit when known.
- `--json`: emit the finished workspace record.

## API And UI Integration

The backend exposes the same records through:

- `GET /v1/agents/workspaces?project=<slug>`
- `POST /v1/agents/workspaces/start`
- `POST /v1/agents/workspaces/{workspace_id}/heartbeat`
- `POST /v1/agents/workspaces/{workspace_id}/finish`

`/v1/runtime/status` also includes the active workspace list in
`agent_workspaces` so browser, TUI, and automation clients can surface
coordination warnings alongside watcher and skill health.
