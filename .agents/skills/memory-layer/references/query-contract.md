# Query Contract

`memctl query --project <slug> --question "<text>"` sends `POST /v1/query` and expects:
- `answer`
- `confidence`
- `results`
- `insufficient_evidence`

If evidence is weak, the backend returns `insufficient_evidence=true` instead of fabricating an answer.
