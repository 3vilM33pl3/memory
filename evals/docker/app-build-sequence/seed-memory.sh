#!/usr/bin/env sh
set -eu

memory_cmd="cargo run --quiet --bin memory -- --config /workspace/evals/docker/app-build-sequence/config.eval.toml"

$memory_cmd remember --project memory --type project --title "Memory product positioning" --summary "Memory is a local-first memory system for coding agents." --note "Memory's user-facing features include multi-backend embeddings, graph-aware search, distributed agents and watchers, activity tracking, get-up-to-speed briefings, and automated evaluations."
$memory_cmd remember --project memory --type reference --title "Memory evaluation explanation" --summary "Memory evaluations compare paired conditions." --note "A strong Memory evaluation runs the same task under no-memory and full-memory conditions, captures token usage and latency, verifies Memory evidence, and compares paired item scores instead of unrelated aggregate runs."
$memory_cmd remember --project memory --type reference --title "Memory graph and query behavior" --summary "Graph-aware search augments retrieval." --note "Graph-aware search uses code and relationship context to explain why memories are related; the query view should show returned memories and the grounded answer created from them."
$memory_cmd remember --project memory --type reference --title "New agent onboarding" --summary "New agents should query and resume before work." --note "A new agent should use get-up-to-speed, query/resume, plan checkpoints, activity history, and verified project memories before making changes."
$memory_cmd remember --project memory --type reference --title "Dockerized evaluation convention" --summary "Dockerized evals isolate the full stack." --note "The Dockerized build sequence runs Postgres with pgvector, the Memory service, the eval runner, and Codex in one reproducible Compose stack with artifacts written to target/memory-evals."
