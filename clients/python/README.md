# memory-layer-client (Python)

Typed Python client for the [Memory Layer](https://www.memory-layer.dev) HTTP API v1 — the frozen `x-stability: core` surface ([reference](https://www.memory-layer.dev/docs/reference/http-api)).

```bash
pip install -e clients/python   # from the repo; PyPI release pending
```

```python
from memory_layer import MemoryLayerClient

client = MemoryLayerClient()  # http://127.0.0.1:4040; token from MEMORY_API_TOKEN
client.remember("notes", title="Tried the client", summary="It speaks core v1.")
answer = client.query("notes", "What did I try?")
print(answer.answer, answer.citations)
```

Highlights: `query` (with `deterministic=True` for reproducible, keyless answers), `query_global`, `remember` (capture + bounded curate in one call), `memory`/`memory_history`, `project_memories`, `memory_graph` (includes decayed ACT-R activation per node), `resume`, `health`/`stats`. Unknown response fields are preserved on `.raw` — core v1 is additive-only, so the client never breaks on new fields.

Tests: `python -m unittest discover -s tests` (stub session, no network). See `examples/quickstart.ipynb`.
