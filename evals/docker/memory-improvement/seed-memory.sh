#!/usr/bin/env sh
set -eu

memory_cmd="${MEMORY_EVAL_MEMORY_CMD:-/workspace/target/debug/memory --config /workspace/evals/docker/memory-improvement/config.eval.toml}"
payload="/tmp/memory-improvement-seed.json"

cat > "$payload" <<'JSON'
{
  "project": "memory",
  "task_title": "Seed Memory improvement benchmark facts",
  "user_prompt": "Create deterministic benchmark memories for Memory improvement v1.",
  "writer_id": "memory-improvement-seed",
  "writer_name": "Memory Improvement Seed",
  "agent_summary": "Seeded durable facts used by memory-improvement-v1.",
  "files_changed": [],
  "tests": [],
  "notes": ["These benchmark facts are intentionally available only through Memory."],
  "structured_candidates": [
    {
      "canonical_text": "Current benchmark codename decision: use Verdant Ledger for the Memory improvement benchmark. The old codename Blue Notebook is superseded and should only appear when explaining stale decisions.",
      "summary": "Verdant Ledger is the current benchmark codename.",
      "memory_type": "decision",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mi-deductive", "mi-codename", "mi-superseded"],
      "sources": [{"file_path": "docs/benchmark-codename.md", "source_kind": "file", "excerpt": "Verdant Ledger replaces Blue Notebook."}]
    },
    {
      "canonical_text": "Release gate rule: a Memory release can be described as ready only after the green gate passes for the paired benchmark. Manual screenshots only are not sufficient release evidence.",
      "summary": "Release readiness requires the green gate on the paired benchmark.",
      "memory_type": "reference",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mi-deductive", "mi-release"],
      "sources": [{"file_path": "docs/release-gate.md", "source_kind": "file", "excerpt": "green gate and paired benchmark required"}]
    },
    {
      "canonical_text": "UI convention examples for Memory benchmark pages use a paper ledger surface, a forest accent, calm editorial spacing, and avoid the purple default.",
      "summary": "Memory benchmark pages use paper ledger and forest accent styling.",
      "memory_type": "convention",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mi-inductive", "mi-ui"],
      "sources": [{"file_path": "docs/ui-conventions.md", "source_kind": "file", "excerpt": "paper ledger, forest accent, no purple default"}]
    },
    {
      "canonical_text": "Incident RIFT-27: an amber retry banner that never clears is usually caused by stale retry state. The fix is to clear stale retry state before re-running the status poll; do not delete the database.",
      "summary": "RIFT-27 explains the amber retry banner and fix.",
      "memory_type": "incident",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mi-abductive", "mi-incident"],
      "sources": [{"file_path": "docs/incidents/amber-retry.md", "source_kind": "file", "excerpt": "RIFT-27 clear stale retry state"}]
    },
    {
      "canonical_text": "The continuity fixture must stay local-only and dependency-free. Do not add external CDNs, package managers, remote fonts, or hosted scripts.",
      "summary": "Continuity fixture is local-only and dependency-free.",
      "memory_type": "convention",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mi-deductive", "mi-local"],
      "sources": [{"file_path": "docs/local-only.md", "source_kind": "file", "excerpt": "local-only dependency-free fixture"}]
    },
    {
      "canonical_text": "Graph evidence for the continuity query workflow is anchored in crates/mem-search/src/lib.rs. Interpret graph-aware retrieval as related files and graph candidates augmenting normal lexical and semantic results.",
      "summary": "Graph-aware retrieval uses related files and graph candidates.",
      "memory_type": "reference",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mi-graph", "mi-query"],
      "sources": [{"file_path": "crates/mem-search/src/lib.rs", "source_kind": "file", "excerpt": "query workflow graph evidence"}]
    },
    {
      "canonical_text": "Memory feature search should follow the existing input event interaction pattern: filter visible feature cards as the user types, keep controls keyboard accessible, and avoid reloads.",
      "summary": "Feature search uses an input event filtering pattern.",
      "memory_type": "convention",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mi-inductive", "mi-ui"],
      "sources": [{"file_path": "docs/ui-conventions.md", "source_kind": "file", "excerpt": "input event filter pattern"}]
    },
    {
      "canonical_text": "The Memory improvement benchmark reports four evidence categories: retrieval, grounded answer, resume, and coding continuity.",
      "summary": "Benchmark evidence categories are retrieval, grounded answer, resume, and coding continuity.",
      "memory_type": "reference",
      "confidence": 0.95,
      "importance": 3,
      "tags": ["mi-deductive", "mi-evidence"],
      "sources": [{"file_path": "docs/evaluation/benchmark-method.md", "source_kind": "file", "excerpt": "retrieval grounded answer resume coding continuity"}]
    },
    {
      "canonical_text": "The benchmark uses three reasoning modes from the paper: deductive, inductive, and abductive. Results should be grouped by reasoning mode and memory capability.",
      "summary": "Benchmark groups results by deductive, inductive, and abductive reasoning.",
      "memory_type": "reference",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mi-deductive", "mi-paper"],
      "sources": [{"file_path": "docs/evaluation/reasoning-taxonomy.md", "source_kind": "file", "excerpt": "deductive inductive abductive"}]
    },
    {
      "canonical_text": "Optional hybrid judging scores evidence use, reasoning quality, consistency, and maintainability. The judge is diagnostic; deterministic checks remain authoritative for pass/fail.",
      "summary": "Hybrid judge dimensions are evidence use, reasoning quality, consistency, and maintainability.",
      "memory_type": "reference",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mi-abductive", "mi-judge"],
      "sources": [{"file_path": "docs/evaluation/hybrid-judge.md", "source_kind": "file", "excerpt": "evidence use reasoning quality consistency maintainability"}]
    },
    {
      "canonical_text": "Benchmark artifacts should be written under target/memory-evals. Important files include run JSON artifacts, comparison.json, markdown reports, prompts, command output, Memory evidence, and token usage.",
      "summary": "Benchmark artifacts live under target/memory-evals.",
      "memory_type": "reference",
      "confidence": 0.95,
      "importance": 3,
      "tags": ["mi-deductive", "mi-artifacts"],
      "sources": [{"file_path": "docs/evaluation/artifacts.md", "source_kind": "file", "excerpt": "target/memory-evals comparison.json"}]
    },
    {
      "canonical_text": "Run the Memory improvement benchmark in Docker with docker compose using the memory-improvement eval stack. It starts Postgres with pgvector, the service, seed memories, and the eval runner.",
      "summary": "Docker run uses the memory-improvement compose stack.",
      "memory_type": "environment",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mi-deductive", "mi-docker"],
      "sources": [{"file_path": "evals/docker/memory-improvement/compose.yml", "source_kind": "file", "excerpt": "docker compose memory-improvement"}]
    },
    {
      "canonical_text": "Failed benchmark items should be diagnosed by category: retrieval miss, stale memory, weak grounded answer, missing graph candidates, or coding regression.",
      "summary": "Benchmark failures are categorized for diagnosis.",
      "memory_type": "reference",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mi-abductive", "mi-failures"],
      "sources": [{"file_path": "docs/evaluation/failures.md", "source_kind": "file", "excerpt": "retrieval miss stale memory"}]
    },
    {
      "canonical_text": "Accessibility conventions for the continuity app: use aria labels where controls change content, preserve keyboard operation, and add responsive CSS with @media.",
      "summary": "Continuity app accessibility requires aria labels and responsive CSS.",
      "memory_type": "convention",
      "confidence": 0.9,
      "importance": 3,
      "tags": ["mi-inductive", "mi-a11y"],
      "sources": [{"file_path": "docs/ui-conventions.md", "source_kind": "file", "excerpt": "aria labels keyboard @media"}]
    },
    {
      "canonical_text": "Final Memory improvement benchmark page should say Verdant Ledger is a proper benchmark because it compares paired conditions, groups reasoning modes, verifies Memory evidence, and reports token cost.",
      "summary": "Final page explains why Verdant Ledger is a proper benchmark.",
      "memory_type": "reference",
      "confidence": 0.95,
      "importance": 4,
      "tags": ["mi-inductive", "mi-final"],
      "sources": [{"file_path": "docs/evaluation/benchmark-method.md", "source_kind": "file", "excerpt": "proper benchmark paired conditions reasoning modes token cost"}]
    }
  ]
}
JSON

$memory_cmd capture task --file "$payload"
$memory_cmd curate --project memory
