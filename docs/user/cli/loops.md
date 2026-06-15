# `memory loops`

`memory loops` operates the loop engineering control plane. It is for inspecting registered loop definitions, changing loop settings, creating manual policy-checked runs, reviewing approval requests, and using the global kill switch.

This command is an operator surface over the service API. It does not bypass policy checks, and the first implementation records control-plane runs before full runner adapters are enabled.

## Common Workflow

```bash
memory loops list
memory loops show context_pack_refresh --project memory
memory loops enable context_pack_refresh --project memory --mode suggest_only --explicit-user-approval
memory loops run context_pack_refresh --project memory --dry-run --reason "manual smoke test"
memory loops runs --project memory
memory loops inspect <run-id>
```

Use `--json` on any subcommand when another tool will parse the result.

## Loop Definitions

```bash
memory loops list
memory loops list --json
memory loops show context_pack_refresh --project memory
memory loops show context_pack_refresh --project memory --repo-root "$PWD"
```

`list` prints the registered loop id, version, risk level, default mode, name, and description. `show` adds the effective settings for the selected project or repo scope when those arguments are provided.

## Settings

```bash
memory loops enable context_pack_refresh --project memory --mode suggest_only --explicit-user-approval
memory loops disable context_pack_refresh --project memory --reason "not ready for this repo"
memory loops pause context_pack_refresh --project memory --until 2026-06-16T09:00:00Z
memory loops snooze context_pack_refresh --project memory --until 2026-06-16T09:00:00Z
```

Scope resolution accepts:

- `--project <slug>` for project-level settings.
- `--repo-root <path>` for repo-specific settings.
- `--scope-type user|workspace|project|repo --scope-id <id>` for explicit scopes.

Enable supports these modes: `observe`, `suggest_only`, `draft_output`, and `autonomous_safe`. High-risk changes can return an approval request instead of immediately changing effective settings. When a human has explicitly approved the setting change, pass `--explicit-user-approval`.

The text output prints the effective scope, mode, blocked reasons, and budget JSON when present.

## Runs

```bash
memory loops run context_pack_refresh --project memory --dry-run --reason "manual validation"
memory loops runs --project memory
memory loops runs --project memory --loop-id context_pack_refresh --status blocked
memory loops inspect <run-id>
memory loops cancel <run-id> --reason "superseded"
memory loops feedback <run-id> --rating useful --note "Good context pack"
memory loops replay <run-id> --dry-run
```

`run` creates a manual control-plane run and records policy decisions, effective settings, blocked reasons, output summary, and traces. `--dry-run` is intended for local validation and CI logs. `replay` reads a previous run and creates a new policy-checked run with the same loop id and scope plus a `replay_of` trigger payload.

## Approvals

```bash
memory loops approvals --project memory
memory loops approvals --project memory --status pending
memory loops approvals --project memory --run-id <run-id>
memory loops approvals --project memory --loop-id context_pack_refresh
memory loops approve <approval-id> --reviewer olivier --reason "Approved for this repo"
memory loops edit-approval <approval-id> --proposed-action '{"proposal_id":"...","scope":"project"}' --reviewer olivier --reason "Narrowed scope"
memory loops reject <approval-id> --reviewer olivier --reason "Too broad"
```

Approval requests are created by policy gates for risky settings or actions. Each
record includes the proposed action JSON, risk reason, linked loop/run, requester,
reviewer, and decision reason. `approve` accepts the proposed action, `reject`
records a rejection and blocks a queued/running linked run safely, and
`edit-approval` replaces the proposed action JSON with a reviewed version. The
service remains responsible for applying policy, tracing the decision, and
updating linked memory proposal status.

## Global Kill Switch

```bash
memory loops global-kill-switch
memory loops global-kill-switch --enabled true --updated-by olivier --reason "maintenance"
memory loops global-kill-switch --enabled false --updated-by olivier --reason "maintenance complete"
```

The global kill switch blocks loop execution across scopes. Use it for emergency stops, maintenance windows, or when loop policy behavior is under investigation.

## CI And Agent Usage

For logs that need to be human-readable:

```bash
memory loops run context_pack_refresh --project memory --dry-run --reason "pre-merge check"
memory loops runs --project memory --status blocked
```

For automation that needs stable output:

```bash
memory loops run context_pack_refresh --project memory --dry-run --json
memory loops approvals --project memory --status pending --json
memory loops edit-approval <approval-id> --proposed-action '{"proposal_id":"..."}' --json
```

Loop commands require the Memory service to be reachable and use the configured local API token for write-capable operations.
