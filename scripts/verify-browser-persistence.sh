#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
chromedriver="${CHROMEDRIVER:-$(command -v chromedriver || true)}"

if [[ -z "${chromedriver}" || ! -x "${chromedriver}" ]]; then
  echo "ChromeDriver is required for browser persistence tests" >&2
  exit 1
fi

export CHROMEDRIVER="${chromedriver}"
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
export WASM_BINDGEN_TEST_TIMEOUT="${WASM_BINDGEN_TEST_TIMEOUT:-60}"

cd "${workspace_dir}"
cargo test -p kael_storage --target wasm32-unknown-unknown --lib
cargo test -p kael_document --target wasm32-unknown-unknown --lib

echo "Browser storage and document persistence tests passed"
