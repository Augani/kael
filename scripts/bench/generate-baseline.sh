#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

BASELINE_DIR="${PROJECT_ROOT}/benchmarks/baselines"
mkdir -p "${BASELINE_DIR}"

echo "=== Kael Baseline Generation ==="
echo ""

cd "${PROJECT_ROOT}"
cargo bench -p kael --bench framework -- --output "${BASELINE_DIR}/kael-messaging-baseline.json"

echo ""
echo "Baseline written to ${BASELINE_DIR}/kael-messaging-baseline.json"
