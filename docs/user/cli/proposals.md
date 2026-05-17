# Proposals Command

`memory proposals` reviews pending memory replacement proposals produced by curation. It is the CLI surface behind the agent-facing `memory-review-proposals` skill and mirrors the TUI Review tab semantics.

Use it when an agent or human wants to inspect why a candidate memory might replace an existing memory before approving or rejecting it.

## Commands

List pending proposals:

```bash
memory proposals list --project memory
memory proposals list --project memory --limit 5 --json
```

Show one proposal:

```bash
memory proposals show --project memory --id <proposal-id>
memory proposals show --project memory --id <proposal-id> --json
```

Resolve a proposal after review:

```bash
memory proposals approve --project memory --id <proposal-id> --json
memory proposals reject --project memory --id <proposal-id> --json
```

## Review Meaning

- `target` is the existing active memory that would be replaced.
- `candidate` is the proposed new memory text.
- `score`, `policy`, and `reasons` explain why curation queued the proposal instead of automatically applying it.
- approving writes a new version of the target memory and removes the proposal from the queue.
- rejecting discards the candidate and leaves the target memory unchanged.

Approving and rejecting mutate memory state. Agents should explain the proposal, gather codebase or memory proof when needed, and ask for explicit confirmation before resolving.

## Related Docs

- [Review Tab](../tui/review.md)
- [Curate Command](curate.md)
- [Memory Layer Skill Bundle](../../developer/skills/memory-layer-skill.md)
