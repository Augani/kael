#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-suite-smoke"

bash "${workspace_dir}/scripts/build-web.sh" \
  --workspace "${workspace_dir}" \
  --package kael_ui \
  --example suite_scale_smoke \
  --features browser \
  --out-dir "${output_dir}" \
  --out-name suite_scale_smoke \
  --html "${workspace_dir}/crates/kael_ui/examples/suite_scale_smoke/index.html"

echo "Built ${output_dir}"
