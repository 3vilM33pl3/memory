# How Skills Work

This page explains how the Memory Layer skill system fits together in this repository.

It covers two different things:

- how the **agent runtime** discovers and selects skills
- how **Memory Layer** bootstraps and ships the repo-local skill files

## Table of Contents

- [Mental Model](#mental-model)
- [Short Description vs `SKILL.md`](#short-description-vs-skillmd)
- [When A Skill Is Selected](#when-a-skill-is-selected)
- [When `SKILL.md` Is Read](#when-skillmd-is-read)
- [What `memory` Does And Does Not Do](#what-memory-does-and-does-not-do)
- [Canonical Skill vs Template vs Example](#canonical-skill-vs-template-vs-example)
- [Bootstrap And Packaging](#bootstrap-and-packaging)
- [Related Docs](#related-docs)

## Mental Model

There are three layers:

1. the agent runtime decides whether a skill matches a turn
2. the selected skill defines the workflow the agent should follow
3. Memory Layer commands and scripts do the actual work

The important split is:

- the **agent runtime** selects and reads skills
- the **skill** tells the agent what to do
- `memory` does **not** decide whether the agent should use a skill

## Short Description vs `SKILL.md`

Skill selection does not start by reading the full `SKILL.md`.

Instead, the runtime first works from a lightweight skill catalog entry:

- skill name
- short description
- path to the skill entrypoint

That short description is what lets the runtime decide that a task “looks like” it should use the skill.

Only after that selection step does the runtime open the full `SKILL.md` and follow the detailed workflow.

## When A Skill Is Selected

At runtime, a skill is selected when either:

- the user explicitly names it
- or the request clearly matches the skill description

For the `memory-layer` skill, those matches are things like:

- project-specific questions about this repo
- asking what changed or what is known already
- storing durable project knowledge
- resuming work after an interruption
- transitioning from planning into approved execution

## When `SKILL.md` Is Read

`SKILL.md` is read when the skill has been selected for the current turn.

That means:

- it is **not** read on every `memory` command
- it is **not** necessarily read on every chat turn
- it is read on turns where the runtime has decided the skill applies

In practice, changes to the live repo-local `SKILL.md` are picked up the next time the runtime selects and reads that skill.

## What `memory` Does And Does Not Do

`memory` provides the commands and bootstrap logic, but it does not make skill-selection decisions.

`memory` does:

- initialize a repo-local memory skill bundle during bootstrap
- copy the packaged or repo-local `skill-template` into `.agents/skills/`
- provide the command surface the skill scripts call

`memory` does not:

- parse `SKILL.md` on every CLI invocation
- decide that an agent should use the skill for a turn

## Canonical Skill vs Template vs Example

There are three important memory-skill copies in this repository:

1. canonical live skill bundle
   - `.agents/skills/`
   - the umbrella skill is `.agents/skills/memory-layer/`
   - the focused skills are:
     - `.agents/skills/memory-query-resume/`
     - `.agents/skills/memory-plan-execution/`
     - `.agents/skills/memory-remember/`
   - this bundle is what the agent uses in this repository
2. packaged template
   - installed as `skill-template`
   - used by `memory init` / `memory wizard` to copy the bundled skills into a repo
3. developer example skill
   - `docs/developer/examples/agent-example/skills/memory-layer/`
   - useful as a reference, but not the canonical current live skill

When these drift, the live repo-local bundle should be treated as authoritative.

## Bootstrap And Packaging

During repo bootstrap, `memory` copies the skill template into:

- `.agents/skills/`

The relevant logic lives in `crates/mem-cli/src/main.rs`:

- `discover_skill_template_dir()`
- `sync_memory_skill_bundle()`

The template is discovered from installed locations such as:

- `/usr/share/memory-layer/skill-template`
- local data directories
- or, during source/dev use, the repo-local `.agents/skills/`

This is why the same skill can exist in three forms:

- live repo-local bundle
- installed template copy
- documentation/example copy

## Related Docs

- [Memory Layer Skill](memory-layer-skill.md)
- [Architecture Overview](../architecture/overview.md)
- [How Memory Layer Works](../architecture/how-it-works.md)
- [Agent Example Skill](../examples/agent-example/skills/memory-layer/SKILL.md)
