# Query Contract

## Purpose

This document defines the expected input, output, behavior, and failure modes for project-memory queries.

The goal is to make `memctl query` predictable for:
- Codex Skill scripts
- humans using the CLI directly
- backend API clients

---

## Query Flow Summary

1. Caller provides a project and natural-language question.
2. Backend performs PostgreSQL BM25 retrieval over canonical memory and chunks.
3. Backend groups and ranks results.
4. Backend returns a concise answer summary, ranked results, provenance, and confidence.

---

## CLI Contract

### Command

```bash
memctl query --project <project-slug> --question "<question>"
```

### Required Inputs
- `--project`: logical project slug
- `--question`: natural-language question

### Optional Inputs
- `--type <memory_type>` repeatable
- `--tag <tag>` repeatable
- `--limit <n>` default 8
- `--json` machine-readable output
- `--min-confidence <0..1>` optional filtering threshold

### Exit Codes
- `0`: success
- `2`: invalid input
- `3`: backend unavailable
- `4`: no results / insufficient evidence
- `5`: unexpected internal error

---

## HTTP API Contract

### Endpoint

`POST /v1/query`

### Request Body

```json
{
  "project": "my-project",
  "query": "How do we handle JWT refresh token rotation?",
  "filters": {
    "types": ["architecture", "decision"],
    "tags": ["auth"]
  },
  "top_k": 8
}
```

### Validation Rules
- `project` must be non-empty
- `query` must be non-empty
- `top_k` must be within a safe range, for example `1..=50`
- unknown memory types should be rejected with `400`

---

## Response Contract

### Success Response

```json
{
  "answer": "Refresh tokens are single-use and rotated on renewal.",
  "confidence": 0.84,
  "results": [
    {
      "memory_id": "mem_123",
      "summary": "JWT refresh token rotation",
      "memory_type": "architecture",
      "score": 14.22,
      "snippet": "Refresh tokens are invalidated after successful rotation...",
      "tags": ["auth", "jwt"],
      "sources": [
        {
          "task_id": "task_42",
          "file_path": "auth/refresh.rs",
          "source_kind": "file"
        }
      ]
    }
  ],
  "insufficient_evidence": false
}
```

### Success Fields
- `answer`: concise synthesis from retrieved results
- `confidence`: `0.0..1.0`
- `results`: ranked supporting entries
- `insufficient_evidence`: explicit signal for weak or absent memory support

### Result Fields
- `memory_id`: canonical memory identifier
- `summary`: short description
- `memory_type`: one of the defined memory types
- `score`: final ranking score
- `snippet`: query-relevant excerpt
- `tags`: matched or associated tags
- `sources`: provenance records

---

## Insufficient Evidence Contract

When retrieval does not support a confident answer, return a structured insufficient-evidence response instead of bluffing.

Example:

```json
{
  "answer": "I could not find enough project memory to answer confidently.",
  "confidence": 0.19,
  "results": [],
  "insufficient_evidence": true
}
```

Rules:
- do not fabricate an answer summary from unrelated matches
- prefer low confidence plus explicit insufficiency
- still include weak matches only if they are genuinely relevant

---

## Ranking Expectations

The backend should combine:
- PostgreSQL BM25 score
- exact phrase or close lexical boost
- tag/type boosts
- importance
- confidence
- light recency weighting

The exact formula can evolve, but the contract requires:
- higher-ranked results must be explainable
- ranking must be deterministic for identical data and query input
- score ordering must be stable

---

## Provenance Requirements

Every returned result must include provenance when available.

At minimum, provenance should link back to one or more of:
- task ID
- file path
- git commit
- source kind
- source excerpt

If provenance is unavailable, the result should be ranked lower and flagged internally.

---

## Output Modes

### Human-readable mode
Default CLI mode should be concise and skimmable:
- answer first
- then top results
- then provenance highlights

### JSON mode
`--json` must emit structured output suitable for scripts.

Requirements:
- no extra prose
- stable field names
- valid UTF-8 JSON

---

## Failure Modes

### Invalid input
Return `400` from API and exit code `2` from CLI.

### Backend unavailable
Return `503` from API and exit code `3` from CLI.

### No results
Return `200` with `insufficient_evidence=true` from API and exit code `4` from CLI only if the CLI is being used in strict mode or chooses to distinguish this condition.

### Internal errors
Return `500` from API and exit code `5` from CLI.

---

## Example CLI Human Output

```text
Answer:
Refresh tokens are single-use and rotated on renewal.

Confidence: 0.84

Top results:
1. JWT refresh token rotation [architecture] score=14.22
   Refresh tokens are invalidated after successful rotation...
   Source: auth/refresh.rs (task_42)
```

---

## Example CLI JSON Output

```json
{
  "answer": "Refresh tokens are single-use and rotated on renewal.",
  "confidence": 0.84,
  "results": [
    {
      "memory_id": "mem_123",
      "summary": "JWT refresh token rotation",
      "memory_type": "architecture",
      "score": 14.22,
      "snippet": "Refresh tokens are invalidated after successful rotation...",
      "tags": ["auth", "jwt"],
      "sources": [
        {
          "task_id": "task_42",
          "file_path": "auth/refresh.rs",
          "source_kind": "file"
        }
      ]
    }
  ],
  "insufficient_evidence": false
}
```

---

## Notes for Skill Authors

When Codex uses query results:
- treat the answer as evidence-backed project context, not as infallible truth
- mention uncertainty when confidence is low
- use provenance when relevant in the final response
- if the query contract reports insufficient evidence, say so clearly
