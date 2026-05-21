# Content Model and Page Types

## Goal

Define repeatable page templates so the documentation site is consistent and easy to expand.

## Page Type 1 — Overview / Hub Page

Use for top-level sections.

```mdx
# Section name

One paragraph explaining what this section helps the reader do.

## Start here

<CardGroup cols={2}>
  <Card title="First task" href="/section/first-task" />
  <Card title="Second task" href="/section/second-task" />
</CardGroup>

## Common tasks

| Task | Page |
|---|---|
| Install X | Link |
| Configure Y | Link |

## Key concepts

Short bullets.

## Related

Links to reference and troubleshooting pages.
```

## Page Type 2 — Task Guide

Use for installation, configuration, setup, and troubleshooting.

```mdx
# Do the thing

## What you will do

Short outcome statement.

## Before you start

Prerequisites.

## Steps

### 1. First step

```bash
command here
```

Expected result.

## Verify

Commands and expected output.

## Troubleshooting

Common errors and links.

## Next

What to read next.
```

## Page Type 3 — Concept Page

Use for ideas such as memories, evidence, curation, retrieval, and trust.

```mdx
# Concept name

Short definition.

## Why it matters

Practical problem.

## How Memory Layer handles it

Explanation with diagram if useful.

## Example

Concrete example.

## Design trade-offs

What this solves and what it does not solve.

## Related

Links.
```

## Page Type 4 — Reference Page

Use for CLI, config, environment variables, schema, and API-like documentation.

```mdx
# Reference name

## Synopsis

```bash
memory command [flags]
```

## Description

What it does.

## Options

| Option | Required | Default | Description |
|---|---:|---|---|

## Examples

## Output

## Related commands
```

## Page Type 5 — Troubleshooting Page

Use for common failure modes.

```mdx
# Problem title

## Symptoms

What the user sees.

## Most likely cause

Short diagnosis.

## Fix

Commands.

## Verify

What success looks like.

## If it still fails

Next diagnostic steps.
```

## Page Type 6 — Evaluation Report Page

Use for benchmark reports.

```mdx
# Benchmark report name

## Summary

One paragraph.

## Setup

- Date
- Commit
- Dataset
- Variants
- Repeats
- Environment

## Results

| Metric | No memory | Full memory | Change |
|---|---:|---:|---:|

## Interpretation

What the result supports.

## Limits

What the result does not prove.

## Artifacts

Links to immutable outputs.

## Reproduce

Commands.
```

## Writing Rules

- Use action-oriented titles.
- Every guide needs a verification section.
- Every major page needs a clear next step.
- Keep claims bounded.
- Link concepts to tasks.
