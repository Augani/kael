#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
wasm_path="${1:-${workspace_dir}/target/browser-smoke/browser_smoke_bg.wasm}"
javascript_path="${2:-${workspace_dir}/target/browser-smoke/browser_smoke.js}"

max_wasm_bytes=12582912
max_gzip_bytes=5242880
max_javascript_bytes=102400

if [[ ! -s "${wasm_path}" || ! -s "${javascript_path}" ]]; then
  echo "browser size budget requires nonempty Wasm and JavaScript artifacts" >&2
  exit 1
fi

wasm_bytes="$(wc -c < "${wasm_path}" | tr -d ' ')"
gzip_bytes="$(gzip -9 -c "${wasm_path}" | wc -c | tr -d ' ')"
javascript_bytes="$(wc -c < "${javascript_path}" | tr -d ' ')"

if (( wasm_bytes > max_wasm_bytes )); then
  echo "browser Wasm exceeds raw budget: ${wasm_bytes} > ${max_wasm_bytes}" >&2
  exit 1
fi
if (( gzip_bytes > max_gzip_bytes )); then
  echo "browser Wasm exceeds gzip budget: ${gzip_bytes} > ${max_gzip_bytes}" >&2
  exit 1
fi
if (( javascript_bytes > max_javascript_bytes )); then
  echo "browser JavaScript exceeds budget: ${javascript_bytes} > ${max_javascript_bytes}" >&2
  exit 1
fi

echo "Browser artifact budget passed: Wasm ${wasm_bytes} bytes raw / ${gzip_bytes} bytes gzip; JavaScript ${javascript_bytes} bytes"
