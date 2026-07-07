# `memory tour`

Guided first-run walkthrough of the three core commands, with their real output.

```bash
memory tour
memory tour --project playground
```

The tour seeds the showcase corpus (same as [`memory demo`](demo.md)), then actually runs the whole daily loop against it:

1. `memory remember` — records that you took the tour as a real memory.
2. `memory query` — asks a question only project memory can answer and shows the cited response.
3. `memory resume` — builds a re-entry briefing for the project.

When run interactively it pauses between steps; piped or scripted runs continue without blocking. Requires a running service.

The honest message the tour lands: **remember, query, and resume are the whole daily loop** — everything else is optional depth.

## Related Docs

- [Demo Command](demo.md)
- [Remember Command](remember.md)
- [Query Command](query.md)
- [Resume Command](resume.md)
