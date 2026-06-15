# Loop Automation Control Plane

Loop automation is the service-owned control plane for background or user-triggered agent loops. It stores definitions, settings, trigger events, run ledgers, traces, approvals, and future memory proposals. The control plane does not give loops a second persistence path; loop outputs still flow through explicit Memory APIs and approval workflows.

For the domain terminology and safety model, see [ADR 0001](../adr/0001-loop-automation-domain-model.md).

## Runtime Boundaries

- `crates/mem-api` owns the public request and response types for loop definitions, settings, runs, approvals, and trigger routing.
- `crates/mem-loops` owns pure loop domain logic: built-in definitions, effective-setting resolution, policy decisions, budget checks, and trigger eligibility decisions.
- `crates/mem-service/src/repository/handlers/loops.rs` owns database-backed registration, settings mutation, trigger-event persistence, run ledger writes, approval records, and route handlers.
- `crates/mem-cli/src/commands/loops.rs` is the operator CLI over the service API.

## Trigger Routing

`POST /v1/loops/triggers/route` records or dry-runs an external or internal trigger event and evaluates it against current loop definitions.

The request includes:

- `source`: where the trigger came from, such as `manual`, `schedule`, `github`, `ci`, `repo`, or `memory`.
- `event_type`: the normalized event, such as `manual`, `schedule`, `ci_failed`, `repo_docs_changed`, `memory_changed`, `issue_created`, or `pull_request_updated`.
- `project` and `repo_root`: optional scope hints for effective settings.
- `payload`: the original event payload after secret redaction by the caller.
- `dedupe_key`: optional idempotency key. Duplicate keys do not start more runs.
- `debounce_seconds`: optional recent-event window using source, event type, scope, and payload hash.
- `trust_level`: `high`, `medium`, `low`, or `data_only`.
- `candidate_loop_ids`: optional loop-id allowlist. Omit it to evaluate every current loop definition.

The response includes the stored trigger event, duplicate/debounced flags, one route decision per candidate loop, and run summaries for eligible loops that were started.

## Eligibility

The pure router in `mem-loops` is intentionally simple:

1. Check whether the loop definition `trigger_spec.supported` contains the event type.
2. Resolve effective settings for the project/repo scope.
3. Combine unsupported trigger, disabled loop, paused/snoozed loop, global kill switch, and exhausted budget reasons.
4. Start a control-plane run only when the trigger is supported and no skipped reasons remain.

Manual `memory loops run` calls still record a trigger event, but they use the manual path so the global kill switch does not block deliberate operator runs. Routed trigger events are non-manual and are blocked by the global kill switch.

## Persistence

`trigger_events` stores normalized trigger inputs with a payload hash and optional dedupe key. `loop_runs.trigger_event_id` links every run to the event that caused it. Duplicate or debounced trigger events return existing event context and do not create new run rows.

Run rows currently record policy-checked control-plane execution. Real runner adapters, context packs, and loop-specific outputs are later milestones.

## Validation

Use narrow tests while changing this area:

```bash
cargo test -p mem-loops --all-targets
cargo test -p mem-service --all-targets
```

Database-backed trigger routing is covered in `crates/mem-service/tests/db_repository.rs` when `MEMORY_LAYER_TEST_DATABASE_URL` is configured.

