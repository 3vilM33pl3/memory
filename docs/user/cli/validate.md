# Validate Command

`memory validate` runs the evidence-backed validation pipeline for one
memory: it gathers the memory's sources, provenance status, related
memories, and recent git history over its source paths, asks the configured
LLM for a verdict, and applies the configured policy.

Manual runs bypass the activation threshold but respect the global
`reinforcement.daily_validation_cap`.

## Usage

```bash
memory validate <memory-id>            # uses the configured dry-run default
memory validate <memory-id> --dry-run  # report only, never changes anything
memory validate <memory-id> --execute  # allow policy actions to apply
memory validate <memory-id> --execute --json
```

## Outcomes

- `revalidated` — the memory is accurate and clearly worded; its
  validation metadata is refreshed.
- `reworded` — wording was improved and applied as a new immutable memory
  version (only with `reinforcement.auto_apply_rewording = true` and high
  confidence).
- `correction_pending` — a proposed correction awaits human review
  (`memory review list` / `apply` / `reject`).
- `flagged_needs_review` — evidence was weak, ambiguous, or contradictory;
  the memory is flagged and rank-penalized, and content is never modified.

Dry runs report the same outcomes as `would_*` actions.

Validation requires `reinforcement.validation_enabled = true` for the
background scheduler; the manual command works whenever reinforcement is
enabled and an LLM is configured.

See `docs/developer/architecture/memory-reinforcement.md` for details.
