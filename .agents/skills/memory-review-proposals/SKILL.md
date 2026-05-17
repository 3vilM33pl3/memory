---
name: memory-review-proposals
version: 0.8.7
description: Review pending Memory Layer replacement proposals interactively inside the LLM CLI; explain why each memory update was proposed, gather codebase proof when needed, and approve or reject only after explicit confirmation
---

# Memory Proposal Review Skill

Use this skill when:
- the user asks to review proposed memories, pending memory updates, or the curation review queue
- the agent needs to explain why a candidate memory would replace an existing memory
- the user wants to gather more evidence before approving or rejecting a pending proposal

Do not use this skill for:
- normal post-task remembering; use `memory-remember`
- creating new captures or running curation from scratch
- broad TUI usage questions that do not involve replacement proposals

## Script

List pending proposals:

```bash
go run ./.agents/skills/memory-layer/scripts/main.go review-proposals list --project <project-slug>
```

Show one proposal:

```bash
go run ./.agents/skills/memory-layer/scripts/main.go review-proposals show --project <project-slug> --id <proposal-id>
```

Resolve one proposal only after explicit confirmation:

```bash
go run ./.agents/skills/memory-layer/scripts/main.go review-proposals approve --project <project-slug> --id <proposal-id>
go run ./.agents/skills/memory-layer/scripts/main.go review-proposals reject --project <project-slug> --id <proposal-id>
```

## Workflow

1. List pending proposals and pick the next proposal the user wants to inspect.
2. Explain the proposal in plain language:
   - target memory summary
   - candidate memory summary and full text
   - candidate type, score, policy, and matcher reasons
   - what would happen if approved
3. If evidence is unclear, offer investigation options in the CLI conversation:
   - search for proof in the codebase with `rg`
   - inspect the target memory history with `memory history <target-memory-id> --json`
   - search project memory with `memory query --project <project> --question "<question>" --json`
   - skip and leave the proposal pending
4. For codebase proof, derive focused searches from the candidate text, target/candidate summaries, file paths in sources when visible, and the matcher reasons. Report both confirming and contradicting evidence.
5. Recommend approve, reject, or leave pending. Prefer leave pending when evidence is insufficient.
6. Ask for explicit confirmation before running approve or reject. Do not bulk-resolve proposals unless the user explicitly asks and each decision is justified.
7. After resolution, refresh the proposal list and continue with the next selected proposal if requested.

## Decision Rules

- Approve when the candidate is more current, more specific, and supported by code/docs/history evidence.
- Reject when the candidate is duplicate noise, stale, too broad, contradicted by the repo, or would replace a better existing memory.
- Leave pending when the evidence is not clear enough to make a durable-memory decision.
- Never invent provenance. If proof cannot be found, say that directly.

## Runtime Requirement

This focused skill uses the shared Go helper under `.agents/skills/memory-layer/scripts/`.
`go` must be available on `PATH` for these helper commands to run.
