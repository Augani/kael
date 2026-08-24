#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-smoke"

bash "${workspace_dir}/scripts/build-web.sh" \
  --workspace "${workspace_dir}" \
  --package kael_ui \
  --example browser_smoke \
  --features browser \
  --out-dir "${output_dir}" \
  --out-name browser_smoke \
  --html "${workspace_dir}/crates/kael_ui/examples/browser_smoke/index.html"

echo "Built ${output_dir}"
echo "Serve it with: python3 -m http.server 8000 --directory ${output_dir}"
