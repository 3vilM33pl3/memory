#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
integration_dir="$repo_root/evals/external/memory-agent-bench"
checkout="$("$integration_dir/scripts/prepare.sh")"

: "${MEMORY_AGENT_BENCH_MEMORY_API_TOKEN:?Set MEMORY_AGENT_BENCH_MEMORY_API_TOKEN to the Memory service API token}"

dataset_config="${MEMORY_AGENT_BENCH_DATASET_CONFIG:-configs/data_conf/Accurate_Retrieval/LongMemEval/Longmemeval_s.yaml}"
agent_config="${MEMORY_AGENT_BENCH_AGENT_CONFIG:-configs/agent_conf/MemoryLayer/MemoryLayer_memory-gpt-4o-mini.yaml}"
max_queries="${MEMORY_AGENT_BENCH_MAX_QUERIES:-4}"

cd "$checkout"
python3 main.py \
  --agent_config "$agent_config" \
  --dataset_config "$dataset_config" \
  --max_test_queries_ablation "$max_queries" \
  "$@"
