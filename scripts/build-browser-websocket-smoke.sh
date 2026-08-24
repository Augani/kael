#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-websocket-smoke"
fixture_dir="${workspace_dir}/crates/kael_net/examples/browser_websocket_smoke"

bash "${workspace_dir}/scripts/build-web.sh" \
  --workspace "${workspace_dir}" \
  --package kael_net \
  --example browser_websocket_smoke \
  --features browser \
  --out-dir "${output_dir}" \
  --out-name browser_websocket_smoke \
  --html "${fixture_dir}/index.html"

test -s "${output_dir}/index.html"
test -s "${output_dir}/browser_websocket_smoke.js"
test -s "${output_dir}/browser_websocket_smoke_bg.wasm"

echo "Built ${output_dir}"
