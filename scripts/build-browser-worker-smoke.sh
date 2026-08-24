#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-worker-smoke"
fixture_dir="${workspace_dir}/crates/kael_ui/examples/browser_worker_smoke"

bash "${workspace_dir}/scripts/build-web.sh" \
  --workspace "${workspace_dir}" \
  --package kael_ui \
  --example browser_worker_smoke \
  --features browser \
  --out-dir "${output_dir}" \
  --out-name browser_worker_smoke \
  --html "${fixture_dir}/index.html"

cp "${fixture_dir}/browser_worker_bootstrap.js" \
  "${output_dir}/browser_worker_bootstrap.js"

test -s "${output_dir}/index.html"
test -s "${output_dir}/browser_worker_smoke.js"
test -s "${output_dir}/browser_worker_smoke_bg.wasm"
test -s "${output_dir}/browser_worker_bootstrap.js"

echo "Built ${output_dir}"
echo "Serve it with: python3 -m http.server 8000 --directory ${output_dir}"
