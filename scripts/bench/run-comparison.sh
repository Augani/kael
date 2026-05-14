#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

BASELINE_DIR="${PROJECT_ROOT}/benchmarks/baselines"
OUTPUT_DIR="${PROJECT_ROOT}/benchmark-results"
REGRESSION_THRESHOLD="${REGRESSION_THRESHOLD:-15.0}"

mkdir -p "${OUTPUT_DIR}"

echo "=== GPUI Benchmark Comparison ==="
echo ""

BASELINE_FILE="${BASELINE_DIR}/kael-messaging-baseline.json"
if [[ ! -f "${BASELINE_FILE}" ]]; then
    echo "Baseline not found at ${BASELINE_FILE}"
    echo "Generate one with: bash scripts/bench/generate-baseline.sh"
    exit 1
fi

echo "Running benchmarks..."
cd "${PROJECT_ROOT}"
cargo run --example perf_bench -- --output "${OUTPUT_DIR}/current.json" --compare

echo ""
echo "Comparison complete."
