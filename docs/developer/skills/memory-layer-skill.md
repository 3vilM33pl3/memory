---
name: memory-layer
description: Query project memory before answering project-specific questions; capture completed task context; curate raw captures into durable canonical memory with provenance.
---

# Memory Layer Skill

## Table of Contents

- [Purpose](#purpose)
- [Goals](#goals)
- [Available Scripts](#available-scripts)
- [When to Query Memory](#when-to-query-memory)
- [When to Capture Task Context](#when-to-capture-task-context)
- [When to Curate Memory](#when-to-curate-memory)
- [Query Behavior Rules](#query-behavior-rules)
- [Capture Payload Guidance](#capture-payload-guidance)
- [Provenance Rules](#provenance-rules)
- [Practical Workflow Patterns](#practical-workflow-patterns)
- [Example Session](#example-session)
- [Error Handling](#error-handling)
- [Decision Heuristic](#decision-heuristic)
- [References](#references)

## Purpose

This skill integrates Codex with the local memory-layer service.

Use this skill when:
- the user asks a project-specific question like “how do we do X here?”
- you discover durable project knowledge while working
- you complete a task that should be remembered
- the user explicitly asks to update or query memory

Do not use this skill for:
- generic programming questions with no project-specific context
- speculative memory creation without provenance
- storing trivial or temporary information that has no future value

---

## Goals

1. Query existing project memory before answering project-specific questions.
2. Capture useful task outcomes after meaningful work.
3. Curate raw captures into durable memory when the task is complete or when explicitly requested.
4. Preserve provenance for every stored memory item.
5. Prefer “insufficient evidence” over guessing.

---

## Available Scripts

### Query memory
```bash
./.agents/skills/memory-layer/scripts/query-memory.sh "<question>"
```

### Capture completed task
```bash
./.agents/skills/memory-layer/scripts/capture-task.sh <payload.json>
```

### Curate memory
```bash
./.agents/skills/memory-layer/scripts/curate-memory.sh
```

---

## When to Query Memory

Query memory before answering if:
- the question is about this repository, project, team conventions, architecture, incidents, workflows, or implementation history
- the answer may already exist in project memory
- the user asks “how do we do this here?”, “what is the convention?”, “what did we decide?”, “why is this implemented this way?”

Examples:
- “How do we handle JWT refresh token rotation here?”
- “What’s the testing convention in this repo?”
- “Why do we use this migration approach?”
- “What’s the background on this auth refactor?”

### Query workflow
1. Form a concise search question.
2. Run the query script.
3. Read the returned answer, ranked entries, and provenance.
4. Use the result in your reasoning.
5. If confidence is low or evidence is insufficient, say so clearly.

---

## When to Capture Task Context

Capture task context after:
- meaningful code changes
- architecture changes
- debugging that revealed a durable lesson
- implementation of a new convention or decision
- completing a user-requested task that future work may depend on

Do not capture:
- trivial edits
- purely temporary trial-and-error
- unverified assumptions
- duplicate notes with no new value

### Capture workflow
1. Create a structured task payload.
2. Include:
   - project
   - task title
   - user prompt
   - agent summary
   - files changed
   - test results
   - notable outputs
   - lessons learned
3. Write payload to a JSON file.
4. Run the capture script with that file.

---

## When to Curate Memory

Curate memory when:
- a task is complete
- multiple related captures have accumulated
- the user explicitly asks to update memory
- the current work surfaced reusable project knowledge

Curate after capture, not before.

Do not curate if:
- no meaningful new information was captured
- the work was incomplete and conclusions are still uncertain

### Curate workflow
1. Ensure capture has already happened.
2. Run the curate script.
3. Wait for success/failure output.
4. If curation fails, report the failure clearly and continue without pretending memory was updated.

---

## Query Behavior Rules

When using query results:
- treat provenance-backed memory as higher confidence
- distinguish between direct evidence and inference
- cite relevant memory entries or file paths where useful
- do not overstate certainty
- if the memory layer returns insufficient evidence, say so

Preferred answer style:
- concise conclusion first
- then supporting project-specific details
- then provenance / memory references if useful

---

## Capture Payload Guidance

A good capture payload should look like this:

```json
{
  "project": "my-project",
  "task_title": "Add JWT refresh token rotation",
  "user_prompt": "Implement refresh token rotation",
  "agent_summary": "Added single-use refresh token rotation and revocation checks",
  "files_changed": [
    "auth/refresh.rs",
    "auth/tokens.rs"
  ],
  "git_diff_summary": "Introduced rotation and revocation logic",
  "tests": [
    {
      "command": "cargo test -p auth",
      "status": "passed"
    }
  ],
  "notes": [
    "Refresh tokens must be invalidated after use"
  ]
}
```

Prefer:
- concrete facts
- verified outcomes
- specific files
- concise lessons

Avoid:
- vague commentary
- speculation
- chain-of-thought style notes
- unsupported conclusions

---

## Provenance Rules

Never invent provenance.

Every captured or curated memory must be traceable to one or more of:
- the task prompt
- changed files
- command outputs
- tests
- user-provided repository context
- explicit documented project information

If provenance is weak, do not present the result as a durable fact.

---

## Practical Workflow Patterns

## Pattern 1: Project-specific question
1. Query memory first.
2. Use the result to answer.
3. If you discover new durable facts while verifying, capture them after the task.

## Pattern 2: Implementation task
1. Do the task.
2. Summarize what changed and what was learned.
3. Capture the task context.
4. Curate after completion.

## Pattern 3: Debugging task
1. Fix the bug.
2. Capture only the durable lesson, not all failed attempts.
3. Curate once the fix is verified.

## Pattern 4: Architecture/decision work
1. Capture the decision rationale and scope.
2. Curate into canonical project memory.
3. Use query in future when similar questions arise.

---

## Example Session

### User asks:
“How do we handle JWT refresh token rotation in this project?”

### You should:
1. Run query-memory.
2. Read the result.
3. Answer using project memory.
4. If you changed the implementation or clarified missing knowledge during the task, capture and curate afterwards.

### User asks:
“Implement refresh token rotation and remember it for later.”

### You should:
1. Complete the implementation.
2. Create a capture payload.
3. Run capture-task.
4. Run curate-memory.
5. Report success or failure honestly.

---

## Error Handling

If query fails:
- say memory lookup failed
- continue with normal repo analysis if appropriate
- do not pretend memory was consulted

If capture fails:
- say task context was not stored
- do not claim the system remembers it

If curate fails:
- say memory was not updated
- continue normally, but be explicit

---

## Decision Heuristic

Use this simple heuristic:

- **Question about this project?** Query.
- **Completed meaningful work?** Capture.
- **Useful new knowledge ready to persist?** Curate.

---

## References

See:
- `./.agents/skills/memory-layer/references/architecture.md`
- `./.agents/skills/memory-layer/references/query-contract.md`
- `./.agents/skills/memory-layer/references/curation-rules.md`
