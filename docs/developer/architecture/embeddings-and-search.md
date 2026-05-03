# Embeddings and Search

How Memory Layer turns project memories into vectors, stores them, and
uses them to answer questions. Pair with
[Architecture Overview](overview.md) for the wider picture and
[How Memory Layer Works](how-it-works.md) for the end-to-end runtime
model.

## Table of Contents

- [What gets embedded](#what-gets-embedded)
- [How vectors are produced](#how-vectors-are-produced)
- [How vectors are stored](#how-vectors-are-stored)
- [How the search path uses embeddings](#how-the-search-path-uses-embeddings)
- [Maintenance surfaces](#maintenance-surfaces)
- [File map](#file-map)

## What gets embedded

Each memory is split into **overlapping chunks** — 320 characters
target size, 80-character overlap, broken on word boundaries. The
chunker runs over `summary + "\n" + canonical_text`, not just the
body, so the summary is visible to every chunk the embedder sees.

For each chunk we store a `search_text` of the form
`"{summary}\n{chunk_text}"` in `memory_chunks`. That is the string
the embedding model actually receives — the summary contributes to
every chunk's vector, which helps short memories rank well even when
only their body matches a query.

Code: `split_search_chunks` in `crates/mem-search/src/lib.rs` at
line 1344; called from `rebuild_memory_chunks` at line 290.

## How vectors are produced

`EmbeddingService` (`crates/mem-search/src/lib.rs`) is a thin wrapper
around a trait object: `Arc<dyn EmbeddingBackend>`. Each backend
lives in `crates/mem-search/src/embedding_backend.rs` and speaks its
provider's native REST dialect. The service chooses a backend from
`config.embeddings.provider`:

| Provider string | Backend | Default `base_url` | Auth | Notes |
|---|---|---|---|---|
| `openai` | `OpenAiBackend` | `https://api.openai.com/v1` | `Authorization: Bearer` | Uses OpenAI's embeddings API, sends `encoding_format = "float"`, and supports optional `dimensions`. Ignores document/query distinction. |
| `openai_compatible` | `OpenAiBackend` | `https://api.openai.com/v1` | `Authorization: Bearer` | For hosted/proxy APIs that mimic OpenAI's `/embeddings` shape. OpenAI-only options are omitted for compatibility. |
| `ollama` | `OpenAiBackend` | `http://127.0.0.1:11434/v1` | none by default | First-class local Ollama support using Ollama's OpenAI-compatible `/embeddings` endpoint. Set `api_key_env` only when a proxy requires auth. |
| `voyage` | `VoyageBackend` | `https://api.voyageai.com` | `Authorization: Bearer` | Anthropic's recommended partner. Uses `input_type: document` at index time, `query` at query time. |
| `cohere` | `CohereBackend` | `https://api.cohere.com` | `Authorization: Bearer` | Posts to `/v2/embed`. Uses `input_type: search_document` / `search_query`. Response carries vectors under `embeddings.float`. |
| `gemini` | `GeminiBackend` | `https://generativelanguage.googleapis.com/v1beta` | `x-goog-api-key` | Model goes in the URL path (`/models/{model}:batchEmbedContents`) and is `"models/{model}"`-qualified in the body. Uses `taskType: RETRIEVAL_DOCUMENT` / `RETRIEVAL_QUERY`. |

All remote backends take their API key from the env var named in
`embeddings.api_key_env` (`OPENAI_API_KEY`, `VOYAGE_API_KEY`,
`COHERE_API_KEY`, `GEMINI_API_KEY` are common choices).
For `provider = "ollama"`, `api_key_env = ""` is valid and no
`Authorization` header is sent.

Every `embed_texts` call also carries an `EmbeddingPurpose`:

- `Document` — for chunks we're storing.
- `Query` — for the single embedding produced when a user asks a
  question.

OpenAI ignores the purpose; Voyage, Cohere, and Gemini each map it
onto their native "are you indexing or searching?" hint, which
measurably improves retrieval quality on those providers.

The identity of a vector space is derived from the config:

```
space_key = "{provider}|{base_url}|{model}"
```

Swapping the model or the base URL produces a new space key; existing
vectors under the old key are not overwritten. This lets you run two
spaces side by side during a migration.

### Triggers that produce vectors

- `/v1/curate` — right after curation inserts or replaces memories,
  `rebuild_memory_chunks` runs and embeds the new chunks.
- `/v1/reindex` — rebuilds chunks for every memory in a project.
- Bundle import — rebuilds chunks for each imported memory after
  the writes land.
- `/v1/reembed` — walks existing chunks that are missing a vector in
  the currently active space and embeds only those.

## How vectors are stored

Two tables carry the chunk and the vectors.

### `memory_chunks`

Holds the chunk itself plus a Postgres tsvector for FTS. Defined in
migration `0001_init.sql`; pgvector extension and a legacy single-
vector column are added in `0004_pgvector.sql`:

```sql
CREATE EXTENSION IF NOT EXISTS vector;
ALTER TABLE memory_chunks
    ADD COLUMN IF NOT EXISTS embedding vector;
CREATE INDEX IF NOT EXISTS idx_memory_chunks_embedding_hnsw
    ON memory_chunks USING hnsw (embedding vector_cosine_ops);
```

The HNSW index on `memory_chunks.embedding` is kept for backward
compatibility with deployments that predate migration `0009`.

### `memory_chunk_embeddings`

The current source of truth for vectors, added in
`0009_multi_embedding_spaces.sql`:

```sql
CREATE TABLE IF NOT EXISTS memory_chunk_embeddings (
    chunk_id UUID NOT NULL REFERENCES memory_chunks(id) ON DELETE CASCADE,
    embedding_space TEXT NOT NULL,
    embedding vector NOT NULL,
    embedding_provider TEXT,
    embedding_base_url TEXT,
    embedding_model TEXT,
    embedding_dimension INTEGER,
    embedding_updated_at TIMESTAMPTZ,
    PRIMARY KEY (chunk_id, embedding_space)
);
CREATE INDEX IF NOT EXISTS idx_memory_chunk_embeddings_space
    ON memory_chunk_embeddings (embedding_space);
```

Inserts are `ON CONFLICT (chunk_id, embedding_space) DO UPDATE`, so
re-embedding is idempotent. A single chunk can carry vectors for
multiple spaces concurrently, which is the mechanism for zero-
downtime model migrations.

At current scale the multi-space table doesn't have its own HNSW
index; semantic lookups do a filtered scan instead. That is cheap
today and is the first thing to revisit if you grow to 100k+ chunks.

Code: `upsert_chunk_embedding` in
`crates/mem-search/src/lib.rs:901`.

## How the search path uses embeddings

`query_memory` at `crates/mem-search/src/lib.rs:159` runs a lexical
and a semantic pass in parallel, merges them, then re-ranks.

### Lexical pass

`fetch_lexical_candidates` (line 596) — `websearch_to_tsquery` + GIN
match against `memory_entries.search_document` plus per-chunk
`ts_rank_cd(mc.tsv, query)`. Pure Postgres FTS, no embedding needed.

### Semantic pass

`fetch_semantic_candidates` (line 764) — embeds the user's question
once, then runs:

```sql
SELECT ..., (mce.embedding <=> $6) AS cosine_distance
FROM memory_chunks mc
JOIN memory_chunk_embeddings mce ON mce.chunk_id = mc.id
JOIN memory_entries m ON m.id = mc.memory_entry_id
JOIN projects p ON p.id = m.project_id
WHERE p.slug = $1
  AND m.status = 'active'
  AND (
        $8::boolean
     OR (m.is_tombstone = FALSE
         AND m.version_no = (
             SELECT MAX(m2.version_no)
             FROM memory_entries m2
             WHERE m2.canonical_id = m.canonical_id
         ))
  )
  AND mce.embedding_space = $4
  AND mce.embedding_dimension = $5
  AND ($2::text[] IS NULL OR m.memory_type = ANY($2))
  AND (cardinality($3::text[]) = 0 OR EXISTS (
          SELECT 1 FROM memory_tags mt
          WHERE mt.memory_entry_id = m.id AND mt.tag = ANY($3)
  ))
ORDER BY cosine_distance ASC, m.updated_at DESC, m.id
LIMIT $7
```

`<=>` is pgvector's cosine-distance operator. The `$8::boolean`
parameter is `QueryRequest.history`; when false (the default) only
the latest non-tombstone version of each canonical memory
contributes. Results are collapsed to "best chunk per memory" before
merge.

### Merge and re-rank

`merge_candidates` (line 972) takes the union of the two passes,
keeping the best score per memory. `rank_candidate` (line 1030)
computes the final score by weighted sum:

| Signal | Weight |
|---|---|
| chunk FTS | 4.0 |
| entry FTS | 2.5 |
| semantic similarity | 4.2 |
| exact phrase hit | 1.4 |
| tag / path / relation boosters | varies |
| recency | 14-day half-life |

Semantic similarity carries the largest single coefficient, which is
why the system keeps useful recall even when lexical overlap is
thin. After ranking, the top-N are sent to the LLM for the final
answer-writing pass.

## Maintenance surfaces

### `/v1/reembed` → `reembed_project_chunks`
(`crates/mem-search/src/lib.rs:370`)

Finds chunks that have no embedding in the currently active space
and embeds their `search_text`. Idempotent. Run after:

- first-time embedding rollout,
- a model change (new `space_key`),
- adding a new provider alongside an existing one.

The query filters `m.is_tombstone = FALSE`, so deleted memories
never get embeddings even if their `memory_chunks` rows are still
around.

### `/v1/prune-embeddings` → `prune_project_embeddings`
(`crates/mem-search/src/lib.rs:444`)

```sql
DELETE FROM memory_chunk_embeddings mce
USING memory_chunks mc, memory_entries m, projects p
WHERE mce.chunk_id = mc.id
  AND mc.memory_entry_id = m.id
  AND m.project_id = p.id
  AND p.slug = $1
  AND m.status = 'active'
  AND m.is_tombstone = FALSE
  AND mce.embedding_space <> $2
```

Drops vectors from retired spaces so the table doesn't keep growing
after a model migration. Run after you've confirmed the new space is
fully populated and queries against it return reasonable results.

### Model migration recipe

1. Update `embeddings.model` (or `base_url`, or `provider`) in
   `memory-layer.toml`. A new `space_key` is implicit.
2. Restart the service (picks up the new config).
3. Hit `/v1/reembed` to populate vectors in the new space for every
   active, non-tombstone chunk. Both spaces now coexist.
4. Verify recall on the new space with `memory query` against a
   known-good question.
5. Hit `/v1/prune-embeddings` to remove the old space.

## File map

| What | Where |
|---|---|
| `EmbeddingService`, `embed_texts`, `EmbeddingPurpose` | `crates/mem-search/src/lib.rs` + `embedding_backend.rs` |
| `EmbeddingBackend` trait + OpenAI/Voyage/Cohere/Gemini impls | `crates/mem-search/src/embedding_backend.rs` |
| `split_search_chunks` (chunker) | `crates/mem-search/src/lib.rs:1344` |
| `rebuild_memory_chunks` | `crates/mem-search/src/lib.rs:290` |
| `upsert_chunk_embedding` | `crates/mem-search/src/lib.rs:901` |
| `query_memory` (entry point) | `crates/mem-search/src/lib.rs:159` |
| `fetch_lexical_candidates` | `crates/mem-search/src/lib.rs:596` |
| `fetch_semantic_candidates` | `crates/mem-search/src/lib.rs:764` |
| `merge_candidates`, `rank_candidate` | `crates/mem-search/src/lib.rs:972, 1030` |
| `reembed_project_chunks` | `crates/mem-search/src/lib.rs:370` |
| `prune_project_embeddings` | `crates/mem-search/src/lib.rs:444` |
| `memory_chunks` table | `migrations/0001_init.sql`, `0004_pgvector.sql` |
| `memory_chunk_embeddings` table | `migrations/0009_multi_embedding_spaces.sql` |
| Version filter (`is_tombstone`, `version_no`) | `migrations/0013_memory_versions.sql` |
