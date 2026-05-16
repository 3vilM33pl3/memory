#!/usr/bin/env bash
set -euo pipefail

repo_url="${MEMORY_AGENT_BENCH_REPO:-https://github.com/HUST-AI-HYZ/MemoryAgentBench.git}"
commit="${MEMORY_AGENT_BENCH_COMMIT:-569241d877899d5c36d7d3b789de6c2489ea6cba}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
integration_dir="$repo_root/evals/external/memory-agent-bench"
checkout="${MEMORY_AGENT_BENCH_CHECKOUT:-$repo_root/target/memory-agent-bench}"

mkdir -p "$(dirname "$checkout")"
if [ ! -d "$checkout/.git" ]; then
  git clone "$repo_url" "$checkout"
fi

git -C "$checkout" fetch --depth 1 origin "$commit"
git -C "$checkout" checkout --detach "$commit"
git -C "$checkout" reset --hard "$commit" >/dev/null

mkdir -p "$checkout/configs/agent_conf/MemoryLayer"
cp "$integration_dir/configs/agent_conf/MemoryLayer_memory-gpt-4o-mini.yaml" \
  "$checkout/configs/agent_conf/MemoryLayer/MemoryLayer_memory-gpt-4o-mini.yaml"

cp "$integration_dir/overlays/memory_layer_adapter.py" "$checkout/memory_layer_adapter.py"
python3 "$integration_dir/overlays/patch_memory_agent_bench.py" "$checkout"

printf '%s\n' "$checkout"
