# `memory demo`

Load a showcase project so query, resume, and the graph show something real on a fresh install.

```bash
memory demo
memory demo --project playground
```

Seeds the chosen project (default `demo`) with an embedded, privacy-safe corpus of memories about Memory Layer itself, then curates it. Works keyless — no LLM or embedding provider required. Requires a running service (`docker compose up` or `memory service run`). Idempotent: rerunning re-observes the same facts rather than duplicating them.

After loading, try:

```bash
memory query --project demo --question "How does reinforcement work?"
memory tour   # guided walkthrough of the three core commands
memory tui    # browse everything, including the memory graph
```

Do not seed demo data into a real project slug.

## Related Docs

- [Tour Command](tour.md)
- [Query Command](query.md)
- [Setup Command](setup.md)
