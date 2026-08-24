#!/usr/bin/env bash
set -euo pipefail

chromedriver="${CHROMEDRIVER:-$(command -v chromedriver || true)}"

if [[ -z "${chromedriver}" || ! -x "${chromedriver}" ]]; then
  echo "ChromeDriver is required for browser notification/share tests" >&2
  exit 1
fi

export CHROMEDRIVER="${chromedriver}"
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner

# Drivers are injected behind Rust traits, so these real-Chrome tests prove
# permission/cancellation policy without opening OS notification or share UI.
cargo test -p kael_notifications --target wasm32-unknown-unknown --lib
cargo test -p kael_share --target wasm32-unknown-unknown --lib
