#!/usr/bin/env sh
# Seeds the deterministic fixture memories for memory-quality-v1.
# Point MEMORY_EVAL_MEMORY_CMD at the CLI + config for the target stack,
# e.g. "cargo run --bin memory --" for the dev profile.
set -eu

memory_cmd="${MEMORY_EVAL_MEMORY_CMD:-memory}"
payload="${TMPDIR:-/tmp}/memory-quality-seed.json"

cat > "$payload" <<'JSON'
{
  "project": "memory-quality-eval",
  "task_title": "Seed memory-quality benchmark facts",
  "user_prompt": "Create deterministic canary memories for memory-quality-v1.",
  "writer_id": "memory-quality-seed",
  "writer_name": "Memory Quality Seed",
  "agent_summary": "Seeded durable facts used by memory-quality-v1.",
  "files_changed": [],
  "tests": [],
  "notes": ["Synthetic ledger-bridge facts; only reachable through Memory."],
  "structured_candidates": [
    {
      "canonical_text": "Sync engine decision: the ledger sync engine commits batches with batched two-phase commit and a 250 ms flush window.",
      "summary": "Ledger sync engine uses batched two-phase commit with a 250 ms flush window.",
      "memory_type": "decision",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mq-decision", "mq-sync"],
      "sources": [{"source_kind": "note", "excerpt": "batched two-phase commit, 250 ms flush window"}]
    },
    {
      "canonical_text": "Export formats: the ledger bridge exports CSV and Parquet only. JSON export was evaluated and rejected for size reasons.",
      "summary": "Ledger bridge exports CSV and Parquet only.",
      "memory_type": "reference",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mq-reference", "mq-export"],
      "sources": [{"source_kind": "note", "excerpt": "CSV and Parquet only; JSON rejected"}]
    },
    {
      "canonical_text": "Error banner convention: a failed sync shows a slate banner with a retry action, and syncs are never auto-retried more than twice.",
      "summary": "Failed syncs show a slate banner; auto-retry at most twice.",
      "memory_type": "convention",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mq-convention", "mq-ui"],
      "sources": [{"source_kind": "note", "excerpt": "slate banner, retry action, at most twice"}]
    },
    {
      "canonical_text": "Incident QLT-9: duplicated ledger rows were caused by replaying the same batch after a timeout. The fix is idempotency keys on batch ids.",
      "summary": "QLT-9 duplicated rows fixed with idempotency keys on batch ids.",
      "memory_type": "incident",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mq-incident"],
      "sources": [{"source_kind": "note", "excerpt": "batch replay after timeout; idempotency keys"}]
    },
    {
      "canonical_text": "The reconciliation job runs every 20 minutes and writes its report to reports/reconcile.log.",
      "summary": "Reconciliation runs every 20 minutes, report at reports/reconcile.log.",
      "memory_type": "reference",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mq-reference", "mq-reconcile"],
      "sources": [{"source_kind": "note", "excerpt": "every 20 minutes; reports/reconcile.log"}]
    },
    {
      "canonical_text": "Schema changes require a migration note in the migrations doc and a paired rollback script.",
      "summary": "Schema changes need a migration note and a paired rollback script.",
      "memory_type": "convention",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mq-convention", "mq-migrations"],
      "sources": [{"source_kind": "note", "excerpt": "migration note plus rollback script"}]
    },
    {
      "canonical_text": "Rate limits: the bridge API allows 40 requests per minute per token and returns retry-after headers on 429 responses.",
      "summary": "Bridge API allows 40 requests per minute per token.",
      "memory_type": "reference",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mq-reference", "mq-api"],
      "sources": [{"source_kind": "note", "excerpt": "40 requests per minute per token"}]
    },
    {
      "canonical_text": "Storage decision: ledger snapshots live in content-addressed storage keyed by BLAKE3 digests.",
      "summary": "Ledger snapshots are content-addressed and keyed by BLAKE3 digests.",
      "memory_type": "decision",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mq-decision", "mq-storage"],
      "sources": [{"source_kind": "note", "excerpt": "content-addressed, BLAKE3 digests"}]
    },
    {
      "canonical_text": "The bridge gateway listens on port 7100.",
      "summary": "Bridge gateway listens on port 7100.",
      "memory_type": "reference",
      "confidence": 0.5,
      "importance": 2,
      "tags": ["mq-stale-port"],
      "sources": [{"source_kind": "note", "excerpt": "port 7100"}]
    },
    {
      "canonical_text": "The bridge gateway listens on port 7420 since the network rework.",
      "summary": "Bridge gateway listens on port 7420 since the network rework.",
      "memory_type": "reference",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mq-fresh-port"],
      "sources": [{"source_kind": "note", "excerpt": "port 7420 since the network rework"}]
    },
    {
      "canonical_text": "The benchmark database codename is Copper Kettle.",
      "summary": "Benchmark database codename is Copper Kettle.",
      "memory_type": "reference",
      "confidence": 0.5,
      "importance": 2,
      "tags": ["mq-stale-codename"],
      "sources": [{"source_kind": "note", "excerpt": "Copper Kettle"}]
    },
    {
      "canonical_text": "The benchmark database codename is Silver Anvil as of migration 12; earlier codenames are retired.",
      "summary": "Benchmark database codename is Silver Anvil as of migration 12.",
      "memory_type": "reference",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mq-fresh-codename"],
      "sources": [{"source_kind": "note", "excerpt": "Silver Anvil as of migration 12"}]
    },
    {
      "canonical_text": "Ledger exports are retained for 30 days.",
      "summary": "Ledger exports retained for 30 days.",
      "memory_type": "reference",
      "confidence": 0.5,
      "importance": 2,
      "tags": ["mq-stale-retention"],
      "sources": [{"source_kind": "note", "excerpt": "30 day retention"}]
    },
    {
      "canonical_text": "Ledger exports are retained for 90 days after the compliance retention update.",
      "summary": "Ledger exports retained for 90 days after the compliance update.",
      "memory_type": "reference",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mq-fresh-retention"],
      "sources": [{"source_kind": "note", "excerpt": "90 days after compliance update"}]
    },
    {
      "canonical_text": "Someone suggested the cache layer might eventually move to a managed key-value store; nothing was decided.",
      "summary": "Undecided suggestion about moving the cache layer to a managed key-value store.",
      "memory_type": "domain_fact",
      "confidence": 0.3,
      "importance": 1,
      "tags": ["mq-vague-cache"],
      "sources": [{"source_kind": "note", "excerpt": "maybe a managed key-value store someday"}]
    },
    {
      "canonical_text": "There was loose talk about maybe renaming the reconciliation job someday; no decision or new name exists.",
      "summary": "Loose talk about renaming the reconciliation job; nothing decided.",
      "memory_type": "domain_fact",
      "confidence": 0.3,
      "importance": 1,
      "tags": ["mq-vague-rename"],
      "sources": [{"source_kind": "note", "excerpt": "maybe rename the reconciliation job"}]
    },
    {
      "canonical_text": "Deploys used to run nightly on the legacy Jenkins pipeline before the platform migration; current CI is undocumented here.",
      "summary": "Legacy note: deploys formerly ran nightly on Jenkins.",
      "memory_type": "domain_fact",
      "confidence": 0.4,
      "importance": 2,
      "tags": ["mq-stale-deploy"],
      "sources": [{"source_kind": "note", "excerpt": "legacy Jenkins pipeline, pre-migration"}]
    },
    {
      "canonical_text": "The ingest path validates each record's schema before enqueue.",
      "summary": "Ingest validates schema before enqueue.",
      "memory_type": "reference",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mq-ingest-member"],
      "sources": [{"source_kind": "note", "excerpt": "schema validation before enqueue"}]
    },
    {
      "canonical_text": "The ingest path retries transient enqueue failures with capped backoff.",
      "summary": "Ingest retries transient failures with capped backoff.",
      "memory_type": "reference",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mq-ingest-member"],
      "sources": [{"source_kind": "note", "excerpt": "capped backoff retries"}]
    },
    {
      "canonical_text": "The ingest path emits a metric per accepted and rejected record.",
      "summary": "Ingest emits accept/reject metrics per record.",
      "memory_type": "reference",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mq-ingest-member"],
      "sources": [{"source_kind": "note", "excerpt": "per-record accept/reject metric"}]
    },
    {
      "canonical_text": "Ingest reliability overview: the ingest path forms one reliability pipeline that validates each record's schema before enqueue, retries transient enqueue failures with capped backoff, and emits accept and reject metrics per record. Taken together these give at-least-once ingest with observable back-pressure; the open gap is that schema-rejected records are dropped silently rather than dead-lettered.",
      "summary": "Ingest reliability pipeline: validation, backoff retries, and per-record metrics.",
      "memory_type": "insight",
      "confidence": 0.85,
      "importance": 4,
      "tags": ["insight", "consolidation", "mq-ingest-insight"],
      "sources": [{"source_kind": "memory", "excerpt": "consolidated from ingest reliability member memories"}]
    }
  ]
}
JSON

$memory_cmd capture task --file "$payload"
$memory_cmd curate --project memory-quality-eval
