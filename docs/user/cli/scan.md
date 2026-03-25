# `mem-cli scan`

`mem-cli scan` is a repository bootstrap command.

Its job is to inspect an existing codebase, ask an LLM for durable project knowledge, validate the result, and then write that knowledge into Memory Layer through the normal capture and curate pipeline.

This is not a generic "summarize my repo" command. It is specifically trying to extract project memory that should still be useful later.

## Table of Contents

- [What It Does](#what-it-does)
- [What It Reads](#what-it-reads)
- [What It Reads From Git](#what-it-reads-from-git)
- [What It Sends To The LLM](#what-it-sends-to-the-llm)
- [What It Accepts From The LLM](#what-it-accepts-from-the-llm)
- [How It Writes Memory](#how-it-writes-memory)
- [Idempotency](#idempotency)
- [Dry Run Mode](#dry-run-mode)
- [Scan Reports](#scan-reports)
- [Configuration Requirements](#configuration-requirements)
- [Current Limits And Defaults](#current-limits-and-defaults)
- [What `scan` Is Good At](#what-scan-is-good-at)
- [What `scan` Is Not Good At](#what-scan-is-not-good-at)
- [Practical Workflow](#practical-workflow)
- [Troubleshooting](#troubleshooting)

## What It Does

At a high level, `scan` does this:

1. load the current repo and project context
2. read a curated subset of repository files
3. read a bounded amount of recent git history
4. build a structured dossier from that material
5. send the dossier to an OpenAI-compatible chat model
6. require strict JSON back
7. validate and deduplicate the returned candidates
8. write them as a normal Memory Layer capture
9. run curation so the resulting memories become searchable

So `scan` is really:

- repository sampling
- LLM extraction
- strict validation
- normal Memory Layer ingestion

It does not bypass the existing backend or write directly to PostgreSQL tables.

## What It Reads

`scan` does not read every file in the repository.

It chooses a bounded set of high-value files using a scoring heuristic.

The current implementation prefers:

- `README*`
- files under `docs/`
- top-level manifests such as `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`
- main Rust entrypoints like `crates/*/src/main.rs` and `crates/*/src/lib.rs`
- files under `src/`
- files under `scripts/`
- files under `packaging/`
- files under `.agents/skills/`
- common config and service files such as `.toml`, `.md`, `.yaml`, `.yml`, `.json`, `.sh`, `.service`

It skips obvious low-value or noisy paths such as:

- `.git/`
- `target/`
- `.mem/`
- `node_modules/`

Implementation limits:

- up to `18` repository files
- up to `8_000` bytes per file after normalization
- file content budget is roughly `70%` of the configured LLM input budget

## What It Reads From Git

`scan` also reads recent git history because important architecture and workflow knowledge often lives in commit history rather than only in current files.

The current implementation:

- reads up to `20` non-merge commits
- captures commit hash
- captures commit timestamp
- captures subject
- captures trimmed body
- captures up to `12` changed paths per commit

You can bound this with:

```bash
mem-cli scan --since "2 weeks ago"
```

or:

```bash
mem-cli scan --since "2026-03-01"
```

## What It Sends To The LLM

The CLI builds a structured dossier with:

- project slug
- canonical repo root
- current `HEAD` commit if available
- selected file contents
- selected git commits

It then sends that dossier to the configured OpenAI-compatible chat endpoint with a system prompt that tells the model to:

- extract durable repository memory
- return strict JSON
- keep candidates concise and repo-specific
- avoid speculative claims
- avoid transient task notes
- attach provenance through files and/or commits

The requested JSON shape is:

- `summary`
- `candidates[]`

Each candidate is expected to include:

- `canonical_text`
- `summary`
- `memory_type`
- `confidence`
- `importance`
- `tags`
- `provenance_files`
- `provenance_commits`
- `rationale`

## What It Accepts From The LLM

The LLM output is not trusted blindly.

The current validation step rejects candidates when:

- `canonical_text` is empty
- `summary` is empty
- both file provenance and commit provenance are missing
- the candidate is a duplicate of an earlier candidate in the same scan

It also normalizes:

- candidate text
- tags
- confidence
- importance

The current hard cap is:

- at most `12` accepted candidates per scan

If validation produces zero acceptable candidates, `scan` fails instead of writing low-quality memory.

## How It Writes Memory

Accepted candidates are converted into a normal `CaptureTaskRequest`.

That request contains:

- `task_title = "Repository scan for <project>"`
- a scan-specific `user_prompt`
- the LLM-generated summary as `agent_summary`
- the selected repo files as `files_changed`
- a condensed git summary in `git_diff_summary`
- the validated candidates as `structured_candidates`

Then `scan` does exactly what a normal high-level write should do:

1. call `capture-task`
2. call `curate`

That means scan output goes through the same:

- backend validation
- provenance rules
- curation rules
- search chunk generation
- activity streaming

## Idempotency

`scan` generates an idempotency key so rerunning it on the same repo state does not create uncontrolled duplicate raw captures.

The key is currently based on:

- prompt version
- project slug
- current `HEAD`
- selected file paths and contents
- selected commit hashes

This means:

- rerunning an unchanged scan tends to collapse to the same raw capture
- changing important files or commits produces a new scan capture

## Dry Run Mode

Use this first if you want to inspect what `scan` is going to do:

```bash
mem-cli scan --project my-project --dry-run
```

In dry-run mode, `scan` still:

- reads files
- reads git history
- calls the LLM
- validates candidates
- writes a scan report

But it does **not**:

- create a capture
- run curation
- write project memory

## Scan Reports

Every scan writes a local report under:

- `.mem/runtime/scan/`

The report includes:

- prompt version
- project
- whether it was a dry run
- summary
- how many files were considered
- how many commits were considered
- the dossier that was sent
- the accepted candidates

This is useful for debugging why a scan produced the memory it did.

## Configuration Requirements

`scan` requires working LLM configuration.

Today that means:

- `[llm].provider = "openai_compatible"`
- `[llm].base_url`
- `[llm].model`
- `[llm].api_key_env`
- the API key available in:
  - process environment
  - `.mem/memory-layer.env`
  - shared `memory-layer.env`

If these are not present, `scan` fails before doing any repository work.

## Current Limits And Defaults

Important implementation details:

- only `openai_compatible` providers are supported today
- the request goes to `POST /chat/completions`
- `response_format` is forced to JSON object
- `temperature` is sent first, then omitted on retry if the model rejects it
- `max_completion_tokens` comes from `[llm].max_output_tokens`

Current fixed limits:

- `MAX_FILES = 18`
- `MAX_COMMITS = 20`
- `MAX_FILE_BYTES = 8_000`
- `MAX_CANDIDATES = 12`

These are implementation limits, not user-facing flags.

## What `scan` Is Good At

`scan` works best for extracting:

- architecture facts
- major functionality
- durable conventions
- setup and environment facts
- repo-specific workflow knowledge

It is especially useful when onboarding Memory Layer to an existing project that already has a lot of knowledge spread across README files, docs, and git history.

## What `scan` Is Not Good At

`scan` is not currently a full repository analyzer.

It does **not**:

- read every file
- execute code
- run tests
- infer runtime behavior from actual execution
- inspect issue trackers or external systems
- use vector search or embeddings
- guarantee that every accepted candidate is correct just because the model produced it

It is only as good as:

- the selected file set
- the selected git history
- the configured model
- the validation and curation pipeline

## Practical Workflow

Recommended usage:

1. initialize the repo with `mem-cli wizard` or `mem-cli init`
2. make sure `[llm]` is configured
3. run a dry run first
4. inspect the generated report
5. run the real scan
6. open the TUI and inspect the resulting memories

Example:

```bash
mem-cli scan --project my-project --dry-run
mem-cli scan --project my-project
mem-cli tui --project my-project
```

## Troubleshooting

Common failure cases:

- missing `[llm].model`
- missing API key
- wrong model name
- unsupported model parameters
- no valid durable candidates returned

Useful checks:

```bash
mem-cli doctor
mem-cli scan --project my-project --dry-run
```

If you are debugging a specific scan result, the most useful artifact is the JSON report in `.mem/runtime/scan/`.

## Related Docs

- [Getting Started](../getting-started.md)
- [Commit History](commits.md)
- [How Memory Layer Works](../../developer/architecture/how-it-works.md)
