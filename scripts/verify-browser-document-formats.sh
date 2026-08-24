#!/usr/bin/env bash
set -euo pipefail

workspace_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
chromedriver="${CHROMEDRIVER:-$(command -v chromedriver || true)}"

if [[ -z "${chromedriver}" || ! -x "${chromedriver}" ]]; then
  echo "ChromeDriver is required for browser document-format tests" >&2
  exit 1
fi

export CHROMEDRIVER="${chromedriver}"
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
export WASM_BINDGEN_TEST_TIMEOUT="${WASM_BINDGEN_TEST_TIMEOUT:-120}"

cd "${workspace_dir}"
# lopdf's unoptimized parser can exceed the WebAssembly debug function-local
# limit. This is the same optimized shape shipped to users and exercised in CI.
cargo test --release -p kael_pdf --target wasm32-unknown-unknown --test browser
cargo test --release -p kael_office --target wasm32-unknown-unknown --test browser

echo "Browser PDF and Office byte tests passed"
