#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-game-input-smoke"

bash "${workspace_dir}/scripts/build-web.sh" \
  --package kael_ui \
  --example browser_game_input_smoke \
  --features browser,game-input \
  --out-dir "${output_dir}" \
  --out-name browser_game_input_smoke \
  --html "${workspace_dir}/crates/kael_ui/examples/browser_game_input_smoke.html"

test -s "${output_dir}/index.html"
test -s "${output_dir}/browser_game_input_smoke.js"
test -s "${output_dir}/browser_game_input_smoke_bg.wasm"
echo "Built ${output_dir}"
