# `memory up-to-speed`

`memory up-to-speed` creates a short briefing for a new or returning agent so it can start productive work without manually reading the full memory database.

It is broader than `memory resume`: resume is checkpoint-oriented, while up-to-speed is activity-oriented. It uses persisted activities, recent memory changes, useful durable memories, commits, warnings, and token-count summaries.

## Common Usage

```bash
memory up-to-speed --project memory
memory up-to-speed --project memory --text
memory up-to-speed --project memory --llm
```

JSON is the default for agent workflows. Use `--text` for a concise human-readable briefing.

## What It Uses

- persisted project activity events, excluding prior briefing events to avoid recursive noise
- project health and warnings such as uncurated captures, unhealthy watchers, missing embeddings, and pending replacement proposals
- recent commits and recently changed memories
- durable context memories selected from the active project
- token usage attached to recent activities when providers report it

The deterministic briefing works without LLM configuration. `--llm` asks the configured OpenAI-compatible model to rewrite the same evidence into a concise briefing; if the model fails, the deterministic briefing is still returned by the backend.

## Token Counts

Token cost in v1 means token counts, not estimated money.

When available, activity metadata separates:

- input tokens
- output tokens
- cache-read tokens
- cache-write tokens
- total tokens

Providers that do not return usage metadata simply leave those fields empty.

## Related Docs

- [Activities](activities.md)
- [Resume Briefings](resume.md)
- [Activity Tab](../tui/activity.md)
