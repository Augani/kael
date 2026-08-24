#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
chromedriver="${CHROMEDRIVER:-$(command -v chromedriver || true)}"

if [[ -z "${chromedriver}" || ! -x "${chromedriver}" ]]; then
  echo "ChromeDriver is required for browser diagnostics tests" >&2
  exit 1
fi

export CHROMEDRIVER="${chromedriver}"
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
export WASM_BINDGEN_TEST_TIMEOUT="${WASM_BINDGEN_TEST_TIMEOUT:-60}"

cd "${workspace_dir}"
cargo test -p kael_diagnostics --target wasm32-unknown-unknown --lib \
  browser_tests::reports_round_trip_through_origin_local_storage

echo "Browser diagnostics persistence and trace tests passed"
