#!/usr/bin/env bash
# Seeds the shared classroom exercise project into the running bundled stack.
# Idempotent: reseeding refreshes the same project. Run BEFORE enabling
# CLASSROOM_READ_ONLY (a read-only service rejects seeding, by design).
#
#   ./classroom/seed.sh              # seeds project "classroom"
#   ./classroom/seed.sh myclass      # seeds a different project name
set -euo pipefail

project="${1:-classroom}"
compose_args=(-f compose.yaml -f classroom/compose.classroom.yaml)

cd "$(dirname "$0")/.."

if ! docker compose "${compose_args[@]}" ps --status running memory | grep -q memory; then
    echo "The memory service is not running. Start it first:"
    echo "  docker compose ${compose_args[*]} up -d"
    exit 1
fi

echo "Seeding exercise project '$project' (keyless, no API keys needed)..."
docker compose "${compose_args[@]}" exec memory memory demo --project "$project"

echo
echo "Done. Students can open http://localhost:4040, pick project '$project',"
echo "and start the worksheet. To lock the class into read-only student mode:"
echo "  CLASSROOM_READ_ONLY=true docker compose ${compose_args[*]} up -d"
