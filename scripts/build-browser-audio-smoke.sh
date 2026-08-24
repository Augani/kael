#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${workspace_dir}/target/browser-audio-smoke"

bash "${workspace_dir}/scripts/build-web.sh" \
  --package kael_audio \
  --example browser_audio_smoke \
  --features browser \
  --out-dir "${output_dir}" \
  --out-name browser_audio_smoke \
  --html "${workspace_dir}/crates/kael_audio/examples/browser_audio_smoke.html"

test -s "${output_dir}/index.html"
test -s "${output_dir}/browser_audio_smoke.js"
test -s "${output_dir}/browser_audio_smoke_bg.wasm"
echo "Built ${output_dir}"
