# Scores Command

`memory scores` shows reinforcement activation scores for a project's
memories: how strongly each memory has been used recently (query
retrievals, answer citations, direct reads, and activation spread from
linked memories), with time decay.

Use it to see which memories are "hot" (and heading for validation), which
are flagged for review, and how volatile each memory's underlying sources
are.

## Usage

```bash
memory scores --project memory
memory scores --project memory --needs-review
memory scores --project memory --limit 50 --json
```

## Columns

- `ACTIVATION` — decay-corrected activation. Crossing
  `reinforcement.validation_threshold` makes the memory due for validation.
- `ACCESS` / `CITED` — direct access count and answer-citation count.
- `VOLAT` — EWMA of provenance change events per day; higher volatility
  shortens the revalidation interval.
- `VALIDATED` — date of the last successful validation.
- Memories flagged by validation show `[NEEDS REVIEW]` and rank lower in
  query results until resolved with `memory review`.

See `docs/developer/architecture/memory-reinforcement.md` for the scoring
model and tuning knobs.
